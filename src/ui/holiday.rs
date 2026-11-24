//! Razdor's «Новогодний вид» (`Settings::map_holiday`, off by default): a winter and holiday look for
//! the world map and the main menu. Style only: no rule, path, AI, fog, save or replay reads it.
//!
//! This module holds the setting's per-frame switch and the pure season logic (tested without a
//! window). Each effect reads [`on`] / [`cheap`] in the module that draws it. The main menu asks once
//! per season (by default from 1 December to 15 January, or the window the remote config gives), and the
//! season of a January date is the year before.

use std::cell::Cell;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use macroquad::prelude::{FilterMode, Image, Texture2D};
use razdor::local;
use razdor::remote_config::{self, parse_date, Holiday, HolidayWindow};
use razdor::rules::marks::civil;

use super::audio::Settings;
use super::living;
use razdor::dt::lights::Kind;

local! {
    /// The setting «Новогодний вид» as the game sees it, taken in every frame (`set_pref`): the setting, or
    /// `RAZDOR_HOLIDAY=1`.
    static ON: Cell<bool> = Cell::new(false);
    /// The cheap path (the light quality is «Низкое» or the battery saver is on), taken in with `set_pref`.
    static CHEAP: Cell<bool> = Cell::new(false);
    /// `RAZDOR_HOLIDAY=1` turns the setting on whatever it says (read once).
    static ENV_ON: bool = std::env::var("RAZDOR_HOLIDAY").is_ok_and(|v| v.trim() == "1");
    /// Whether the game plays the Christmas mod, taken in on each mod change (`set_running_mod`).
    static MOD_RUNNING: Cell<bool> = Cell::new(false);
    /// The mod the game plays (`set_running_mod`), to tell a change of game apart.
    static PLAYED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// Counts up on each change of the game played or of the look: the main menu reloads when it moves
    /// (`game_generation`).
    static GENERATION: Cell<u64> = Cell::new(0);
    /// «Снегопад» (`Settings::map_snowfall`), taken in every frame (`set_snowfall`): on by default.
    static SNOWFALL: Cell<bool> = Cell::new(true);
}

/// Whether `RAZDOR_HOLIDAY=1` forces the setting on (test builds and screenshots).
pub fn env_forced() -> bool {
    ENV_ON.with(|e| *e)
}

/// Whether the setting is on as the game sees it: `map_holiday`, or `RAZDOR_HOLIDAY=1`.
pub fn wanted(map_holiday: bool) -> bool {
    map_holiday || env_forced()
}

/// Takes the setting «Новогодний вид» in (`wanted`), and whether the cheap path applies (every frame, from
/// `display::follow_settings`). The setting alone does not show the look: see [`on`].
pub fn set_pref(setting: bool, cheap: bool) {
    let before = on();
    ON.set(wanted(setting));
    CHEAP.set(cheap);
    bump_if_changed(before, on());
}

/// The setting as the game sees it (`wanted`), this frame.
pub fn setting() -> bool {
    ON.get()
}

/// Takes «Снегопад» (`Settings::map_snowfall`) in, every frame (from `display::follow_settings`). Off: no falling
/// snow on the map or in the main menu, the holiday's or the weather's; the snow on the land stays.
pub fn set_snowfall(on: bool) {
    SNOWFALL.set(on);
}

/// Whether falling snow is shown (`set_snowfall`, on by default). Read by the map's weather and the menu's snow.
pub fn snowfall() -> bool {
    SNOWFALL.get()
}

/// The look is on this frame: the setting is on and the Christmas mod is the game played (`look_active`).
/// Every effect reads this, never the setting.
pub fn on() -> bool {
    look_active(ON.get(), MOD_RUNNING.get())
}

/// The look is active: the setting is on and the Christmas mod is the game played (pure).
pub fn look_active(setting: bool, mod_running: bool) -> bool {
    setting && mod_running
}

/// Counts up on each change of the game played and of the look's state (`on`). The main menu compares it
/// with the value it last drew: a change reloads its art, music and snow.
pub fn game_generation() -> u64 {
    GENERATION.get()
}

fn bump_if_changed(before: bool, after: bool) {
    if before != after {
        GENERATION.set(GENERATION.get() + 1);
    }
}

/// Whether the game plays the Christmas mod: the mod it plays (`active`, None for the base game) is the
/// catalog's holiday mod (`christmas`, the `holiday.mod` id; None when the catalog names none).
pub fn is_christmas_mod(active: Option<&str>, christmas: Option<&str>) -> bool {
    matches!((active, christmas), (Some(a), Some(c)) if a == c)
}

/// Takes in the mod the game plays (called on each mod change). The Christmas mod's id is read from the
/// hub's cached catalog, so no network is needed; without a cache the mod is not the Christmas one.
pub fn set_running_mod(active: Option<&str>) {
    let christmas = razdor::hub::local_dir()
        .and_then(|local| razdor::hub::Hub { source: razdor::hub::Source::from_env(), local }.cached_catalog())
        .and_then(|c| c.holiday)
        .and_then(|h| h.mod_id);
    let before = on();
    // `RAZDOR_SCENE_HOLIDAY_MOD=1` (a snapshot aid only): the Christmas mod counts as played, since a
    // snapshot run has no hub catalog (its source is the mock) to name it.
    let forced = std::env::var("RAZDOR_SCENE_HOLIDAY_MOD").is_ok_and(|v| v == "1");
    MOD_RUNNING.set(forced || is_christmas_mod(active, christmas.as_deref()));
    if PLAYED.with(|p| p.borrow().as_deref() != active) {
        PLAYED.with(|p| *p.borrow_mut() = active.map(str::to_owned));
        GENERATION.set(GENERATION.get() + 1);
    }
    bump_if_changed(before, on());
}

/// Whether the game plays the Christmas mod (the look needs it, see [`on`]).
pub fn mod_running() -> bool {
    MOD_RUNNING.get()
}

/// Test hook: the game plays (or not) the Christmas mod, without the hub's catalog.
#[cfg(test)]
pub fn set_mod_running_for_tests(running: bool) {
    MOD_RUNNING.set(running);
}

/// Whether the look runs its cheap path: on, and the light quality is «Низкое» or the battery saver is on.
pub fn cheap() -> bool {
    on() && CHEAP.get()
}

/// The setting's change since the last frame (pure): `Some(true)` when it turned on, `Some(false)` when it
/// turned off, None when it did not change.
pub fn edge(seen: bool, setting: bool) -> Option<bool> {
    (seen != setting).then_some(setting)
}

/// What the switch-on path must do (pure): the Christmas mod's download (`download`: not on the disk) and the
/// switch of the game to it (`switch`: not the one played). A switch remembers the game played first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnSteps {
    pub download: bool,
    pub switch: bool,
}

/// The switch-on steps from what is in place: `installed` (the mod is in the hub folder), `active_is_christmas`
/// (the game played is the Christmas mod).
pub fn on_steps(installed: bool, active_is_christmas: bool) -> OnSteps {
    OnSteps { download: !installed, switch: !active_is_christmas }
}

/// What the switch-off path does (pure): when the Christmas mod is the game played, the game goes back to the
/// one remembered (`previous`, None: the base game); otherwise the game stays as it is.
pub fn off_target(christmas_played: bool, previous: Option<String>) -> Option<Option<String>> {
    christmas_played.then_some(previous)
}

/// Where a switch to the Christmas mod `id` stands (pure): done when the game plays it, still owed while a
/// mod switch runs or is queued (`busy`), and missed otherwise (the switch failed or was refused). A missed
/// switch keeps the switch pending for the next launch or turn-on.
#[derive(Debug, PartialEq, Eq)]
pub enum Awaited {
    Done,
    Waiting,
    Missed,
}

pub fn awaited(id: &str, active: Option<&str>, busy: bool) -> Awaited {
    if active == Some(id) {
        Awaited::Done
    } else if busy {
        Awaited::Waiting
    } else {
        Awaited::Missed
    }
}

/// The mod choice that waits behind a running switch (pure): the queued choice (`queued`, the latest one; the
/// outer Option is "one is queued", the inner the mod, None the base game) when it differs from the game
/// played, else None (nothing to switch).
pub fn queued_next(queued: Option<Option<String>>, active: Option<&str>) -> Option<Option<String>> {
    queued.filter(|next| next.as_deref() != active)
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

/// The season a date falls in under the remote config's holiday object: `on` false is no season at all, a
/// `window` (inclusive dates) names the season by its first date's year, and None is the built-in season
/// ([`season`]).
pub fn season_in(window: Option<HolidayWindow>, on: bool, y: i64, m: u32, d: u32) -> Option<i64> {
    if !on {
        return None;
    }
    match window {
        Some(w) => ((y, m, d) >= w.from && (y, m, d) <= w.to).then_some(w.from.0),
        None => season(y, m, d),
    }
}

/// The season of a date under the remote config (`remote_config::holiday`).
pub fn season_now(today: (i64, u32, u32)) -> Option<i64> {
    let Holiday { on, window } = remote_config::holiday();
    season_in(window, on, today.0, today.1, today.2)
}

/// Whether the season start is due: there is a season (`season`, its year) that the setting `holiday_asked`
/// (the season year last done) does not name yet.
pub fn due(asked: Option<i64>, season: Option<i64>) -> bool {
    season.is_some_and(|year| asked != Some(year))
}

/// Whether the season start is due now (the settings and today's date, see [`today`]).
pub fn start_due(s: &Settings, today: (i64, u32, u32)) -> bool {
    due(s.holiday_asked, season_now(today))
}

/// Today's date (UTC) as (year, month, day). `RAZDOR_TODAY=YYYY-MM-DD` replaces it (tests and
/// screenshots only).
pub fn today() -> (i64, u32, u32) {
    if let Some(d) = std::env::var("RAZDOR_TODAY").ok().as_deref().and_then(parse_date) {
        return d;
    }
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    civil((secs / 86_400) as i64)
}

/// The garlands' points on a building's light boxes (`garland_points`): sprite pixels, the colour index (0 red,
/// 1 green, 2 gold, 3 blue) and the dot's index in its box (the order of the frame's dots).
pub type Garland = (f32, f32, u8, u8);

/// Spacing of the garland dots along a box's edges, in sprite pixels.
pub const GARLAND_STEP: i32 = 6;

/// The garland dots of a building's light boxes: each window, door and gate box (`[x0, y0, x1, y1]`) gets a frame
/// of dots, one every [`GARLAND_STEP`] px: along the top edge `y0` from `x0` to `x1` (both ends included), down the
/// right side `x1`, then up the left side `x0` (the dots run clockwise). The dot index restarts in each box and
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

/// The garland controller's modes, run in order like a light string's box: 0 step (each light jumps to its next
/// colour once a second), 1 flash (two groups swap on and off every 0.5 s), 2 chase (a wave runs left to right),
/// 3 twinkle (each light blinks at random), 4 glow (all breathe together, the colours drift slowly).
pub const GARLAND_MODES: u8 = 5;
/// Seconds one mode runs before the controller moves on.
pub const GARLAND_MODE_SECS: f64 = 8.0;
/// Seconds at the end of a mode over which it cross-fades into the next.
pub const GARLAND_FADE_SECS: f64 = 0.3;
/// The brightness of a light that is off: a warm dim glow, not black.
pub const GARLAND_DIM: f32 = 0.25;
/// The chase wave's length in screen pixels.
pub const CHASE_PX: f64 = 600.0;

/// The controller at `t` seconds: the mode now, the mode it fades into, and the fade (0 until the last
/// [`GARLAND_FADE_SECS`] of the mode, then rising to 1 at its end, when the next mode takes over at 0).
pub fn garland_mode(t: f64) -> (u8, u8, f32) {
    let slot = (t / GARLAND_MODE_SECS).floor();
    let mode = (slot as i64).rem_euclid(i64::from(GARLAND_MODES)) as u8;
    let next = (mode + 1) % GARLAND_MODES;
    let into = t - slot * GARLAND_MODE_SECS;
    let x = ((into - (GARLAND_MODE_SECS - GARLAND_FADE_SECS)) / GARLAND_FADE_SECS).clamp(0.0, 1.0);
    (mode, next, (x * x * (3.0 - 2.0 * x)) as f32)
}

/// A pseudo-random number in [0, 1) from two numbers (the same sine hash as the windows' shader).
pub fn unit_hash(a: f32, b: f32) -> f32 {
    let s = (a * 12.9898 + b * 78.233).sin() * 43758.547;
    s - s.floor()
}

/// The step mode's colour (0 red, 1 green, 2 gold, 3 blue) at `t`: it jumps once a second, phased by `phase`.
pub fn step_colour(t: f64, phase: u32) -> u8 {
    ((t.floor() as i64 + i64::from(phase)).rem_euclid(4)) as u8
}

/// Whether flash group `parity` (0 or 1) is on at `t`: the two groups swap every half second.
pub fn flash_on(t: f64, parity: u32) -> bool {
    ((t / 0.5).floor() as i64 + i64::from(parity)).rem_euclid(2) == 0
}

/// The chase's brightness at `t` for a light at screen x `x`: a wave moving right, period 1.2 s, from
/// [`GARLAND_DIM`] to 1.
pub fn chase_wave(t: f64, x: f32) -> f32 {
    let s = (std::f64::consts::TAU * (t / 1.2 - f64::from(x) / CHASE_PX)).sin().max(0.0);
    GARLAND_DIM + (1.0 - GARLAND_DIM) * s as f32
}

/// Whether a light with `hash` is lit in the twinkle slot at `t` (slots of 0.25 s): about 70% are.
pub fn twinkle_on(t: f64, hash: f32) -> bool {
    unit_hash(hash, (t / 0.25).floor() as f32) >= 0.3
}

/// The glow mode's brightness at `t`: all lights breathe together, 2 s a breath, from 0.4 to 1.
pub fn glow_breath(t: f64) -> f32 {
    0.4 + 0.6 * (0.5 + 0.5 * (std::f64::consts::TAU * t / 2.0).sin()) as f32
}

/// A light's brightness under pattern `mode` at `t`: `parity` its flash group, `hash` its own random number
/// (0 to 1), `x` its screen x. Step is always full.
pub fn garland_level(mode: u8, t: f64, parity: u32, hash: f32, x: f32) -> f32 {
    match mode {
        1 if flash_on(t, parity) => 1.0,
        1 => GARLAND_DIM,
        2 => chase_wave(t, x),
        3 if twinkle_on(t, hash) => 1.0,
        3 => GARLAND_DIM,
        4 => glow_breath(t),
        _ => 1.0,
    }
}

/// A light's colour under pattern `mode` at `t`: the step mode's colour, else the light's own (`phase`, 0 to 3).
pub fn garland_colour(mode: u8, t: f64, phase: u32) -> u8 {
    if mode == 0 { step_colour(t, phase) } else { (phase % 4) as u8 }
}

/// What a light shows at `t` (its colour index and brightness), cross-fading from the controller's mode into the
/// next one over the fade. Lights take their phase, flash group, hash and screen x as in [`garland_level`].
pub fn garland_show(t: f64, phase: u32, parity: u32, hash: f32, x: f32) -> (u8, f32) {
    let (mode, next, fade) = garland_mode(t);
    let (a, b) = (garland_level(mode, t, parity, hash, x), garland_level(next, t, parity, hash, x));
    let colour = if fade < 0.5 { garland_colour(mode, t, phase) } else { garland_colour(next, t, phase) };
    (colour, a + (b - a) * fade)
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
    fn the_christmas_mod_is_the_one_the_game_plays_and_the_catalog_names() {
        assert!(is_christmas_mod(Some("christmas-2026"), Some("christmas-2026")));
        assert!(!is_christmas_mod(Some("evolution"), Some("christmas-2026")), "another mod plays");
        assert!(!is_christmas_mod(None, Some("christmas-2026")), "the base game plays");
        assert!(!is_christmas_mod(Some("christmas-2026"), None), "the catalog names no Christmas mod");
        assert!(!is_christmas_mod(None, None));
    }

    #[test]
    fn the_season_runs_from_1_december_to_15_january() {
        assert_eq!(season(2026, 12, 1), Some(2026));
        assert_eq!(season(2027, 1, 15), Some(2026));
        assert_eq!(season(2027, 1, 16), None);
        assert_eq!(season(2026, 11, 30), None);
    }

    /// The start is due on a built-in date (`due` of its season).
    fn due_on(asked: Option<i64>, date: (i64, u32, u32)) -> bool {
        due(asked, season(date.0, date.1, date.2))
    }

    #[test]
    fn the_season_start_is_due_once_per_season() {
        assert!(due_on(None, (2026, 12, 1)), "1 December, never done: due");
        assert!(due_on(None, (2027, 1, 15)), "15 January: still the season");
        assert!(!due_on(None, (2027, 1, 16)), "16 January: no season");
        assert!(!due_on(None, (2026, 11, 30)), "30 November: no season");
        assert!(!due_on(Some(2026), (2027, 1, 3)), "done for this season: not again");
        assert!(due_on(Some(2026), (2027, 12, 1)), "the next December: due again");
        assert!(!due_on(None, (2026, 7, 1)), "July: nothing");
    }

    #[test]
    fn a_remote_window_and_the_stop_switch_move_the_season() {
        let window = Some(HolidayWindow { from: (2026, 11, 23), to: (2027, 1, 15) });
        assert_eq!(season_in(window, true, 2026, 11, 23), Some(2026), "the first day");
        assert_eq!(season_in(window, true, 2027, 1, 15), Some(2026), "the last day, inclusive");
        assert_eq!(season_in(window, true, 2027, 1, 16), None, "the day after");
        assert_eq!(season_in(window, true, 2026, 11, 22), None, "the day before");
        assert_eq!(season_in(window, true, 2026, 12, 25), Some(2026), "Christmas inside");
        assert_eq!(season_in(window, false, 2026, 12, 25), None, "the stop switch: no season on 25 December");
        assert_eq!(season_in(None, false, 2026, 12, 25), None, "the stop switch with the built-in window");
        assert_eq!(season_in(None, true, 2026, 12, 25), Some(2026), "no window: the built-in season");
        assert!(due(Some(2025), season_in(window, true, 2026, 11, 23)), "the new window is due once");
    }

    #[test]
    fn a_date_override_reads_year_month_day() {
        assert_eq!(parse_date("2026-12-01"), Some((2026, 12, 1)));
        assert_eq!(parse_date(" 2027-01-15 "), Some((2027, 1, 15)));
        assert_eq!(parse_date("2026-13-01"), None, "no month 13");
        assert_eq!(parse_date("tomorrow"), None);
    }

    #[test]
    fn the_switch_on_steps_are_what_is_still_missing() {
        assert_eq!(on_steps(false, false), OnSteps { download: true, switch: true }, "a fresh install");
        assert_eq!(on_steps(true, true), OnSteps { download: false, switch: false }, "all in place");
        assert_eq!(on_steps(true, false), OnSteps { download: false, switch: true }, "installed, not played");
        assert_eq!(on_steps(false, true), OnSteps { download: true, switch: false }, "played but not on the disk");
    }

    #[test]
    fn the_look_needs_the_setting_and_the_christmas_mod_played() {
        assert!(look_active(true, true));
        assert!(!look_active(true, false), "the setting on, another game played: vanilla");
        assert!(!look_active(false, true), "the Christmas mod played, the setting off: vanilla");
        assert!(!look_active(false, false));
    }

    #[test]
    fn the_effects_follow_the_setting_and_the_game_played() {
        set_mod_running_for_tests(false);
        set_pref(true, false);
        assert!(!on(), "the setting on alone is not the look");
        assert!(!cheap());
        set_mod_running_for_tests(true);
        assert!(on(), "the setting on and the Christmas mod played");
        set_pref(false, false);
        assert!(!on(), "the setting off: vanilla even with the mod");
        set_mod_running_for_tests(false);
    }

    #[test]
    fn a_switch_on_or_off_of_the_look_or_a_change_of_game_moves_the_generation() {
        set_mod_running_for_tests(false);
        set_pref(false, false);
        let g0 = game_generation();
        set_pref(false, false);
        assert_eq!(game_generation(), g0, "no change, no reload");
        set_pref(true, false);
        assert_eq!(game_generation(), g0, "the setting alone does not move the look: the game must");
        set_mod_running_for_tests(true);
        assert_eq!(game_generation(), g0, "the test hook does not count (set_running_mod does)");
        set_pref(false, false);
        set_mod_running_for_tests(false);
    }

    #[test]
    fn the_switch_is_an_edge_of_the_setting() {
        assert_eq!(edge(false, true), Some(true), "turned on");
        assert_eq!(edge(true, false), Some(false), "turned off");
        assert_eq!(edge(true, true), None);
        assert_eq!(edge(false, false), None);
    }

    #[test]
    fn a_switch_to_the_christmas_mod_is_done_only_when_the_game_plays_it() {
        assert_eq!(awaited("holiday", Some("holiday"), false), Awaited::Done);
        assert_eq!(awaited("holiday", Some("holiday"), true), Awaited::Done, "played: done whatever runs");
        assert_eq!(awaited("holiday", Some("evolution"), true), Awaited::Waiting, "another switch runs: the switch waits");
        assert_eq!(awaited("holiday", None, false), Awaited::Missed, "nothing runs and it is not played: missed, still pending");
    }

    #[test]
    fn a_queued_mod_choice_runs_only_when_it_differs_from_the_game_played() {
        assert_eq!(queued_next(None, Some("evolution")), None, "nothing queued");
        assert_eq!(queued_next(Some(Some("holiday".into())), Some("evolution")), Some(Some("holiday".into())));
        assert_eq!(queued_next(Some(Some("holiday".into())), Some("holiday")), None, "already played");
        assert_eq!(queued_next(Some(None), Some("holiday")), Some(None), "the base game is a choice too");
        assert_eq!(queued_next(Some(None), None), None, "the base game is already played");
    }

    #[test]
    fn switching_off_goes_back_to_the_remembered_game_only_from_the_christmas_mod() {
        assert_eq!(off_target(true, Some("evolution".into())), Some(Some("evolution".into())), "back to the mod before");
        assert_eq!(off_target(true, None), Some(None), "the base game was played before");
        assert_eq!(off_target(false, Some("evolution".into())), None, "another game played: it stays");
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
    fn the_controller_switches_mode_every_8_seconds() {
        assert_eq!(garland_mode(0.0).0, 0);
        assert_eq!(garland_mode(7.99).0, 0, "the first mode runs 8 s");
        assert_eq!(garland_mode(8.0).0, 1, "then the next");
        assert_eq!(garland_mode(16.0).0, 2);
        assert_eq!(garland_mode(32.0).0, 4, "the fifth mode");
        assert_eq!(garland_mode(40.0).0, 0, "after the fifth, the first again");
        assert_eq!(garland_mode(-0.5).0, 4, "before time 0 it still cycles");
    }

    #[test]
    fn each_mode_fades_into_the_next_over_its_last_0_3_seconds() {
        assert_eq!(garland_mode(7.5), (0, 1, 0.0), "no fade before the last 0.3 s");
        let (m, n, f) = garland_mode(7.85);
        assert_eq!((m, n), (0, 1));
        assert!(f > 0.0 && f < 1.0, "fading: {f}");
        assert!(garland_mode(7.999).2 > 0.99, "almost done at the end");
        assert_eq!(garland_mode(8.0).2, 0.0, "the next mode starts with no fade");
        assert_eq!(garland_mode(8.0).1, 2, "and it fades into the one after");
    }

    #[test]
    fn step_jumps_to_the_next_colour_once_a_second() {
        for k in 0..10 {
            let t = f64::from(k);
            assert_eq!(step_colour(t + 0.1, 0), step_colour(t + 0.9, 0), "steady within the second {k}");
            assert_ne!(step_colour(t + 0.999, 0), step_colour(t + 1.0, 0), "a jump at {}", t + 1.0);
        }
        let cycle: Vec<u8> = (0..5).map(|k| step_colour(f64::from(k) + 0.5, 0)).collect();
        assert_eq!(cycle, vec![0, 1, 2, 3, 0], "red, green, gold, blue, red");
        assert_ne!(step_colour(0.5, 0), step_colour(0.5, 1), "neighbours are phased apart");
    }

    #[test]
    fn the_flash_groups_are_opposite_and_swap_every_half_second() {
        for k in 0..20 {
            let t = f64::from(k) * 0.13;
            assert_ne!(flash_on(t, 0), flash_on(t, 1), "the groups are opposite at {t}");
        }
        assert!(flash_on(0.1, 0) && !flash_on(0.1, 1));
        assert!(!flash_on(0.6, 0) && flash_on(0.6, 1), "swapped by 0.5 s");
        assert_eq!(flash_on(0.1, 0), flash_on(0.4, 0), "steady within a half second");
    }

    #[test]
    fn the_chase_wave_moves_right_with_a_1_2_second_period() {
        // The crest is at x = 600 * (t / 1.2 - 1/4): at t = 0 it is at x = -150, at t = 0.3 at x = 0.
        assert!(chase_wave(0.0, -150.0) > 0.999, "the crest at the start");
        assert!(chase_wave(0.3, 0.0) > 0.999, "the crest 0.3 s later, 150 px to the right");
        assert!(chase_wave(0.0, 0.0) < 0.9, "not there yet");
        for k in 0..20 {
            let (t, x) = (f64::from(k) * 0.23, k as f32 * 37.0);
            assert!((chase_wave(t, x) - chase_wave(t + 1.2, x)).abs() < 1e-5, "period 1.2 s at {t}");
            assert!((GARLAND_DIM..=1.0).contains(&chase_wave(t, x)));
        }
    }

    #[test]
    fn the_twinkle_is_fixed_within_a_quarter_second_and_about_70_percent_lit() {
        let lit = (0..2000).filter(|k| twinkle_on(f64::from(*k) * 0.25 + 0.1, 0.37)).count();
        assert!((1300..1500).contains(&lit), "about 70% lit: {lit}");
        assert_eq!(twinkle_on(3.01, 0.37), twinkle_on(3.20, 0.37), "one slot, one answer");
        assert_eq!(twinkle_on(5.5, 0.37), twinkle_on(5.5, 0.37), "deterministic");
        assert!((0..50).any(|k| twinkle_on(f64::from(k), 0.37) != twinkle_on(f64::from(k), 0.61)), "the lights differ");
    }

    #[test]
    fn the_glow_breathes_on_a_2_second_period() {
        assert!((glow_breath(0.3) - glow_breath(2.3)).abs() < 1e-5);
        assert!((0..200).map(|k| glow_breath(f64::from(k) * 0.01)).all(|v| (0.4..=1.0).contains(&v)));
    }

    #[test]
    fn a_light_shows_its_mode_and_fades_into_the_next() {
        // 3 s: step, no fade. The colour is the step colour, the brightness full.
        assert_eq!(garland_show(3.0, 2, 0, 0.5, 0.0), (step_colour(3.0, 2), 1.0));
        // 8.5 s: flash, no fade: the group's own on or off level.
        assert_eq!(garland_show(8.5, 0, 1, 0.5, 0.0).1, garland_level(1, 8.5, 1, 0.5, 0.0));
        // 7.95 s: step (full) fading into flash: between the two levels.
        let (_, level) = garland_show(7.95, 0, 0, 0.5, 0.0);
        let (a, b) = (garland_level(0, 7.95, 0, 0.5, 0.0), garland_level(1, 7.95, 0, 0.5, 0.0));
        assert!(level >= a.min(b) - 1e-6 && level <= a.max(b) + 1e-6, "{level} between {a} and {b}");
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
        set_mod_running_for_tests(true);
        set_pref(false, true);
        assert!(!cheap(), "the look off: no cheap path to take");
        set_pref(true, true);
        assert!(cheap());
        set_pref(true, false);
        assert!(!cheap());
        set_pref(false, false);
        set_mod_running_for_tests(false);
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
