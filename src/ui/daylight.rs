//! The map's light follows the time of day (Razdor's, «Освещение», off by default). Style only: the
//! light is a tint over the drawn map and additive glows at the towns and the armies. No rule,
//! fog, path or save reads it.
//!
//! The sun's elevation `e` is a sine of the hour (1 at noon, -1 at midnight). Brightness `s` is a
//! smooth step of `e`, so the night keeps a floor and the day is the original's colours (only noon
//! is exactly plain). Warmth `w` is a bell around dawn and dusk.

use razdor::local;
use serde::{Deserialize, Serialize};
use std::cell::Cell;

/// The light of one frame: a tint multiplied over the map, and the glows' strength (0 by day).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub tint: [f32; 3],
    pub glow: f32,
}

/// The night's colour (blue), and the dawn and dusk warmth (per channel, at full warmth).
const NIGHT: [f32; 3] = [0.50, 0.58, 0.82];
const WARM: [f32; 3] = [1.0, 0.80, 0.58];

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The light at `hour` (0..24, fractional; 0 is midnight, 12 noon).
pub fn light_at(hour: f32) -> Light {
    let e = (std::f32::consts::TAU * (hour - 6.0) / 24.0).sin();
    let s = smoothstep(-0.3, 1.0, e);
    let w = (-((e - 0.08) / 0.20).powi(2)).exp();
    let mut tint = [0.0; 3];
    for c in 0..3 {
        let base = lerp(NIGHT[c], 1.0, s);
        tint[c] = (base * lerp(1.0, WARM[c], w * 0.7)).clamp(0.0, 1.0);
    }
    Light { tint, glow: 1.0 - smoothstep(-0.15, 0.20, e) }
}

/// The hour in -12..12 that is the shortest way from `from` to `to` (on the 24 h circle).
fn signed_gap(from: f32, to: f32) -> f32 {
    (to - from + 12.0).rem_euclid(24.0) - 12.0
}

/// The shown hour moved a step toward `target`, the shortest way round the day: a fraction of the
/// gap each second (`dt` in seconds). A gap over 6 h (a load, a long jump) snaps to the target.
pub fn ease_hour(shown: f32, target: f32, dt: f32) -> f32 {
    let d = signed_gap(shown, target);
    if d.abs() > 6.0 {
        return target.rem_euclid(24.0);
    }
    (shown + d * (dt * 2.5).clamp(0.0, 1.0)).rem_euclid(24.0)
}

/// The setting «Освещение» (`Settings::map_light`): off, the game's clock, a real-time day
/// cycle, or a fixed hour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum LightMode {
    #[default]
    Off,
    GameClock,
    Cycle,
    Fixed,
}

impl LightMode {
    /// The next mode in the settings row: Off, game clock, cycle, fixed hour, Off.
    pub fn next(self) -> LightMode {
        match self {
            LightMode::Off => LightMode::GameClock,
            LightMode::GameClock => LightMode::Cycle,
            LightMode::Cycle => LightMode::Fixed,
            LightMode::Fixed => LightMode::Off,
        }
    }
}

/// The settings the drawing takes in every frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Prefs {
    mode: LightMode,
    /// Real minutes per game day in `Cycle`.
    cycle_minutes: u32,
    /// The hour in `Fixed`.
    fixed_hour: u32,
}

local! {
    static PREFS: Cell<Prefs> = Cell::new(Prefs { mode: LightMode::Off, cycle_minutes: 24, fixed_hour: 18 });
    /// The hour the game-clock mode shows, eased toward the game's (`None` until the first frame).
    static SHOWN_HOUR: Cell<Option<f32>> = Cell::new(None);
    /// `RAZDOR_LIGHT_HOUR` (a snapshot's fixed hour, read once): it wins over the settings.
    static ENV_HOUR: Option<f32> = std::env::var("RAZDOR_LIGHT_HOUR").ok().and_then(|s| s.trim().parse::<f32>().ok());
}

/// Takes the settings «Освещение» in (every frame).
pub fn set_prefs(mode: LightMode, cycle_minutes: u32, fixed_hour: u32) {
    PREFS.set(Prefs { mode, cycle_minutes, fixed_hour });
}

/// The light of this frame, or None when it is off (the frame is then drawn as before).
/// `game_hour` is the game's hour of day, `real_secs` the wall clock, `dt` the frame's length.
pub fn frame_light(game_hour: f32, real_secs: f64, dt: f32) -> Option<Light> {
    if let Some(h) = ENV_HOUR.with(|h| *h) {
        return Some(light_at(h.rem_euclid(24.0)));
    }
    let prefs = PREFS.get();
    let hour = match prefs.mode {
        LightMode::Off => return None,
        LightMode::GameClock => {
            let shown = match SHOWN_HOUR.get() {
                Some(h) => ease_hour(h, game_hour, dt),
                None => game_hour.rem_euclid(24.0),
            };
            SHOWN_HOUR.set(Some(shown));
            shown
        }
        LightMode::Cycle => {
            let day_secs = f64::from(prefs.cycle_minutes.max(1)) * 60.0;
            ((real_secs / day_secs * 24.0).rem_euclid(24.0)) as f32
        }
        LightMode::Fixed => (prefs.fixed_hour % 24) as f32,
    };
    Some(light_at(hour))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(t: [f32; 3]) -> f32 {
        0.299 * t[0] + 0.587 * t[1] + 0.114 * t[2]
    }

    #[test]
    fn noon_is_plain_and_bright() {
        let noon = light_at(12.0);
        for c in noon.tint {
            assert!((c - 1.0).abs() < 1e-3, "noon tint {c}");
        }
        assert!(noon.glow < 0.01);
    }

    #[test]
    fn midnight_glows_fully() {
        assert!(light_at(0.0).glow > 0.99);
    }

    #[test]
    fn night_keeps_a_floor_of_light() {
        for i in 0..96 {
            let h = i as f32 * 0.25;
            let l = lum(light_at(h).tint);
            assert!(l >= 0.55, "hour {h}: luminance {l}");
        }
    }

    #[test]
    fn dusk_is_warmer_than_night() {
        // The warmth is a bell round e = 0.08 (about 18:00): there the red keeps most of its
        // share (r/b near 0.9), while the night's blue leaves r/b near 0.6. The formula's warmth
        // at 19:00 is already faded, so the test looks at the dusk's peak.
        let dusk = light_at(18.0).tint;
        let night = light_at(1.0).tint;
        assert!(dusk[0] / dusk[2] > 0.85, "dusk r/b {}", dusk[0] / dusk[2]);
        assert!(dusk[0] / dusk[2] > night[0] / night[2] + 0.2);
    }

    #[test]
    fn night_is_blue() {
        let t = light_at(1.0).tint;
        assert!(t[2] > t[0], "night tint {t:?}");
    }

    #[test]
    fn adjacent_quarter_hours_change_little() {
        // The steepest step is at dawn and dusk (about 0.08 a channel in a quarter hour, from the
        // warmth bell); the test allows that and no more.
        let mut prev = light_at(0.0).tint;
        for i in 1..=96 {
            let t = light_at(i as f32 * 0.25).tint;
            for c in 0..3 {
                assert!((t[c] - prev[c]).abs() < 0.1, "hour {}: channel {c} jumps", i as f32 * 0.25);
            }
            prev = t;
        }
    }

    #[test]
    fn ease_wraps_the_short_way() {
        let eased = ease_hour(23.5, 0.5, 0.1);
        assert!(eased > 23.5 && eased < 24.0, "eased to {eased}, not back round the day");
    }

    #[test]
    fn ease_snaps_on_a_long_jump() {
        assert_eq!(ease_hour(2.0, 15.0, 0.016), 15.0);
        assert_eq!(ease_hour(20.0, 3.0, 0.016), 3.0);
    }

    #[test]
    fn ease_converges() {
        let mut h = 0.0;
        for _ in 0..600 {
            h = ease_hour(h, 12.0, 1.0 / 60.0);
        }
        assert!((h - 12.0).abs() < 1e-3, "converged to {h}");
    }

    #[test]
    fn modes_cycle_and_default_off() {
        assert_eq!(LightMode::default(), LightMode::Off);
        assert_eq!(LightMode::Off.next(), LightMode::GameClock);
        assert_eq!(LightMode::GameClock.next(), LightMode::Cycle);
        assert_eq!(LightMode::Cycle.next(), LightMode::Fixed);
        assert_eq!(LightMode::Fixed.next(), LightMode::Off);
    }

    #[test]
    fn off_draws_nothing_and_fixed_follows_the_setting() {
        set_prefs(LightMode::Off, 24, 18);
        assert!(frame_light(12.0, 0.0, 0.016).is_none());
        set_prefs(LightMode::Fixed, 24, 1);
        assert_eq!(frame_light(12.0, 0.0, 0.016), Some(light_at(1.0)));
        set_prefs(LightMode::Off, 24, 18);
    }

    #[test]
    fn cycle_runs_one_day_per_cycle_minutes() {
        set_prefs(LightMode::Cycle, 12, 18);
        // Half of a 12-minute day in: noon.
        let noon = frame_light(0.0, 6.0 * 60.0, 0.016).expect("cycle is on");
        assert_eq!(noon, light_at(12.0));
        set_prefs(LightMode::Off, 24, 18);
    }
}
