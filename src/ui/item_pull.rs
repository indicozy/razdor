//! Pulling an item out of its cell (Razdor's, the owner's decision, 2026-10-31): a press that
//! picks an item up from a pack cell, a worn slot or a card's slot leaves it on the pointer as
//! before; when the button is then held while the pointer leaves the cell and the button goes
//! up, the release is a click where it lands: the item is put, swapped or given there as a click
//! there would. Released back on its own cell it is put back. A plain click (no leaving) is the
//! original's click-to-pick, which changes nothing here.
//!
//! [`picked`] starts a pull when an item is picked up; [`tick`] (once per frame, after the input's
//! frame) works out whether this frame drops it; [`dropped`] is the drop for the screens, which
//! treat it as a click at the pointer, as `widgets::clicked`.

use std::cell::Cell;

use macroquad::prelude::{MouseButton, Rect, Vec2};
use razdor::local;

use super::input::{is_mouse_button_down, is_mouse_button_released};
use super::widgets;

/// A pull in progress: the cell the item came from, and whether the pointer has left it while
/// the button was down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pull {
    pub origin: Rect,
    pub left: bool,
}

/// The next pull state and whether this frame drops the item. The pointer leaving `origin`
/// while the button is down makes it a pull; the release of a pull drops it (and ends it), a
/// release of a click (never left) does not. A frame with the button up ends the pull without a drop.
pub fn step(state: Option<Pull>, pointer: Vec2, down: bool, released: bool) -> (Option<Pull>, bool) {
    let Some(pull) = state else { return (None, false) };
    // A release in the same frame as a fast leave counts the pointer where it is released.
    let left = pull.left || !pull.origin.contains(pointer);
    if released {
        (None, left)
    } else if down {
        (Some(Pull { left, ..pull }), false)
    } else {
        (None, false)
    }
}

local! {
    static STATE: Cell<Option<Pull>> = const { Cell::new(None) };
    /// This frame's drop (`tick`).
    static DROP: Cell<bool> = const { Cell::new(false) };
}

/// An item was picked up from the cell or slot `origin` this frame (the press that took it).
pub fn picked(origin: Rect) {
    STATE.with(|s| s.set(Some(Pull { origin, left: false })));
}

/// Once per frame, after the input's frame (`App::frame`): whether the pull drops this frame.
pub fn tick() {
    let pointer = Vec2::from(widgets::pointer());
    let down = is_mouse_button_down(MouseButton::Left);
    let released = is_mouse_button_released(MouseButton::Left);
    let (next, drop) = STATE.with(|s| step(s.get(), pointer, down, released));
    STATE.with(|s| s.set(next));
    DROP.with(|d| d.set(drop));
}

/// The item is dropped this frame: a release of a pull (`tick`). Like `widgets::clicked`, none
/// while a screen blocks the input or a window swallows it.
pub fn dropped() -> bool {
    DROP.with(|d| d.get()) && !widgets::input_blocked() && !widgets::input_swallowed()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: Rect = Rect { x: 100.0, y: 100.0, w: 50.0, h: 50.0 };
    const IN: Vec2 = Vec2 { x: 120.0, y: 120.0 };
    const OUT: Vec2 = Vec2 { x: 300.0, y: 120.0 };

    /// Runs `frames` of (pointer, down, released) from a pick at the cell, returning the drops.
    fn run(frames: &[(Vec2, bool, bool)]) -> Vec<bool> {
        let mut state = Some(Pull { origin: CELL, left: false });
        frames
            .iter()
            .map(|&(p, down, released)| {
                let (next, drop) = step(state, p, down, released);
                state = next;
                drop
            })
            .collect()
    }

    #[test]
    fn a_click_without_leaving_gives_no_drop() {
        assert_eq!(run(&[(IN, true, false), (IN, true, false), (IN, false, true)]), vec![false, false, false]);
    }

    #[test]
    fn a_press_leaving_the_cell_and_released_elsewhere_drops() {
        assert_eq!(run(&[(IN, true, false), (OUT, true, false), (OUT, false, true)]), vec![false, false, true]);
    }

    #[test]
    fn a_release_in_the_frame_of_a_fast_leave_drops() {
        assert_eq!(run(&[(IN, true, false), (OUT, false, true)]), vec![false, true]);
    }

    #[test]
    fn leaving_and_coming_back_still_drops_where_released() {
        assert_eq!(run(&[(OUT, true, false), (IN, true, false), (IN, false, true)]), vec![false, false, true]);
        // Released back on its own cell after a leave: the drop puts it back there.
        assert_eq!(run(&[(IN, true, false), (OUT, true, false), (IN, false, true)]), vec![false, false, true]);
    }

    #[test]
    fn the_release_ends_the_pull() {
        let (state, drop) = step(Some(Pull { origin: CELL, left: true }), OUT, false, true);
        assert_eq!((state, drop), (None, true));
        assert_eq!(step(state, OUT, false, true), (None, false));
        assert_eq!(step(None, OUT, true, false), (None, false));
    }

    #[test]
    fn a_button_up_without_a_release_ends_the_pull_without_a_drop() {
        assert_eq!(step(Some(Pull { origin: CELL, left: true }), OUT, false, false), (None, false));
    }
}
