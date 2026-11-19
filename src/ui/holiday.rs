//! Razdor's «Новогодний вид» (`Settings::map_holiday`, off by default): a winter and holiday look for
//! the world map and the main menu. Style only: no rule, path, AI, fog, save or replay reads it.
//!
//! This module holds the setting's per-frame switch and the pure season logic (tested without a
//! window). Each effect reads [`on`] / [`cheap`] in the module that draws it. The main menu asks once
//! per season (from 1 December to 15 January), and the season of a January date is the year before.

// The look's readers come in Tasks 2-8; until they land, nothing here is called outside the tests.
#![allow(dead_code)]

use std::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

use razdor::local;
use razdor::rules::marks::civil;

use super::audio::Settings;

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
    fn the_cheap_path_needs_the_look_on() {
        set_pref(false, true);
        assert!(!cheap(), "the look off: no cheap path to take");
        set_pref(true, true);
        assert!(cheap());
        set_pref(true, false);
        assert!(!cheap());
        set_pref(false, false);
    }
}
