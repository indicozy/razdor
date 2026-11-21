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

/// The garlands' points on a building's light boxes (`garland_points`): sprite pixels and the colour index (0 red,
/// 1 green, 2 gold, 3 blue), one point every 6 px along the top edge of each window, door and gate box.
pub type Garland = (f32, f32, u8);

/// Spacing of the garland points along a box's top edge, in sprite pixels.
pub const GARLAND_STEP: i32 = 6;

/// The garland points of a building's light boxes: along the top edge `y0` of each window, door and gate box
/// (`[x0, y0, x1, y1]`), one point every [`GARLAND_STEP`] px from `x0` to `x1` (both ends included). Colours
/// cycle 0 to 3 across all the points in order. Chimneys, flags, sails and the other kinds give none.
pub fn garland_points(boxes: &[(Kind, [i32; 4])]) -> Vec<Garland> {
    let mut out = Vec::new();
    for (kind, [x0, y0, x1, _]) in boxes {
        if !matches!(kind, Kind::Window | Kind::Door | Kind::Gate) {
            continue;
        }
        let mut x = *x0;
        while x <= *x1 {
            out.push((x as f32, *y0 as f32, (out.len() % 4) as u8));
            x += GARLAND_STEP;
        }
    }
    out
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
    fn a_window_top_edge_gets_a_point_every_6_px_with_cycling_colours() {
        let pts = garland_points(&[(Kind::Window, [10, 20, 34, 30])]);
        assert!((4..=5).contains(&pts.len()), "a 24-px box: {pts:?}");
        assert_eq!(pts[0], (10.0, 20.0, 0), "from the left end, on the top edge");
        assert_eq!(pts[1], (16.0, 20.0, 1));
        assert_eq!(pts[2], (22.0, 20.0, 2));
        assert_eq!(pts[3], (28.0, 20.0, 3));
        assert_eq!(pts[4].2, 0, "the colours cycle");
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
    fn the_colour_index_runs_on_across_boxes() {
        let pts = garland_points(&[(Kind::Window, [0, 0, 0, 4]), (Kind::Window, [0, 9, 0, 13])]);
        let colours: Vec<u8> = pts.iter().map(|p| p.2).collect();
        assert_eq!(colours, vec![0, 1], "one point per single-px box, colours still cycle");
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
