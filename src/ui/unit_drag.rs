//! Dragging a unit's card to another cell of the army grid (the army screen, the barracks):
//! pressed on a card, it follows the mouse once moved; let go over another cell, the unit
//! goes there, swapping with a unit standing in it (`Game::move_unit`). A press that does
//! not move stays a click.

use std::cell::Cell;

use crate::ui::input::{is_mouse_button_down};
use macroquad::prelude::*;
use razdor::rules::battle::Team;
use razdor::rules::content::UnitId;
use razdor::rules::formation::Slot;

use super::assets::Assets;
use super::widgets::{input_blocked, pointer};

/// Pixels the mouse must move before a press on a card drags it.
const START: f32 = 5.0;

#[derive(Clone, Copy)]
struct Drag {
    /// Squad index, and its portrait.
    unit: usize,
    kind: UnitId,
    start: Vec2,
    moved: bool,
}

thread_local! {
    static DRAG: Cell<Option<Drag>> = const { Cell::new(None) };
}

/// A press on squad member `unit`'s card: it may be dragged from here.
pub fn press(unit: usize, kind: UnitId) {
    DRAG.with(|d| d.set(Some(Drag { unit, kind, start: pointer().into(), moved: false })));
}

/// The unit being dragged, once the mouse has moved (its card is drawn dimmed).
pub fn dragged() -> Option<usize> {
    DRAG.with(|d| d.get()).filter(|d| d.moved).map(|d| d.unit)
}

/// After the grid is drawn: follows the mouse with the portrait (card `size`), and on the
/// release over one of `cells` other than the unit's own returns (unit, cell).
pub fn update(assets: &Assets, cells: &[(Slot, Rect)], size: Vec2) -> Option<(usize, Slot)> {
    let mut d = DRAG.with(|d| d.get())?;
    let m = Vec2::from(pointer());
    if !d.moved && m.distance(d.start) > START {
        d.moved = true;
    }
    if is_mouse_button_down(MouseButton::Left) && !input_blocked() {
        if d.moved {
            let r = Rect::new(m.x - size.x / 2.0, m.y - size.x / 2.0, size.x, size.x);
            assets.draw_portrait(d.kind, Team::Player, r);
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, super::chrome::GOLD);
        }
        DRAG.with(|c| c.set(Some(d)));
        return None;
    }
    DRAG.with(|c| c.set(None));
    if !d.moved {
        return None;
    }
    cells.iter().find(|(_, r)| r.contains(m)).map(|&(slot, _)| (d.unit, slot))
}

/// Leaving the screen: nothing stays held.
pub fn cancel() {
    DRAG.with(|d| d.set(None));
}

/// What a press on a grid of one army does, as the original's army window (0x4c346c) and
/// the building window's hero grid within the hero's army (0x4c653c). The original acts on
/// the press and knows no dragging; Razdor's drag starts from the same press unless it
/// swaps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridPress {
    /// The selected unit was pressed: nothing is selected now.
    Deselect,
    /// A unit pressed with nothing selected: it is selected.
    Select(usize),
    /// Another unit pressed while one is selected: the two swap cells at once (`Card-Move`,
    /// no slide) and nothing is selected.
    Swap { selected: usize, pressed: usize },
    /// An empty cell pressed while a unit is selected: its card slides there (0x4b0c04,
    /// `Card-Move`); the selection is cleared when the slide ends.
    Slide(usize),
    /// An empty cell with nothing selected.
    Nothing,
}

/// The press on `pressed` (a squad index, `None` for an empty cell) with `selected`
/// selected. The hero is a unit like any other here: selected, a press on another unit
/// swaps the two (no test on the hero or a named character in 0x4c346c).
pub fn grid_press(selected: Option<usize>, pressed: Option<usize>) -> GridPress {
    match (selected, pressed) {
        (Some(s), Some(p)) if s == p => GridPress::Deselect,
        (None, Some(p)) => GridPress::Select(p),
        (Some(s), Some(p)) => GridPress::Swap { selected: s, pressed: p },
        (Some(s), None) => GridPress::Slide(s),
        (None, None) => GridPress::Nothing,
    }
}

/// How long a card slides from `from` to `to` (screen px at interface scale `k`), in ms:
/// 0.7 ms per original pixel, rounded, at most 200 (0x4b0c04, as the battle slide 4b0284).
pub fn slide_ms(from: Vec2, to: Vec2, k: f32) -> i64 {
    ((from.distance(to) / k.max(f32::EPSILON) * 0.7).round() as i64).min(200)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_follow_the_army_window() {
        // 0x4c346c: pressed = selected → deselect.
        assert_eq!(grid_press(Some(3), Some(3)), GridPress::Deselect);
        assert_eq!(grid_press(Some(0), Some(0)), GridPress::Deselect);
        // A unit with nothing selected is selected, the hero too.
        assert_eq!(grid_press(None, Some(3)), GridPress::Select(3));
        assert_eq!(grid_press(None, Some(0)), GridPress::Select(0));
        // Another unit swaps, the hero selected included.
        assert_eq!(grid_press(Some(3), Some(5)), GridPress::Swap { selected: 3, pressed: 5 });
        assert_eq!(grid_press(Some(0), Some(5)), GridPress::Swap { selected: 0, pressed: 5 });
        assert_eq!(grid_press(Some(5), Some(0)), GridPress::Swap { selected: 5, pressed: 0 });
        // An empty cell slides the selected unit there, or does nothing.
        assert_eq!(grid_press(Some(4), None), GridPress::Slide(4));
        assert_eq!(grid_press(None, None), GridPress::Nothing);
    }

    #[test]
    fn slides_take_0_7_ms_a_pixel_up_to_200() {
        assert_eq!(slide_ms(vec2(0.0, 0.0), vec2(96.0, 0.0), 1.0), 67);
        assert_eq!(slide_ms(vec2(0.0, 0.0), vec2(96.0, 0.0), 2.0), 34);
        assert_eq!(slide_ms(vec2(0.0, 0.0), vec2(300.0, 400.0), 1.0), 200);
    }
}
