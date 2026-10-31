//! Where a pop-up that follows the pointer goes on a phone (Razdor's, backlog 1449): above the
//! finger, clear of it by a finger's width, or beside it when there is no room above. On the
//! desktop nothing changes: each pop-up keeps its own placement (`placed` hands it back).

use macroquad::prelude::{screen_height, screen_width, Rect};

use super::display;
use super::widgets::FINGER_DP;

/// The gap kept from the screen's edges, in screen pixels.
const MARGIN: f32 = 4.0;

/// The phone's clearance for a finger, in screen pixels: a finger's width (`FINGER_DP`, 48 dp).
/// `None` on the desktop, where the pointer is a mouse's and the placements stay as they were.
pub fn finger_gap() -> Option<f32> {
    display::dp_px().map(|dp| FINGER_DP * dp)
}

/// The rectangle a phone's pop-ups may use: the screen above the bottom bar (a pop-up never
/// covers the bar's buttons).
pub fn usable() -> Rect {
    Rect::new(0.0, 0.0, screen_width(), super::pan::bar_top().min(screen_height()))
}

/// The pop-up for a pointer at `at` on a phone: `desktop` as it is on the desktop, else
/// [`place_popup`] of `size` in [`usable`]. The pop-up's own size is `size`, whatever its
/// desktop rectangle is.
pub fn placed(desktop: Rect, at: (f32, f32), size: (f32, f32)) -> Rect {
    match finger_gap() {
        Some(gap) => place_popup(at, size, usable(), gap),
        None => desktop,
    }
}

/// Places a pop-up of `size` (w, h) for a finger at `touch` inside `screen`, `gap` clear of the
/// finger: above it, centred on it, when that fits; else beside it, to the right, then to the
/// left, whichever fits first (vertically centred, kept on the screen); else on the side with
/// more room. Pure: no screen or state is read.
///
/// The result is always on `screen` (unless the pop-up is bigger than the screen), and off the
/// finger: above, its bottom is `gap` over the touch; beside, its near side is `gap` from it.
pub fn place_popup(touch: (f32, f32), size: (f32, f32), screen: Rect, gap: f32) -> Rect {
    let (tx, ty) = touch;
    let (w, h) = size;
    let fit_x = |x: f32| x.clamp(screen.x + MARGIN, (screen.right() - MARGIN - w).max(screen.x + MARGIN));
    let fit_y = |y: f32| y.clamp(screen.y + MARGIN, (screen.bottom() - MARGIN - h).max(screen.y + MARGIN));

    let above = ty - gap - h;
    if above >= screen.y + MARGIN {
        return Rect::new(fit_x(tx - w / 2.0), above, w, h);
    }

    let right = tx + gap;
    let left = tx - gap - w;
    let fits_right = right + w <= screen.right() - MARGIN;
    let fits_left = left >= screen.x + MARGIN;
    let x = if fits_right {
        right
    } else if fits_left {
        left
    } else if screen.right() - tx >= tx - screen.x {
        right
    } else {
        left
    };
    Rect::new(fit_x(x), fit_y(ty - h / 2.0), w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use macroquad::prelude::Rect;

    /// A phone's screen in pixels: 400 × 800, its finger gap 48.
    fn phone() -> Rect {
        Rect::new(0.0, 0.0, 400.0, 800.0)
    }

    /// The finger's square: its half gap around the touch.
    fn finger(touch: (f32, f32), gap: f32) -> Rect {
        Rect::new(touch.0 - gap / 2.0, touch.1 - gap / 2.0, gap, gap)
    }

    fn on_screen(r: Rect, s: Rect) -> bool {
        r.x >= s.x && r.y >= s.y && r.right() <= s.right() && r.bottom() <= s.bottom()
    }

    #[test]
    fn with_room_above_the_pop_up_goes_above_the_finger_a_finger_width_clear() {
        let touch = (200.0, 500.0);
        let r = place_popup(touch, (100.0, 50.0), phone(), 48.0);
        assert_eq!(r.bottom(), 500.0 - 48.0, "the bottom is a finger's width over the touch");
        assert!(r.x >= 0.0 && r.right() <= 400.0);
        assert!(!r.overlaps(&finger(touch, 48.0)));
        assert!(on_screen(r, phone()));
    }

    #[test]
    fn with_no_room_above_the_pop_up_goes_beside_the_finger_on_the_right() {
        let touch = (200.0, 40.0);
        let r = place_popup(touch, (100.0, 50.0), phone(), 48.0);
        assert_eq!(r.x, 200.0 + 48.0, "its left side is a finger's width right of the touch");
        assert!(!r.overlaps(&finger(touch, 48.0)));
        assert!(on_screen(r, phone()));
    }

    #[test]
    fn beside_the_finger_when_the_right_has_no_room_it_goes_left() {
        let touch = (380.0, 40.0);
        let r = place_popup(touch, (100.0, 50.0), phone(), 48.0);
        assert_eq!(r.right(), 380.0 - 48.0, "its right side is a finger's width left of the touch");
        assert_eq!(r.y, 40.0 - 25.0, "centred on the touch's height");
        assert!(!r.overlaps(&finger(touch, 48.0)));
        assert!(on_screen(r, phone()));
    }

    #[test]
    fn beside_the_finger_is_kept_on_the_screen_and_a_low_touch_goes_above() {
        // No room above, the box taller than the touch's room: it stays on the screen's top.
        let r = place_popup((200.0, 10.0), (100.0, 300.0), phone(), 48.0);
        assert_eq!(r.y, 4.0);
        assert!(on_screen(r, phone()));
        // Low on the screen: still above it, clear of the finger, on the screen.
        let touch = (200.0, 760.0);
        let r = place_popup(touch, (100.0, 50.0), phone(), 48.0);
        assert!(on_screen(r, phone()));
        assert!(!r.overlaps(&finger(touch, 48.0)));
    }

    #[test]
    fn near_the_left_edge_the_pop_up_stays_on_the_screen_and_off_the_finger() {
        let touch = (5.0, 600.0);
        let r = place_popup(touch, (100.0, 50.0), phone(), 48.0);
        assert_eq!(r.x, 4.0, "clamped to the screen's margin");
        assert!(on_screen(r, phone()));
        assert!(!r.overlaps(&finger(touch, 48.0)));
    }

    #[test]
    fn near_the_right_and_bottom_edges_the_pop_up_stays_on_the_screen_and_off_the_finger() {
        let touch = (395.0, 790.0);
        let r = place_popup(touch, (100.0, 50.0), phone(), 48.0);
        assert!(on_screen(r, phone()));
        assert!(r.right() <= 396.0);
        assert!(!r.overlaps(&finger(touch, 48.0)));
    }

    #[test]
    fn a_touch_in_the_top_left_corner_goes_beside_and_stays_on_the_screen() {
        let touch = (6.0, 6.0);
        let r = place_popup(touch, (100.0, 50.0), phone(), 48.0);
        assert!(on_screen(r, phone()));
        assert!(!r.overlaps(&finger(touch, 48.0)));
    }

    #[test]
    fn a_pop_up_on_a_screen_offset_from_zero_stays_inside_that_screen() {
        let screen = Rect::new(100.0, 50.0, 400.0, 700.0);
        let touch = (110.0, 60.0);
        let r = place_popup(touch, (120.0, 60.0), screen, 48.0);
        assert!(on_screen(r, screen));
    }
}
