//! The frame's display state: the window's size and DPI, read once per loop iteration (`main`, before anything is
//! drawn) and kept, so the map's draw path reads plain values instead of asking the window for each sprite. Before the
//! first refresh, and while the stored size is empty, it asks the window, as it did before.

use macroquad::prelude::*;
use razdor::local;
use std::cell::Cell;

local! {
    /// The window's (width, height, DPI) as the loop last read them; (0, 0, 1) before the first refresh.
    static SCREEN: Cell<(f32, f32, f32)> = const { Cell::new((0.0, 0.0, 1.0)) };
}

/// Reads the window's size and DPI for this frame (called once per loop iteration).
pub fn refresh() {
    SCREEN.set((screen_width(), screen_height(), screen_dpi_scale()));
}

/// Tests: the window's size as this frame sees it, without a window (the layout code reads it).
#[cfg(test)]
pub fn set_for_test(w: f32, h: f32) {
    SCREEN.set((w, h, 1.0));
}

fn current() -> (f32, f32, f32) {
    let s = SCREEN.get();
    if s.0 > 0.0 && s.1 > 0.0 {
        s
    } else {
        (screen_width(), screen_height(), screen_dpi_scale())
    }
}

/// The window's width in pixels, as this frame sees it.
pub fn screen_w() -> f32 {
    current().0
}

/// The window's height in pixels, as this frame sees it.
pub fn screen_h() -> f32 {
    current().1
}

/// The window's DPI scale, as this frame sees it.
pub fn dpi() -> f32 {
    current().2
}
