//! The cheat console (a Razdor extra, issue #1): `~` (the key left of 1, Ё on a Russian
//! layout) on the world map and in battle opens a command line over the top of the screen
//! with a short scroll-back. The commands are `rules::cheats`; `help` lists them. While it is
//! open every key goes to it: the screen below takes none.

use crate::ui::input::{is_key_pressed, mouse_wheel};
use macroquad::prelude::*;

use razdor::i18n::tr;

use super::chrome;
use super::widgets::*;

/// Lines the scroll-back keeps, and how many show at once.
const KEPT: usize = 80;
const SHOWN: usize = 9;
/// Commands Up / Down bring back.
const RECALLED: usize = 30;

/// The characters of the console's key: they open and close it, and are never typed.
pub fn is_toggle(c: char) -> bool {
    matches!(c, '`' | '~' | 'ё' | 'Ё')
}

/// What a line of the scroll-back is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A command as it was typed.
    Typed,
    /// What it did.
    Said,
    /// Why it did not work.
    Error,
}

#[derive(Default)]
pub struct Console {
    pub open: bool,
    line: String,
    log: Vec<(String, Kind)>,
    recall: Vec<String>,
    /// The command Up brought back (index into `recall`).
    recall_at: Option<usize>,
    /// Lines scrolled up from the newest.
    scroll: usize,
}

impl Console {
    pub fn print(&mut self, line: impl Into<String>, kind: Kind) {
        self.log.push((line.into(), kind));
        if self.log.len() > KEPT {
            self.log.remove(0);
        }
        self.scroll = 0;
    }

    /// Before the screen's frame: opens on the console's key where it is `allowed`, and while
    /// open takes the typing. Returns whether the screen must take no input this frame (also
    /// on the frame the console closes, so its Esc does not reach the game) and the command
    /// entered, if any.
    pub fn input(&mut self, allowed: bool) -> (bool, Option<String>) {
        let key = is_key_pressed(KeyCode::GraveAccent);
        if !self.open {
            if !allowed {
                return (false, None);
            }
            // Nobody else reads characters on the map or in battle.
            let mut chars = Vec::new();
            while let Some(c) = get_char_pressed() {
                chars.push(c);
            }
            if key || chars.into_iter().any(is_toggle) {
                self.open = true;
                if self.log.is_empty() {
                    self.print(tr("Cheat console: help lists the commands; ~ or Esc closes it."), Kind::Said);
                }
                return (true, None);
            }
            return (false, None);
        }
        if !allowed {
            self.open = false;
            return (false, None);
        }
        let mut chars = Vec::new();
        while let Some(c) = get_char_pressed() {
            chars.push(c);
        }
        if key || is_key_pressed(KeyCode::Escape) || chars.iter().copied().any(is_toggle) {
            self.open = false;
            return (true, None);
        }
        let typed: Vec<char> = chars.into_iter().filter(|c| !is_toggle(*c)).collect();
        apply_typing(&mut self.line, &typed, backspace_repeat(), false, false, 200);
        if is_key_pressed(KeyCode::Up) && !self.recall.is_empty() {
            let at = self.recall_at.map_or(self.recall.len() - 1, |i| i.saturating_sub(1));
            self.recall_at = Some(at);
            self.line = self.recall[at].clone();
        }
        if is_key_pressed(KeyCode::Down) {
            match self.recall_at {
                Some(i) if i + 1 < self.recall.len() => {
                    self.recall_at = Some(i + 1);
                    self.line = self.recall[i + 1].clone();
                }
                _ => {
                    self.recall_at = None;
                    self.line.clear();
                }
            }
        }
        let most = self.log.len().saturating_sub(SHOWN);
        if is_key_pressed(KeyCode::PageUp) || wheel_raw() > 0.0 {
            self.scroll = (self.scroll + 3).min(most);
        }
        if is_key_pressed(KeyCode::PageDown) || wheel_raw() < 0.0 {
            self.scroll = self.scroll.saturating_sub(3);
        }
        let mut entered = None;
        if enter_pressed() && !self.line.trim().is_empty() {
            let line = std::mem::take(&mut self.line).trim().to_string();
            if self.recall.last() != Some(&line) {
                self.recall.push(line.clone());
                if self.recall.len() > RECALLED {
                    self.recall.remove(0);
                }
            }
            self.recall_at = None;
            self.print(format!("> {line}"), Kind::Typed);
            entered = Some(line);
        }
        (true, entered)
    }

    /// Draws the console over the top of the screen, when open.
    pub fn draw(&self) {
        if !self.open {
            return;
        }
        let k = chrome::k();
        let size = (16.0 * k).round();
        let pitch = (size * 1.3).round();
        let pad = 10.0 * k;
        let w = screen_width();
        let h = pad * 2.0 + pitch * (SHOWN as f32 + 1.0) + 6.0 * k;
        draw_rectangle(0.0, 0.0, w, h, Color::new(0.02, 0.02, 0.02, 0.86));
        draw_line(0.0, h, w, h, 2.0 * k, chrome::GOLD);
        let end = self.log.len() - self.scroll.min(self.log.len());
        let start = end.saturating_sub(SHOWN);
        for (i, (line, kind)) in self.log[start..end].iter().enumerate() {
            let color = match kind {
                Kind::Typed => DIM,
                Kind::Said => INK,
                Kind::Error => chrome::RED_TEXT,
            };
            text_fit(line, pad, pad + pitch * (i as f32 + 0.8), w - 2.0 * pad, size, color);
        }
        let caret = if (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { "" };
        let ly = pad + pitch * (SHOWN as f32 + 0.8) + 4.0 * k;
        draw_rectangle(pad * 0.5, ly - size - 2.0 * k, w - pad, pitch + 2.0 * k, FIELD_BG);
        let mut shown = format!("> {}{caret}", self.line);
        while measure(&shown, size).width > w - 3.0 * pad && shown.chars().count() > 2 {
            shown.remove(2);
        }
        text(&shown, pad, ly, size, ACCENT);
        if self.scroll > 0 {
            let note = tr("PgDn: newer lines");
            let nw = measure(note, size * 0.85).width;
            text(note, w - nw - pad, pad + pitch * 0.8, size * 0.85, DIM);
        }
    }
}

/// The wheel whatever holds the input (the console holds it back from the screen).
fn wheel_raw() -> f32 {
    mouse_wheel().1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_never_typed() {
        for c in ['`', '~', 'ё', 'Ё'] {
            assert!(is_toggle(c));
        }
        for c in ['e', 'е', 'Е', '1', ' '] {
            assert!(!is_toggle(c));
        }
    }

    #[test]
    fn the_scroll_back_keeps_its_last_lines() {
        let mut c = Console::default();
        for i in 0..KEPT + 5 {
            c.print(format!("{i}"), Kind::Said);
        }
        assert_eq!(c.log.len(), KEPT);
        assert_eq!(c.log[0].0, "5");
    }
}
