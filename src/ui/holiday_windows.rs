//! The holiday look's welcome window, drawn over the main menu (the season start, «OK»). The state
//! lives here; the season logic is in `holiday`.

use std::cell::Cell;

use macroquad::prelude::*;
use razdor::i18n::tr;
use razdor::local;

use super::audio::{cue, Cue};
use super::chrome::{self, Skin};
use super::widgets::{button_sounding, key, wrap};

local! {
    /// The welcome window is up (`holiday::greeting_open`).
    static GREETING: Cell<bool> = Cell::new(false);
}

/// Opens the welcome window.
pub fn open_greeting() {
    GREETING.set(true);
}

/// Whether the welcome window is up (the menu's buttons wait for it).
pub fn up() -> bool {
    GREETING.get()
}

/// Draws the welcome window when it is up and takes its button. Returns nothing: the menu under it
/// takes no press while it is up.
pub fn frame() {
    // On a phone the windows keep the legacy text metrics the main menu's question had (`dt_font`).
    let phone = super::display::dp_px().is_some();
    super::dt_font::legacy_metrics_if(phone, || {
        if GREETING.get() {
            greeting_window();
        }
    });
}

/// The window's size and unit: a phone's dp (the screen's fraction capped at 440 dp), else the desktop's scale.
fn sizes() -> (f32, f32, f32, f32) {
    let phone = super::display::dp_px();
    let unit = phone.unwrap_or_else(chrome::k);
    let (sw, sh) = (screen_width(), screen_height());
    let w = if phone.is_some() { (sw * 0.9).min(440.0 * unit) } else { 440.0 * chrome::k() };
    (w, sw, sh, unit)
}

fn greeting_window() {
    let (w, sw, sh, unit) = sizes();
    let body = tr("Happy holidays! We've dressed up the world of Razdor: snow, garlands, Santa hats and a sleigh in the sky. The Christmas mod is already on. You can turn it off any time in Maps and mods and in the settings. Warmth to you and glorious victories!");
    let (title_h, pad, size) = (46.0 * unit, 22.0 * unit, 16.0 * unit);
    let inner = w - 2.0 * pad;
    let lines = wrap(body, inner, size);
    let btn_h = 44.0 * unit;
    let h = title_h + lines.len() as f32 * size * 1.35 + 30.0 * unit + btn_h + pad + 16.0 * unit;
    let r = Rect::new((sw - w) / 2.0, ((sh - h) / 2.0).max(0.0), w, h);
    let (r, closed) = chrome::window(r, tr("Holiday look"), Skin::Marble, true);
    let mut y = r.y + title_h;
    for line in &lines {
        y += size * 1.35;
        chrome::shadow_text(line, r.x + pad, y, size, chrome::CREAM);
    }
    let by = r.y + h - pad - btn_h;
    if button_sounding(r.x + pad, by, inner, btn_h, tr("OK"), true, Cue::MenuPress) || key(KeyCode::Escape) || key(KeyCode::Enter) || closed {
        cue(Cue::MenuPress);
        GREETING.set(false);
    }
}
