//! The debug mode's strip (Settings → «Прочее» → «Режим отладки», Razdor's): while the mode is on, a thin
//! strip at the top of every screen says so, so a player who turned it on does not wonder why the frames
//! drop. It is drawn last, over everything (`main`, before the pointer), takes no input, and sits below
//! the phone's notch or cutout (`platform::safe_insets`).

use macroquad::prelude::*;
use razdor::i18n::tr;
use razdor::platform;

use super::{chrome, display};

/// The strip's colour: a dark amber, the same on every screen.
const FILL: Color = Color::new(0.42, 0.22, 0.02, 0.94);

/// The strip's height and text size: on a phone (`dp`, px per dp) the text is 14 dp (never under
/// 12 sp); on the desktop 13 scaled px (`k`, the interface scale).
fn geometry(dp: Option<f32>, k: f32) -> (f32, f32) {
    match dp {
        Some(dp) => (30.0 * dp, 14.0 * dp),
        None => (24.0 * k, 13.0 * k),
    }
}

/// The strip's top (under the safe area), height and text size, in screen px.
fn strip() -> (f32, f32, f32) {
    let (_, top, _, _) = platform::safe_insets();
    let (height, size) = geometry(display::dp_px(), chrome::k());
    (top, height, size)
}

/// The strip's bottom edge in screen px while the debug mode is on, else 0.0. What sits at the top of
/// the screen (the FPS counter) goes under it, so the strip does not cover it.
pub fn bottom() -> f32 {
    if !razdor::diag::debug_mode() {
        return 0.0;
    }
    let (top, height, _) = strip();
    top + height
}

/// Draws the strip when the debug mode is on (`diag::debug_mode`); nothing otherwise.
pub fn draw() {
    if !razdor::diag::debug_mode() {
        return;
    }
    let text = tr("Debug mode is on — FPS may drop");
    let (top, height, size) = strip();
    let width = screen_width();
    let strip = top + height;
    draw_rectangle(0.0, 0.0, width, strip, FILL);
    // The text sits in the strip under the safe area, its baseline set by the size.
    chrome::shadow_centered(text, width / 2.0, top + (height + size * 0.7) / 2.0, size, chrome::CREAM);
}

#[cfg(test)]
mod tests {
    use super::geometry;

    #[test]
    fn strip_geometry_on_a_phone_and_on_the_desktop() {
        // A phone: 30 dp high, 14 dp text, whatever the scale.
        assert_eq!(geometry(Some(2.0), 1.5), (60.0, 28.0));
        // The desktop: 24 and 13 scaled px.
        assert_eq!(geometry(None, 1.5), (36.0, 19.5));
    }
}
