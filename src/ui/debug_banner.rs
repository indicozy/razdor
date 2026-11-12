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

/// Draws the strip when the debug mode is on (`diag::debug_mode`); nothing otherwise.
pub fn draw() {
    if !razdor::diag::debug_mode() {
        return;
    }
    let text = tr("Debug mode is on — FPS may drop");
    let (_, top, _, _) = platform::safe_insets();
    // On a phone the text is 14 dp (never under 12 sp); on the desktop 13 scaled px.
    let (height, size) = match display::dp_px() {
        Some(dp) => (30.0 * dp, 14.0 * dp),
        None => (24.0 * chrome::k(), 13.0 * chrome::k()),
    };
    let width = screen_width();
    let strip = top + height;
    draw_rectangle(0.0, 0.0, width, strip, FILL);
    // The text sits in the strip under the safe area, its baseline set by the size.
    chrome::shadow_centered(text, width / 2.0, top + (height + size * 0.7) / 2.0, size, chrome::CREAM);
}
