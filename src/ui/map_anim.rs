//! Razdor's «Анимация карты» (`Settings::map_anim`, the owner's decision 2026-11-11: settings instead of a map
//! cache for the zoomed-out cost). Style only: no rule, path, fog or save reads it.
//!
//! - `On` (the default): everything on the map moves, as before. Byte-identical to the map without the setting.
//! - `Near`: the animations fade out when the map is zoomed out (`world_view::zoom_lod`: the level 1 at 16 screen
//!   px a cell, 0 at 8 and under). The sway of the trees is scaled by the level; the water, the living map and the
//!   other clock-driven parts stop at level 0.
//! - `Off`: the map is still. The animation time stops, so the sprites, the water, the mist and the flicker hold
//!   one frame; the water, the living map and the wakes are off, the fog's reveal is instant, and the idle frames
//!   are paced by the battery saver's rate (`frame_cap::battery_cap`).
//!
//! The map's animation time (`now`) is the frame clock minus the time the animations were not running (`LOST`),
//! which only grows while the level is under 1. With `On` it is the frame clock itself, to the bit.
//!
//! Env override (a profiling and snapshot switch): `RAZDOR_MAP_ANIM=on|near|off`.

use serde::{Deserialize, Serialize};

use razdor::local;

/// The setting «Анимация карты».
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// Everything on the map moves (today's behaviour).
    #[default]
    On,
    /// The animations fade out when the map is zoomed out.
    Near,
    /// The map is still.
    Off,
}

impl Mode {
    /// The next setting in the row: On, Near only, Off, On.
    pub fn next(self) -> Mode {
        match self {
            Mode::On => Mode::Near,
            Mode::Near => Mode::Off,
            Mode::Off => Mode::On,
        }
    }

    /// The setting from its env name (`RAZDOR_MAP_ANIM`); anything else is `None`.
    pub fn parse(s: &str) -> Option<Mode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "on" => Some(Mode::On),
            "near" => Some(Mode::Near),
            "off" => Some(Mode::Off),
            _ => None,
        }
    }
}

/// The setting as the settings window last pushed it (`display::follow_settings`): 0 On, 1 Near, 2 Off.
static PREF: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

local! {
    /// `RAZDOR_MAP_ANIM` overrides the setting (read once).
    static ENV: Option<Mode> = std::env::var("RAZDOR_MAP_ANIM").ok().and_then(|v| Mode::parse(&v));
    /// The level the map frame set (`begin`): 1 while everything moves, 0 while nothing does.
    static LEVEL: std::cell::Cell<f32> = std::cell::Cell::new(1.0);
    /// The frame clock of the last map frame, and the time the animations were not running (s).
    static LAST: std::cell::Cell<Option<f64>> = std::cell::Cell::new(None);
    static LOST: std::cell::Cell<f64> = std::cell::Cell::new(0.0);
}

/// Takes the setting in (every frame, from `display::follow_settings`).
pub fn set_pref(mode: Mode) {
    let v = match mode {
        Mode::On => 0,
        Mode::Near => 1,
        Mode::Off => 2,
    };
    PREF.store(v, std::sync::atomic::Ordering::Relaxed);
}

/// The setting in effect: the env override, else the settings window's.
pub fn mode() -> Mode {
    ENV.with(|e| *e).unwrap_or(match PREF.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Mode::Near,
        2 => Mode::Off,
        _ => Mode::On,
    })
}

/// Whether the map is still («Выкл»): the fog's reveal is instant and the battery saver's idle rate applies.
pub fn off() -> bool {
    mode() == Mode::Off
}

/// The level of the animations at a camera scale (screen px a cell) under `mode`: 1 while they all run, 0 while
/// none does, `zoom_lod` between under `Near`.
pub fn level_at(mode: Mode, scale: f32) -> f32 {
    match mode {
        Mode::On => 1.0,
        Mode::Near => super::world_view::zoom_lod(scale),
        Mode::Off => 0.0,
    }
}

/// The level the last map frame set (`begin`): 1 with `On`, 0 with `Off`, `zoom_lod` with `Near`.
pub fn level() -> f32 {
    LEVEL.with(|l| l.get())
}

/// Starts a map frame at camera scale `scale`: sets the level and moves the animation time on. Time the level was
/// under 1 does not count, so the animations resume where they stopped. Called once a map frame, before its drawing.
pub fn begin(scale: f32) {
    advance(super::clock::now(), level_at(mode(), scale));
}

/// The frame at clock `now` with `level`: the time since the last frame, at the last frame's level, is lost.
fn advance(now: f64, level: f32) {
    let prev_level = LEVEL.with(|l| l.get());
    if let Some(last) = LAST.with(|l| l.get()) {
        let dt = (now - last).max(0.0);
        LOST.with(|l| l.set(l.get() + dt * f64::from(1.0 - prev_level)));
    }
    LAST.with(|l| l.set(Some(now)));
    LEVEL.with(|l| l.set(level));
}

/// The map's animation time (s): the frame clock less the time the animations were not running. Equal to the
/// frame clock while the level is always 1.
pub fn now() -> f64 {
    super::clock::now() - LOST.with(|l| l.get())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_on_and_the_row_cycles() {
        assert_eq!(Mode::default(), Mode::On);
        assert_eq!(Mode::On.next(), Mode::Near);
        assert_eq!(Mode::Near.next(), Mode::Off);
        assert_eq!(Mode::Off.next(), Mode::On);
    }

    #[test]
    fn the_env_names_parse_and_nothing_else_does() {
        assert_eq!(Mode::parse("on"), Some(Mode::On));
        assert_eq!(Mode::parse(" Near "), Some(Mode::Near));
        assert_eq!(Mode::parse("OFF"), Some(Mode::Off));
        assert_eq!(Mode::parse("0"), None);
        assert_eq!(Mode::parse(""), None);
    }

    #[test]
    fn the_setting_sets_the_level_per_zoom() {
        // On: everything moves at every zoom; Off: nothing does at any zoom.
        for scale in [1.0, 8.0, 16.0, 40.0] {
            assert_eq!(level_at(Mode::On, scale), 1.0, "on at {scale} px a cell");
            assert_eq!(level_at(Mode::Off, scale), 0.0, "off at {scale} px a cell");
        }
        // Near: the level of the zoom (`zoom_lod`): 0 under 8 px a cell, 1 from 16, linear between.
        assert_eq!(level_at(Mode::Near, 4.0), 0.0);
        assert_eq!(level_at(Mode::Near, 8.0), 0.0);
        assert!((level_at(Mode::Near, 12.0) - 0.5).abs() < 1e-6);
        assert_eq!(level_at(Mode::Near, 16.0), 1.0);
        assert_eq!(level_at(Mode::Near, 30.0), 1.0);
    }

    #[test]
    fn the_setting_survives_serialization_and_an_older_file_moves() {
        let back: Mode = serde_json::from_str(&serde_json::to_string(&Mode::Near).unwrap()).unwrap();
        assert_eq!(back, Mode::Near);
        assert_eq!(Mode::default(), Mode::On, "an older file: the map moves as before");
    }

    #[test]
    fn the_time_stops_while_the_level_is_zero_and_resumes_after() {
        LAST.with(|l| l.set(None));
        LOST.with(|l| l.set(0.0));
        advance(10.0, 1.0);
        advance(11.0, 1.0);
        assert_eq!(LOST.with(|l| l.get()), 0.0, "at level 1 nothing is lost");
        advance(12.0, 0.0);
        assert_eq!(LOST.with(|l| l.get()), 0.0, "the second before this frame ran at level 1");
        advance(13.5, 0.0);
        assert_eq!(LOST.with(|l| l.get()), 1.5, "the 1.5 s since the frame at 12 ran at level 0: lost");
        advance(14.0, 1.0);
        assert_eq!(LOST.with(|l| l.get()), 2.0, "the 0.5 s since 13.5 ran at level 0 too: lost");
        advance(14.0, 1.0);
        assert_eq!(LOST.with(|l| l.get()), 2.0, "no time passed: no loss");
    }

    #[test]
    fn a_partial_level_loses_the_share_it_does_not_run() {
        LAST.with(|l| l.set(None));
        LOST.with(|l| l.set(0.0));
        advance(0.0, 0.5);
        advance(2.0, 0.5);
        assert!((LOST.with(|l| l.get()) - 1.0).abs() < 1e-12, "half of 2 s is lost");
    }
}
