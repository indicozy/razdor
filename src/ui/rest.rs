//! The pointer resting still (Razdor's, for the touchpad mode): where the pointer came to rest
//! and when, updated once per frame from `App::frame`. A touchpad has no button held under a
//! finger, so the cursor resting over an item, an army or a building shows what the original
//! shows only while the right button is held (interface.md §7.4), once it has rested
//! [`REST_SECS`].

use std::cell::Cell;

use macroquad::prelude::{MouseButton, Vec2};
use razdor::local;

use super::clock;
use crate::ui::input::is_mouse_button_down;

/// How long the pointer must rest before its tooltip shows, in seconds: the same 350 ms as the
/// gestures' long press (the owner's one delay for holding still).
pub const REST_SECS: f32 = 0.35;

/// Pixels the pointer may wander and still count as resting.
const SLOP: f32 = 2.0;

/// Where the pointer came to rest, and the clock when it did.
pub type Rest = Option<(Vec2, f64)>;

local! {
    static STATE: Cell<Rest> = const { Cell::new(None) };
}

/// The next rest state: a move of more than [`SLOP`] pixels restarts the rest at `now`, a
/// button down clears it (no rest while a button is held), and no state yet starts one.
pub fn step(rest: Rest, pointer: Vec2, now: f64, buttons_down: bool) -> Rest {
    if buttons_down {
        return None;
    }
    match rest {
        Some((at, since)) if (pointer - at).length() <= SLOP => Some((at, since)),
        _ => Some((pointer, now)),
    }
}

/// Whether the rest has lasted [`REST_SECS`] at `now`.
pub fn rested_for(rest: Rest, now: f64) -> bool {
    rest.is_some_and(|(_, since)| now - since >= REST_SECS as f64)
}

/// Updates the rest state for this frame (called once per frame, after the input's frame).
pub fn tick() {
    let pointer = Vec2::from(super::widgets::pointer());
    let buttons = is_mouse_button_down(MouseButton::Left) || is_mouse_button_down(MouseButton::Right);
    STATE.with(|s| s.set(step(s.get(), pointer, clock::now(), buttons)));
}

/// The pointer has rested still for [`REST_SECS`] with no button down, as of the last frame.
pub fn rested() -> bool {
    STATE.with(|s| rested_for(s.get(), clock::now()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use macroquad::prelude::vec2;

    #[test]
    fn a_rest_under_the_delay_does_not_count() {
        let rest = step(None, vec2(10.0, 10.0), 1.0, false);
        assert!(!rested_for(rest, 1.0 + REST_SECS as f64 - 0.01));
    }

    #[test]
    fn a_rest_of_the_delay_counts() {
        let rest = step(None, vec2(10.0, 10.0), 1.0, false);
        assert!(rested_for(rest, 1.0 + REST_SECS as f64));
        assert!(rested_for(step(rest, vec2(10.5, 10.0), 2.0, false), 2.0 + REST_SECS as f64));
    }

    #[test]
    fn a_move_restarts_the_rest() {
        let rest = step(None, vec2(10.0, 10.0), 1.0, false);
        let moved = step(rest, vec2(50.0, 10.0), 2.0, false);
        assert!(!rested_for(moved, 2.0 + REST_SECS as f64 - 0.01));
        assert!(rested_for(moved, 2.0 + REST_SECS as f64));
    }

    #[test]
    fn a_button_down_is_no_rest() {
        let rest = step(None, vec2(10.0, 10.0), 1.0, false);
        let held = step(rest, vec2(10.0, 10.0), 2.0, true);
        assert_eq!(held, None);
        assert!(!rested_for(held, 5.0));
        // Released, the rest starts again from the release.
        let released = step(held, vec2(10.0, 10.0), 3.0, false);
        assert!(!rested_for(released, 3.1));
    }

    #[test]
    fn no_rest_without_a_state() {
        assert!(!rested_for(None, 100.0));
    }
}
