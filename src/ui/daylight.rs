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

/// The sun at `hour` (0..24, fractional; 0 is midnight, 12 noon): its elevation `e` (a sine of the
/// hour), the brightness `s` (a smooth step of `e`) and the dawn and dusk warmth `w` (a bell).
fn sun(hour: f32) -> (f32, f32, f32) {
    let e = (std::f32::consts::TAU * (hour - 6.0) / 24.0).sin();
    let s = smoothstep(-0.3, 1.0, e);
    let w = (-((e - 0.08) / 0.20).powi(2)).exp();
    (e, s, w)
}

/// The light at `hour` (0..24, fractional; 0 is midnight, 12 noon).
pub fn light_at(hour: f32) -> Light {
    let (e, s, w) = sun(hour);
    let mut tint = [0.0; 3];
    for c in 0..3 {
        let base = lerp(NIGHT[c], 1.0, s);
        tint[c] = (base * lerp(1.0, WARM[c], w * 0.7)).clamp(0.0, 1.0);
    }
    Light { tint, glow: 1.0 - smoothstep(-0.15, 0.20, e) }
}

/// The sky's colour by day, at night (moonlight) and at dawn and dusk (per channel, at full warmth).
const SKY_DAY: [f32; 3] = [0.55, 0.70, 0.85];
const SKY_NIGHT: [f32; 3] = [0.45, 0.52, 0.70];
const SKY_WARM: [f32; 3] = [0.95, 0.70, 0.45];

/// The sky's colour at `hour`: the day's blended toward the night's by the sun's brightness, and
/// toward the dawn and dusk warmth by the same bell as the light's tint.
pub fn sky_at(hour: f32) -> [f32; 3] {
    let (_, s, w) = sun(hour);
    let mut sky = [0.0; 3];
    for c in 0..3 {
        let base = lerp(SKY_NIGHT[c], SKY_DAY[c], s);
        sky[c] = lerp(base, SKY_WARM[c], w * 0.7);
    }
    sky
}

/// The relief's light of one frame (`relief_at`): the light's direction in the screen's axes (x
/// right, y down, z toward the viewer), the raking strength, the cavities' darkening, and the factor
/// of the painted shadows' alpha. Style only; the map's sprites and ground read it in their shaders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Relief {
    pub light: [f32; 3],
    pub strength: f32,
    pub cavity: f32,
    pub shadow_k: f32,
}

/// The sun's direction on the screen: lower right, so the original's painted shadows (which fall
/// toward the upper left) agree with it. Only the elevation changes with the hour.
const SUN_XY: [f32; 2] = [0.62, 0.62];
/// The moon's elevation in radians, which lights the night (the sun's elevation by day).
const MOON_ELEV: f32 = 0.6;
/// The raking strength at night.
const NIGHT_STRENGTH: f32 = 0.3;

/// The sun's elevation in radians for the sine `e` of the hour: 0.20 at the horizon, 1.15 at noon.
fn sun_elevation(e: f32) -> f32 {
    lerp(0.20, 1.15, e.clamp(0.0, 1.0))
}

/// The relief's light at `hour` (0..24, fractional). By day the sun's direction, its strength and
/// the painted shadows' factor follow `e` (a low sun rakes hard, a high one gently); at night the
/// moon. The blend runs over the sun's sine from -0.5 to 0 (about 4:00 to 6:00), so nothing jumps
/// at dawn or dusk.
pub fn relief_at(hour: f32) -> Relief {
    let (e, _, _) = sun(hour);
    let up = e.clamp(0.0, 1.0);
    // 1 by day (the sun up), 0 at night (the moon).
    let day = smoothstep(-0.5, 0.0, e);
    let elev = lerp(MOON_ELEV, sun_elevation(e), day);
    let (tan, [sx, sy]) = (elev.tan(), SUN_XY);
    let len = (sx * sx + sy * sy + tan * tan).sqrt();
    let strength = lerp(NIGHT_STRENGTH, lerp(0.65, 0.25, up), day);
    let cavity = lerp(0.20, 0.35, day);
    let shadow_k = lerp(0.55, lerp(1.15, 1.0, smoothstep(0.3, 1.0, up)), day);
    Relief { light: [sx / len, sy / len, tan / len], strength, cavity, shadow_k }
}

impl Relief {
    /// The same relief for a ground drawn turned by `yaw_deg` on the 2.5D view: the light's screen
    /// direction is turned back into the ground's axes (screen = R(yaw) · ground, so ground =
    /// R(−yaw) · screen). Yaw 0 is the identity.
    pub fn turned(self, yaw_deg: f32) -> Relief {
        let (s, c) = yaw_deg.to_radians().sin_cos();
        let [x, y, z] = self.light;
        Relief { light: [c * x + s * y, -s * x + c * y, z], ..self }
    }
}

/// The shadows (`Settings::map_shadows`, «Тени»): the original's painted ones (the default), or
/// cast by the sun in real time (`casts_at`, `sun_relief`; the sprites' painted shadows are removed).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowMode {
    #[default]
    Painted,
    Sun,
}

/// A cast shadow of one light: its direction on the map (a unit vector, x east, y south: away from
/// the light), its length per unit of height (the cotangent of the elevation, clamped) and its alpha
/// at full light (0 when the light is down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cast {
    pub dir: [f32; 2],
    pub len: f32,
    pub alpha: f32,
}

/// The sun's shadow and the moon's at one hour (`casts_at`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Casts {
    pub sun: Cast,
    pub moon: Cast,
}

/// The sun's shadow alpha at full light (by day), fading through the dusk.
const SUN_ALPHA: f32 = 0.6;
/// The moon's shadow alpha, from dusk to dawn.
const MOON_ALPHA: f32 = 0.15;
/// The shortest and the longest shadow per unit of height (the dawn's cap, `perf-analysis`).
const LEN_MIN: f32 = 0.25;
const LEN_MAX: f32 = 2.6;
/// The moon's bearing toward the light: fixed in the south-west (x east, y south).
const MOON_BEARING: [f32; 2] = [-std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2];

/// The sun's bearing toward the light at `hour` (unit, x east, y south): east at 6:00, south at 12:00,
/// west at 18:00, so the shadows point away from it (west at 6:00, north at noon, east at 18:00).
fn sun_bearing(hour: f32) -> [f32; 2] {
    let a = std::f32::consts::TAU * (hour - 6.0) / 24.0;
    [a.cos(), a.sin()]
}

/// The shadow length per unit of height at an elevation: its cotangent, clamped to the cap.
fn cast_len(elev: f32) -> f32 {
    (1.0 / elev.tan()).clamp(LEN_MIN, LEN_MAX)
}

/// The moon's weight at `hour`: 0 by day, 1 from 20:00 to 5:00, ramps over 19:00 to 20:00 and 5:00
/// to 6:00 (the dawn). Continuous over the day's wrap at midnight.
fn night_weight(hour: f32) -> f32 {
    let h = if hour < 12.0 { hour + 24.0 } else { hour };
    smoothstep(19.0, 20.0, h) * (1.0 - smoothstep(29.0, 30.0, h))
}

/// The cast shadows at `hour` (0..24, fractional). The sun's: away from the sun's bearing, its length
/// from its elevation (the relief's curve, `sun_elevation`), alpha by day and fading through the dusk
/// (`e` below 0.1 of the sine). The moon's: fixed in the south-west, alpha from dusk to dawn.
pub fn casts_at(hour: f32) -> Casts {
    let (e, _, _) = sun(hour);
    let toward = sun_bearing(hour);
    let sun_cast = Cast {
        dir: [-toward[0], -toward[1]],
        len: cast_len(sun_elevation(e)),
        alpha: SUN_ALPHA * smoothstep(-0.12, 0.1, e),
    };
    let moon_cast = Cast {
        dir: [-MOON_BEARING[0], -MOON_BEARING[1]],
        len: cast_len(MOON_ELEV),
        alpha: MOON_ALPHA * night_weight(hour),
    };
    Casts { sun: sun_cast, moon: moon_cast }
}

/// The relief of `ShadowMode::Sun` at `hour`: the same strength, cavities and moon blend as
/// `relief_at`, but the light is in the map's axes (the sun travels in the world; by night the moon
/// in the south-west), and no painted shadow is kept (`shadow_k` 0).
pub fn sun_relief(hour: f32) -> Relief {
    let base = relief_at(hour);
    let (e, _, _) = sun(hour);
    let day = smoothstep(-0.5, 0.0, e);
    let elev = lerp(MOON_ELEV, sun_elevation(e), day);
    // The bearing turns from the moon's to the sun's the short way round (an angle, not a blend of the
    // two vectors: those pass near zero at the dawn's crossing and would jump).
    let (moon, toward) = (MOON_BEARING[1].atan2(MOON_BEARING[0]), sun_bearing(hour));
    let gap = (toward[1].atan2(toward[0]) - moon).rem_euclid(std::f32::consts::TAU);
    let gap = if gap > std::f32::consts::PI { gap - std::f32::consts::TAU } else { gap };
    let phi = moon + gap * day;
    let v = [phi.cos(), phi.sin(), elev.tan()];
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    Relief { light: [v[0] / len, v[1] / len, v[2] / len], shadow_k: 0.0, ..base }
}

// The hour of the last frame that had the light on (`None` when it was off or not yet drawn).
local! {
    static LAST_HOUR: Cell<Option<f32>> = Cell::new(None);
}

/// The sky's colour for the water (`terrain`'s `Sky`): the light's hour when the light is on, the
/// day's colour otherwise.
pub fn sky_rgb() -> [f32; 3] {
    match LAST_HOUR.get() {
        Some(h) => sky_at(h),
        None => SKY_DAY,
    }
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
    shadows: ShadowMode,
}

local! {
    static PREFS: Cell<Prefs> = Cell::new(Prefs { mode: LightMode::Off, cycle_minutes: 24, fixed_hour: 18, shadows: ShadowMode::Painted });
    /// `RAZDOR_SHADOWS=sun|painted` (a snapshot's shadows, read once): it wins over the settings.
    static ENV_SHADOWS: Option<ShadowMode> = std::env::var("RAZDOR_SHADOWS").ok().and_then(|s| match s.trim() {
        "sun" => Some(ShadowMode::Sun),
        "painted" => Some(ShadowMode::Painted),
        _ => None,
    });
    /// The hour the game-clock mode shows, eased toward the game's (`None` until the first frame).
    static SHOWN_HOUR: Cell<Option<f32>> = Cell::new(None);
    /// `RAZDOR_LIGHT_HOUR` (a snapshot's fixed hour, read once): it wins over the settings.
    static ENV_HOUR: Option<f32> = std::env::var("RAZDOR_LIGHT_HOUR").ok().and_then(|s| s.trim().parse::<f32>().ok());
}

/// Takes the settings «Освещение» and «Тени» in (every frame).
pub fn set_prefs(mode: LightMode, cycle_minutes: u32, fixed_hour: u32, shadows: ShadowMode) {
    PREFS.set(Prefs { mode, cycle_minutes, fixed_hour, shadows });
}

/// The shadows in effect: `RAZDOR_SHADOWS` when set, else the setting.
pub fn shadow_mode() -> ShadowMode {
    ENV_SHADOWS.with(|e| *e).unwrap_or(PREFS.get().shadows)
}

/// The light of this frame, or None when it is off (the frame is then drawn as before).
/// `game_hour` is the game's hour of day, `real_secs` the wall clock, `dt` the frame's length.
pub fn frame_light(game_hour: f32, real_secs: f64, dt: f32) -> Option<Light> {
    if let Some(h) = ENV_HOUR.with(|h| *h) {
        LAST_HOUR.set(Some(h.rem_euclid(24.0)));
        return Some(light_at(h.rem_euclid(24.0)));
    }
    let prefs = PREFS.get();
    let hour = match prefs.mode {
        LightMode::Off => {
            LAST_HOUR.set(None);
            return None;
        }
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
    LAST_HOUR.set(Some(hour));
    Some(light_at(hour))
}

/// The hour the lighting shows this frame, when the lighting is on (`frame_light`); None otherwise.
pub fn shown_hour() -> Option<f32> {
    LAST_HOUR.get()
}

/// The relief of this frame: [`relief_at`] the hour `frame_light` just set (`sun_relief` with the sun
/// shadows), None when the light is off. Call it after `frame_light` in the same frame.
pub fn frame_relief() -> Option<Relief> {
    let hour = LAST_HOUR.get()?;
    Some(if shadow_mode() == ShadowMode::Sun { sun_relief(hour) } else { relief_at(hour) })
}

/// Whether this frame's shadows are cast by the sun: the light is on and the shadows are «От солнца».
pub fn sun_shadows_on() -> bool {
    LAST_HOUR.get().is_some() && shadow_mode() == ShadowMode::Sun
}

/// The cast shadows of this frame (`casts_at` the hour `frame_light` just set), None unless
/// `sun_shadows_on`.
pub fn frame_casts() -> Option<Casts> {
    let hour = LAST_HOUR.get()?;
    (shadow_mode() == ShadowMode::Sun).then(|| casts_at(hour))
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
    fn the_sky_is_the_day_colour_at_noon_and_blue_at_night() {
        let noon = sky_at(12.0);
        for (c, d) in noon.iter().zip(SKY_DAY) {
            assert!((c - d).abs() < 1e-3, "noon sky {noon:?}");
        }
        let night = sky_at(1.0);
        assert!(night[2] > night[0], "night sky {night:?}");
    }

    #[test]
    fn the_dusk_sky_is_warmer_than_the_night_sky() {
        let dusk = sky_at(18.0);
        let night = sky_at(1.0);
        assert!(dusk[0] / dusk[2] > night[0] / night[2] + 0.2, "dusk {dusk:?} night {night:?}");
    }

    #[test]
    fn the_sky_is_the_day_colour_with_the_light_off_and_the_light_s_hour_on() {
        set_prefs(LightMode::Off, 24, 18, ShadowMode::Painted);
        assert!(frame_light(12.0, 0.0, 0.016).is_none());
        assert_eq!(sky_rgb(), SKY_DAY, "lighting off: the day's colour");
        set_prefs(LightMode::Fixed, 24, 1, ShadowMode::Painted);
        frame_light(12.0, 0.0, 0.016);
        assert_eq!(sky_rgb(), sky_at(1.0), "the light's hour");
        set_prefs(LightMode::Off, 24, 18, ShadowMode::Painted);
        frame_light(12.0, 0.0, 0.016);
        assert_eq!(sky_rgb(), SKY_DAY);
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
        set_prefs(LightMode::Off, 24, 18, ShadowMode::Painted);
        assert!(frame_light(12.0, 0.0, 0.016).is_none());
        set_prefs(LightMode::Fixed, 24, 1, ShadowMode::Painted);
        assert_eq!(frame_light(12.0, 0.0, 0.016), Some(light_at(1.0)));
        set_prefs(LightMode::Off, 24, 18, ShadowMode::Painted);
    }

    #[test]
    fn cycle_runs_one_day_per_cycle_minutes() {
        set_prefs(LightMode::Cycle, 12, 18, ShadowMode::Painted);
        // Half of a 12-minute day in: noon.
        let noon = frame_light(0.0, 6.0 * 60.0, 0.016).expect("cycle is on");
        assert_eq!(noon, light_at(12.0));
        set_prefs(LightMode::Off, 24, 18, ShadowMode::Painted);
    }

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    #[test]
    fn a_flat_normal_is_lit_exactly_one_at_every_hour() {
        // The shader's factor: dot(N, L) / L.z, with the flat normal N = (0, 0, 1).
        for i in 0..96 {
            let r = relief_at(i as f32 * 0.25);
            let f = dot([0.0, 0.0, 1.0], r.light) / r.light[2];
            assert!((f - 1.0).abs() < 1e-6, "hour {}: flat factor {f}", i as f32 * 0.25);
        }
    }

    #[test]
    fn the_light_comes_from_the_front_and_lower_right_of_the_screen() {
        for i in 0..96 {
            let l = relief_at(i as f32 * 0.25).light;
            assert!(l[2] > 0.0, "hour {}: the light is in front of the ground", i as f32 * 0.25);
            assert!(l[0] > 0.0 && l[1] > 0.0, "hour {}: lower right {l:?}", i as f32 * 0.25);
            assert!((dot(l, l) - 1.0).abs() < 1e-5, "hour {}: unit length", i as f32 * 0.25);
        }
    }

    #[test]
    fn a_low_sun_rakes_harder_than_noon() {
        let dawn = relief_at(7.0);
        let noon = relief_at(12.0);
        assert!(dawn.strength > noon.strength, "7:00 {} vs noon {}", dawn.strength, noon.strength);
        assert!((noon.strength - 0.25).abs() < 1e-5 && (noon.cavity - 0.35).abs() < 1e-5);
        assert!((noon.shadow_k - 1.0).abs() < 1e-5, "noon shadows are the painted ones");
    }

    #[test]
    fn the_painted_shadows_deepen_at_low_sun_and_fade_at_night() {
        assert!((relief_at(18.0).shadow_k - 1.15).abs() < 1e-5, "dusk {}", relief_at(18.0).shadow_k);
        assert!((relief_at(1.0).shadow_k - 0.55).abs() < 1e-5, "night {}", relief_at(1.0).shadow_k);
    }

    #[test]
    fn night_is_lit_by_the_moon_at_its_elevation() {
        let r = relief_at(1.0);
        let moon = [0.62, 0.62, 0.6f32.tan()];
        let len = dot(moon, moon).sqrt();
        for (c, m) in moon.iter().enumerate() {
            assert!((r.light[c] - m / len).abs() < 1e-5, "night light {:?}", r.light);
        }
        assert!((r.strength - NIGHT_STRENGTH).abs() < 1e-5 && (r.cavity - 0.20).abs() < 1e-5);
    }

    #[test]
    fn adjacent_quarter_hours_change_the_relief_little() {
        let mut prev = relief_at(0.0);
        for i in 1..=96 {
            let h = i as f32 * 0.25;
            let r = relief_at(h);
            for c in 0..3 {
                assert!((r.light[c] - prev.light[c]).abs() < 0.1, "hour {h}: light {c} jumps");
            }
            assert!((r.strength - prev.strength).abs() < 0.15, "hour {h}: strength jumps");
            assert!((r.cavity - prev.cavity).abs() < 0.1, "hour {h}: cavity jumps");
            assert!((r.shadow_k - prev.shadow_k).abs() < 0.15, "hour {h}: shadow factor jumps");
            prev = r;
        }
    }

    #[test]
    fn turning_the_ground_turns_the_light_back_into_its_axes() {
        let r = relief_at(12.0);
        assert_eq!(r.turned(0.0), r, "yaw 0 is the identity");
        // At yaw 90 the ground's north is the screen's right (view25): the light from the screen's
        // right comes from the ground's north, (0, -1).
        let east = Relief { light: [1.0, 0.0, 0.0], ..r }.turned(90.0);
        assert!(east.light[0].abs() < 1e-5 && (east.light[1] + 1.0).abs() < 1e-5, "{:?}", east.light);
        let turned = r.turned(37.0);
        assert!((dot(turned.light, turned.light) - dot(r.light, r.light)).abs() < 1e-5, "a turn keeps the length");
    }

    #[test]
    fn the_frame_relief_follows_the_frame_light() {
        set_prefs(LightMode::Off, 24, 18, ShadowMode::Painted);
        frame_light(12.0, 0.0, 0.016);
        assert!(frame_relief().is_none(), "lighting off: no relief");
        set_prefs(LightMode::Fixed, 24, 1, ShadowMode::Painted);
        frame_light(12.0, 0.0, 0.016);
        assert_eq!(frame_relief(), Some(relief_at(1.0)));
        set_prefs(LightMode::Off, 24, 18, ShadowMode::Painted);
        frame_light(12.0, 0.0, 0.016);
        assert!(frame_relief().is_none());
    }

    fn approx(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
    }

    #[test]
    fn the_sun_casts_west_at_six_north_at_noon_and_east_at_eighteen() {
        // The shadow points away from the sun: the sun is east at 6:00, so the shadow is west.
        assert!(approx(casts_at(6.0).sun.dir, [-1.0, 0.0]), "6:00 {:?}", casts_at(6.0).sun.dir);
        assert!(approx(casts_at(12.0).sun.dir, [0.0, -1.0]), "noon {:?}", casts_at(12.0).sun.dir);
        assert!(approx(casts_at(18.0).sun.dir, [1.0, 0.0]), "18:00 {:?}", casts_at(18.0).sun.dir);
    }

    #[test]
    fn the_shadow_is_longer_in_the_morning_than_at_noon_and_capped() {
        assert!(casts_at(7.0).sun.len > casts_at(12.0).sun.len, "7:00 {} noon {}", casts_at(7.0).sun.len, casts_at(12.0).sun.len);
        for i in 0..96 {
            let h = i as f32 * 0.25;
            let len = casts_at(h).sun.len;
            assert!((LEN_MIN..=LEN_MAX).contains(&len), "hour {h}: length {len}");
        }
        assert!((casts_at(6.0).sun.len - LEN_MAX).abs() < 1e-5, "the dawn is capped at {LEN_MAX}");
    }

    #[test]
    fn the_sun_shadow_is_strong_by_day_and_gone_at_night() {
        assert!((casts_at(10.0).sun.alpha - SUN_ALPHA).abs() < 1e-5, "by day {}", casts_at(10.0).sun.alpha);
        assert!(casts_at(1.0).sun.alpha < 1e-5 && casts_at(23.0).sun.alpha < 1e-5, "the sun is down");
    }

    #[test]
    fn the_moon_casts_from_the_south_west_at_night_only() {
        let night = casts_at(23.0).moon;
        assert!((night.alpha - MOON_ALPHA).abs() < 1e-5, "23:00 moon alpha {}", night.alpha);
        // The moon is in the south-west, so its shadow points north-east, whatever the hour.
        assert!(approx(night.dir, [std::f32::consts::FRAC_1_SQRT_2, -std::f32::consts::FRAC_1_SQRT_2]), "{:?}", night.dir);
        assert!(casts_at(1.0).moon.alpha > 0.11 && casts_at(5.0).moon.alpha > 0.11, "from dusk to dawn");
        for h in [9.0, 12.0, 16.0, 18.0] {
            assert!(casts_at(h).moon.alpha < 1e-5, "{h}:00 the moon is not up");
        }
        assert!(casts_at(19.5).moon.alpha < MOON_ALPHA && casts_at(19.5).moon.alpha > 0.0, "the moon comes up over the dusk");
        assert!(casts_at(6.0).moon.alpha < 1e-5, "gone at dawn");
    }

    #[test]
    fn the_casts_change_little_from_one_quarter_hour_to_the_next_all_day() {
        let mut prev = casts_at(0.0);
        for i in 1..=96 {
            let h = i as f32 * 0.25;
            let c = casts_at(h);
            assert!((c.sun.dir[0] - prev.sun.dir[0]).abs() < 0.1 && (c.sun.dir[1] - prev.sun.dir[1]).abs() < 0.1, "hour {h}: sun bearing jumps");
            // The steepest step is the dawn's cotangent (about 0.4 a quarter hour, before the cap bites).
            assert!((c.sun.len - prev.sun.len).abs() < 0.5, "hour {h}: sun length jumps {} -> {}", prev.sun.len, c.sun.len);
            assert!((c.sun.alpha - prev.sun.alpha).abs() < 0.25, "hour {h}: sun alpha jumps");
            assert!((c.moon.alpha - prev.moon.alpha).abs() < 0.06, "hour {h}: moon alpha jumps");
            prev = c;
        }
        // Midnight: 23:59 and 0:00 are the same night.
        let (late, early) = (casts_at(23.99), casts_at(0.0));
        assert!((late.moon.alpha - early.moon.alpha).abs() < 1e-3 && (late.sun.dir[0] - early.sun.dir[0]).abs() < 0.05);
    }

    #[test]
    fn the_sun_relief_lights_from_the_sun_s_bearing_in_the_map_s_axes() {
        // At 6:00 the sun is east, so the light comes from +x; at 18:00 from −x.
        assert!(sun_relief(6.0).light[0] > 0.5 && sun_relief(6.0).light[2] > 0.0, "{:?}", sun_relief(6.0).light);
        assert!(sun_relief(18.0).light[0] < -0.5, "{:?}", sun_relief(18.0).light);
        for i in 0..96 {
            let r = sun_relief(i as f32 * 0.25);
            assert_eq!(r.shadow_k, 0.0, "no painted shadows in the sun mode");
            assert!(r.light[2] > 0.0, "hour {}: the light is above the ground", i as f32 * 0.25);
            assert!((dot(r.light, r.light) - 1.0).abs() < 1e-5, "unit length");
        }
        // A night by the moon: the light comes from the south-west (−x, +y).
        let night = sun_relief(1.0).light;
        assert!(night[0] < 0.0 && night[1] > 0.0, "{night:?}");
    }

    #[test]
    fn the_sun_relief_changes_little_from_one_quarter_hour_to_the_next() {
        let mut prev = sun_relief(0.0);
        for i in 1..=96 {
            let h = i as f32 * 0.25;
            let r = sun_relief(h);
            // The dawn's blend turns the light from the moon's bearing to the sun's over two hours (a
            // 165-degree sweep, fastest in its middle): about 0.55 a component in a quarter hour at most.
            for c in 0..3 {
                assert!((r.light[c] - prev.light[c]).abs() < 0.6, "hour {h}: light {c} jumps");
            }
            prev = r;
        }
    }

    #[test]
    fn the_sun_shadows_are_the_frame_s_only_in_the_sun_mode_with_the_light_on() {
        set_prefs(LightMode::Fixed, 24, 10, ShadowMode::Painted);
        frame_light(12.0, 0.0, 0.016);
        assert!(frame_casts().is_none() && !sun_shadows_on(), "painted: no sun shadows");
        assert_eq!(frame_relief(), Some(relief_at(10.0)), "painted: the painted relief");
        set_prefs(LightMode::Fixed, 24, 10, ShadowMode::Sun);
        frame_light(12.0, 0.0, 0.016);
        assert_eq!(frame_casts(), Some(casts_at(10.0)));
        assert!(sun_shadows_on());
        assert_eq!(frame_relief(), Some(sun_relief(10.0)), "sun: the sun's relief");
        set_prefs(LightMode::Off, 24, 10, ShadowMode::Sun);
        frame_light(12.0, 0.0, 0.016);
        assert!(frame_casts().is_none() && frame_relief().is_none(), "lighting off: nothing, shadows or not");
        set_prefs(LightMode::Off, 24, 18, ShadowMode::Painted);
    }

    #[test]
    fn the_shadow_mode_defaults_to_painted_and_survives_serialization() {
        assert_eq!(ShadowMode::default(), ShadowMode::Painted);
        let back: ShadowMode = serde_json::from_str(&serde_json::to_string(&ShadowMode::Sun).unwrap()).unwrap();
        assert_eq!(back, ShadowMode::Sun);
    }
}
