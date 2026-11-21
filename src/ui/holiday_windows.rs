//! The holiday look's windows, drawn over the main menu: the welcome of the season start («OK» and
//! «Письмо Деду Морозу»), the letter to Ded Moroz (a multi-line field, «Отправить» / «Отмена») and
//! the note that the letter is on its way. The state lives here; the season logic is in `holiday`,
//! the letters' storage in `razdor::letters`.

use std::cell::{Cell, RefCell};

use macroquad::prelude::*;
use razdor::i18n::{lang, tr, Lang};
use razdor::letters::{self, Letter, MAX_CHARS};
use razdor::local;

use super::audio::{cue, Cue};
use super::chrome::{self, Skin};
use super::widgets::{button_sounding, key, text_field_sized, wrap};

local! {
    /// The welcome window is up (`holiday::greeting_open`).
    static GREETING: Cell<bool> = Cell::new(false);
    /// The letter window is up.
    static LETTER: Cell<bool> = Cell::new(false);
    /// The note that the letter is on its way is up.
    static NOTE: Cell<bool> = Cell::new(false);
    /// The letter's text as it is typed.
    static TEXT: RefCell<String> = RefCell::new(String::new());
}

/// Opens the welcome window.
pub fn open_greeting() {
    GREETING.set(true);
}

/// Whether one of the holiday windows is up (the menu's buttons wait for it).
pub fn up() -> bool {
    GREETING.get() || LETTER.get() || NOTE.get()
}

/// Opens the letter window (from the welcome, or the menu's link later in the season).
pub fn open_letter() {
    GREETING.set(false);
    NOTE.set(false);
    LETTER.set(true);
}

/// The letter as it is sent: the interface language, the program's version and the text, stamped now.
fn letter_now(text: &str) -> Letter {
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let lang = match lang() {
        Lang::En => "en",
        Lang::Ru => "ru",
    };
    Letter { time, lang: lang.into(), version: env!("CARGO_PKG_VERSION").into(), text: text.into() }
}

/// Keeps the letter in the settings folder. True when it is kept; a failure is logged and the
/// letter window stays up, so the player can try again.
fn keep_letter(text: &str) -> bool {
    let Some(dir) = razdor::platform::config_dir() else {
        razdor::diag!("letters: no settings folder; the letter is not kept");
        return false;
    };
    match letters::append(&dir, &letter_now(text)) {
        Ok(()) => true,
        Err(why) => {
            razdor::diag!("letters: the letter is not kept: {why}");
            false
        }
    }
}

/// Draws the open holiday window (at most one at a time) and takes its buttons. Returns nothing: the
/// menu under it takes no press while one is up.
pub fn frame() {
    // On a phone the windows keep the legacy text metrics the main menu's question had (`dt_font`).
    let phone = super::display::dp_px().is_some();
    super::dt_font::legacy_metrics_if(phone, || {
        if LETTER.get() {
            letter_window();
        } else if NOTE.get() {
            note_window();
        } else if GREETING.get() {
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
    let (title_h, pad, size, gap) = (46.0 * unit, 22.0 * unit, 16.0 * unit, 14.0 * unit);
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
    let bw = (inner - gap) / 2.0;
    let ok = button_sounding(r.x + pad, by, bw, btn_h, tr("OK"), true, Cue::MenuPress) || key(KeyCode::Escape) || key(KeyCode::Enter) || closed;
    let letter = button_sounding(r.x + pad + bw + gap, by, bw, btn_h, tr("Letter to Ded Moroz"), true, Cue::MenuPress);
    if ok || letter {
        cue(Cue::MenuPress);
        GREETING.set(false);
        if letter {
            LETTER.set(true);
        }
    }
}

fn letter_window() {
    let (w, sw, sh, unit) = sizes();
    let (title_h, pad, size, gap) = (46.0 * unit, 22.0 * unit, 16.0 * unit, 14.0 * unit);
    let inner = w - 2.0 * pad;
    let hint = tr("What would you like in Razdor next year?");
    let hint_lines = wrap(hint, inner, size);
    let field_h = 150.0 * unit;
    let btn_h = 44.0 * unit;
    let h = title_h + hint_lines.len() as f32 * size * 1.35 + 12.0 * unit + field_h + 34.0 * unit + btn_h + pad;
    let r = Rect::new((sw - w) / 2.0, ((sh - h) / 2.0).max(0.0), w, h);
    let (r, closed) = chrome::window(r, tr("Letter to Ded Moroz"), Skin::Marble, true);
    let mut y = r.y + title_h;
    for line in &hint_lines {
        y += size * 1.35;
        chrome::shadow_text(line, r.x + pad, y, size, chrome::CREAM);
    }
    let field_y = y + 12.0 * unit;
    let changed = TEXT.with(|t| {
        let mut t = t.borrow_mut();
        let changed = text_field_sized("letter", r.x + pad, field_y, inner, field_h, &mut t, true, size);
        // The field stops at the letter's limit: the rest of a paste is cut.
        if t.chars().count() > MAX_CHARS {
            *t = t.chars().take(MAX_CHARS).collect();
        }
        changed
    });
    let _ = changed;
    let count = TEXT.with(|t| t.borrow().chars().count());
    let counter = format!("{count} / {MAX_CHARS}");
    let by = field_y + field_h + 6.0 * unit;
    chrome::shadow_text(&counter, r.x + pad, by + size, size * 0.85, chrome::CREAM);
    let bty = r.y + h - pad - btn_h;
    let bw = (inner - gap) / 2.0;
    let send = button_sounding(r.x + pad, bty, bw, btn_h, tr("Send"), true, Cue::MenuPress);
    let cancel = button_sounding(r.x + pad + bw + gap, bty, bw, btn_h, tr("Cancel"), true, Cue::MenuPress) || key(KeyCode::Escape) || closed;
    if send {
        let text = TEXT.with(|t| t.borrow().clone());
        if let Some(clean) = letters::clean(&text) {
            cue(Cue::MenuPress);
            if keep_letter(&clean) {
                TEXT.with(|t| t.borrow_mut().clear());
                LETTER.set(false);
                NOTE.set(true);
            }
        }
    } else if cancel {
        cue(Cue::MenuPress);
        LETTER.set(false);
        TEXT.with(|t| t.borrow_mut().clear());
    }
}

fn note_window() {
    let (w, sw, sh, unit) = sizes();
    let (title_h, pad, size) = (46.0 * unit, 22.0 * unit, 16.0 * unit);
    let inner = w - 2.0 * pad;
    let lines = wrap(tr("Your letter is on its way to Ded Moroz!"), inner, size);
    let btn_h = 44.0 * unit;
    let h = title_h + lines.len() as f32 * size * 1.35 + 30.0 * unit + btn_h + pad;
    let r = Rect::new((sw - w) / 2.0, ((sh - h) / 2.0).max(0.0), w, h);
    let (r, closed) = chrome::window(r, tr("Letter to Ded Moroz"), Skin::Marble, true);
    let mut y = r.y + title_h;
    for line in &lines {
        y += size * 1.35;
        chrome::shadow_text(line, r.x + pad, y, size, chrome::CREAM);
    }
    let by = r.y + h - pad - btn_h;
    if button_sounding(r.x + pad, by, inner, btn_h, tr("OK"), true, Cue::MenuPress) || key(KeyCode::Escape) || key(KeyCode::Enter) || closed {
        cue(Cue::MenuPress);
        NOTE.set(false);
    }
}
