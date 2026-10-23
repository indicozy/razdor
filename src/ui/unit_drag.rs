//! Dragging a unit's card to another cell of the army grid (the army screen, the barracks):
//! pressed on a card, it follows the mouse once moved; let go over another cell, the unit
//! goes there, swapping with a unit standing in it (`Game::move_unit`). A press that does
//! not move stays a click. The garrison tab drags between its two grids the same way
//! ([`press_guard`], [`follow`]).
//!
//! Razdor's setting «Расстановка армии» ([`Controls`], the owner's decision after a player's
//! report, 2026-10-17): with dragging only (the default) a press on a card only selects its
//! unit and units move by dragging alone; «Как в оригинале» keeps the original's
//! press-to-pick, press-to-move (interface.md §9.6) with dragging on top.

use razdor::local;
use std::cell::Cell;

use crate::ui::input::{is_mouse_button_down};
use macroquad::prelude::*;
use razdor::rules::battle::Team;
use razdor::rules::content::UnitId;
use razdor::rules::formation::Slot;
use serde::{Deserialize, Serialize};

use super::assets::Assets;
use super::chrome;
use super::widgets::{input_blocked, pointer};

/// Pixels the mouse must move before a press on a card drags it.
const START: f32 = 5.0;

/// How the army grids outside battle are arranged (Razdor's setting in «Ещё…»). Battle is
/// not affected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Controls {
    /// Razdor's choice (the default, also for settings files without the key): a press on a
    /// card only selects its unit (its card and items show); units move and items are given
    /// by dragging. The original has no dragging and moves units by presses (0x4c346c).
    #[default]
    Drag,
    /// The original's: a press picks a unit, the next press swaps it or moves it (0x4c346c,
    /// 0x4c653c, 0x4c6f50), and Razdor's dragging works as well.
    Original,
}

impl Controls {
    /// The settings button's next choice.
    pub fn next(self) -> Controls {
        match self {
            Controls::Drag => Controls::Original,
            Controls::Original => Controls::Drag,
        }
    }
}

local! {
    static CONTROLS: Cell<Controls> = const { Cell::new(Controls::Drag) };
}

/// The setting, pushed in each frame (`main_menu::apply_options`).
pub fn set_controls(c: Controls) {
    CONTROLS.with(|m| m.set(c));
}

/// The arrangement chosen in the settings.
pub fn controls() -> Controls {
    CONTROLS.with(|m| m.get())
}

#[derive(Clone, Copy)]
struct Drag {
    /// Squad index (a garrison index when `guard`), and its portrait.
    unit: usize,
    guard: bool,
    kind: UnitId,
    /// Its hits and maximum: the dragged portrait shows its wounds as its card does.
    hp: i32,
    max: i32,
    start: Vec2,
    moved: bool,
}

local! {
    static DRAG: Cell<Option<Drag>> = const { Cell::new(None) };
}

/// A press on squad member `unit`'s card (its type `kind`, its hits `hp` of `max`): it may be
/// dragged from here.
pub fn press(unit: usize, kind: UnitId, (hp, max): (i32, i32)) {
    DRAG.with(|d| d.set(Some(Drag { unit, guard: false, kind, hp, max, start: pointer().into(), moved: false })));
}

/// A press on garrison unit `unit`'s card (the garrison tab): it may be dragged from here.
pub fn press_guard(unit: usize, kind: UnitId, (hp, max): (i32, i32)) {
    DRAG.with(|d| d.set(Some(Drag { unit, guard: true, kind, hp, max, start: pointer().into(), moved: false })));
}

/// The squad member being dragged, once the mouse has moved (its card is drawn dimmed).
pub fn dragged() -> Option<usize> {
    dragged_from().filter(|d| !d.0).map(|d| d.1)
}

/// The card being dragged, once the mouse has moved: (a garrison unit, its index).
pub fn dragged_from() -> Option<(bool, usize)> {
    DRAG.with(|d| d.get()).filter(|d| d.moved).map(|d| (d.guard, d.unit))
}

/// After the grid is drawn: follows the mouse with the portrait (card `size`), and on the
/// release over one of `cells` other than the unit's own returns (unit, cell). A garrison
/// drag ends here without a move.
pub fn update(assets: &Assets, cells: &[(Slot, Rect)], size: Vec2) -> Option<(usize, Slot)> {
    let (guard, unit) = follow(assets, size)?;
    if guard {
        return None;
    }
    let m = Vec2::from(pointer());
    cells.iter().find(|(_, r)| r.contains(m)).map(|&(slot, _)| (unit, slot))
}

/// Follows the mouse with the dragged portrait (card `size`); on a release after the mouse
/// moved returns where the drag started: (a garrison unit, its index). The caller decides
/// what lies under the pointer.
pub fn follow(assets: &Assets, size: Vec2) -> Option<(bool, usize)> {
    let mut d = DRAG.with(|d| d.get())?;
    let m = Vec2::from(pointer());
    if !d.moved && m.distance(d.start) > START {
        d.moved = true;
    }
    if is_mouse_button_down(MouseButton::Left) && !input_blocked() {
        if d.moved {
            let r = Rect::new(m.x - size.x / 2.0, m.y - size.x / 2.0, size.x, size.x);
            assets.draw_portrait(d.kind, Team::Player, r);
            // A wounded unit keeps its wounds on the portrait, as in its cell (chrome::wounds).
            chrome::wounds(r, d.hp, d.max);
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, super::chrome::GOLD);
        }
        DRAG.with(|c| c.set(Some(d)));
        return None;
    }
    DRAG.with(|c| c.set(None));
    d.moved.then_some((d.guard, d.unit))
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
    /// An empty cell with nothing selected (with dragging only, any empty cell: the
    /// selection is cleared).
    Nothing,
}

/// The press on `pressed` (a squad index, `None` for an empty cell) with `selected`
/// selected, as the original's. The hero is a unit like any other here: selected, a press
/// on another unit swaps the two (no test on the hero or a named character in 0x4c346c).
pub fn grid_press(selected: Option<usize>, pressed: Option<usize>) -> GridPress {
    match (selected, pressed) {
        (Some(s), Some(p)) if s == p => GridPress::Deselect,
        (None, Some(p)) => GridPress::Select(p),
        (Some(s), Some(p)) => GridPress::Swap { selected: s, pressed: p },
        (Some(s), None) => GridPress::Slide(s),
        (None, None) => GridPress::Nothing,
    }
}

/// The press as the setting `controls` has it. With [`Controls::Drag`] (Razdor's choice, the
/// owner's decision) a press never moves a unit: a unit pressed is selected, the selected one
/// is deselected again (the original's deselection, 0x4c346c, 0x4c653c, 0x4c6f50: the
/// building window has no other way back to the town's menu with a full formation), and an
/// empty cell clears the selection, where the original swaps or slides on the second press.
pub fn grid_press_in(controls: Controls, selected: Option<usize>, pressed: Option<usize>) -> GridPress {
    match (controls, pressed) {
        (Controls::Original, _) => grid_press(selected, pressed),
        (Controls::Drag, Some(p)) if selected == Some(p) => GridPress::Deselect,
        (Controls::Drag, Some(p)) => GridPress::Select(p),
        (Controls::Drag, None) => GridPress::Nothing,
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
    fn as_in_the_original_the_second_press_moves() {
        use Controls::Original;
        assert_eq!(grid_press_in(Original, Some(3), Some(5)), GridPress::Swap { selected: 3, pressed: 5 });
        assert_eq!(grid_press_in(Original, Some(4), None), GridPress::Slide(4));
        assert_eq!(grid_press_in(Original, Some(3), Some(3)), GridPress::Deselect);
        assert_eq!(grid_press_in(Original, None, Some(3)), GridPress::Select(3));
        assert_eq!(grid_press_in(Original, None, None), GridPress::Nothing);
    }

    #[test]
    fn with_dragging_only_a_press_never_moves_a_unit() {
        use Controls::Drag;
        // A unit pressed is selected, whatever was selected before: no swap.
        assert_eq!(grid_press_in(Drag, None, Some(3)), GridPress::Select(3));
        assert_eq!(grid_press_in(Drag, Some(3), Some(5)), GridPress::Select(5));
        assert_eq!(grid_press_in(Drag, Some(0), Some(5)), GridPress::Select(5));
        // The selected unit pressed again is deselected (0x4c346c, 0x4c6f50), the hero included.
        assert_eq!(grid_press_in(Drag, Some(3), Some(3)), GridPress::Deselect);
        assert_eq!(grid_press_in(Drag, Some(0), Some(0)), GridPress::Deselect);
        // An empty cell: no slide, nothing selected.
        assert_eq!(grid_press_in(Drag, Some(4), None), GridPress::Nothing);
        assert_eq!(grid_press_in(Drag, None, None), GridPress::Nothing);
    }

    #[test]
    fn dragging_is_the_default_also_for_old_settings_files() {
        assert_eq!(Controls::default(), Controls::Drag);
        assert_eq!(Controls::Drag.next(), Controls::Original);
        assert_eq!(Controls::Original.next(), Controls::Drag);
        assert_eq!(serde_json::to_string(&Controls::Original).unwrap(), "\"original\"");
    }

    #[test]
    fn slides_take_0_7_ms_a_pixel_up_to_200() {
        assert_eq!(slide_ms(vec2(0.0, 0.0), vec2(96.0, 0.0), 1.0), 67);
        assert_eq!(slide_ms(vec2(0.0, 0.0), vec2(96.0, 0.0), 2.0), 34);
        assert_eq!(slide_ms(vec2(0.0, 0.0), vec2(300.0, 400.0), 1.0), 200);
    }
}
