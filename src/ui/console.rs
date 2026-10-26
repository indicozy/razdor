//! The cheat console (a Razdor extra, issue #1): `~` (the key left of 1, Ё on a Russian
//! layout) on the world map and in battle opens a command line over the top of the screen
//! with a short scroll-back. The commands are `rules::cheats`; `help` lists them. While it is
//! open every key goes to it: the screen below takes none.

use crate::ui::input::{is_key_pressed, is_mouse_button_pressed, mouse_position, mouse_wheel};
use macroquad::prelude::*;

use razdor::i18n::tr;

use super::chrome;
use super::display::{self, SafeArea};
use super::keymap::{self, Action};
use super::widgets::*;

/// Lines the scroll-back keeps, and how many show at once.
const KEPT: usize = 80;
const SHOWN: usize = 9;
/// Commands Up / Down bring back.
const RECALLED: usize = 30;
/// On a phone (dp): the text keeps at least this far from each side, clear of the rounded
/// corners, and the close box is a finger's target.
const SIDE_DP: f32 = 24.0;
const CLOSE_DP: f32 = 44.0;

/// Where the console's content sits: `content` is the screen inside the safe area (the dark
/// band is always the full width), `close` the close box (a phone only), and `text_x` to
/// `text_right` the span the text and the input line take, left of the close box.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub content: Rect,
    pub close: Option<Rect>,
    pub text_x: f32,
    pub text_right: f32,
}

/// The console's layout on a screen of `screen` px with the safe `area`; `dp` is the px per dp
/// on a phone (None on desktop) and `pad` the console's inner margin in px. On a phone the
/// sides keep at least 24 dp, insets or not, for the rounded corners; the top takes the safe
/// area's top. Desktop with no insets: the whole screen, as before.
pub fn layout(screen: (f32, f32), area: SafeArea, dp: Option<f32>, pad: f32) -> Layout {
    let (w, h) = screen;
    let min_side = dp.map_or(0.0, |d| SIDE_DP * d);
    let left = area.left.max(min_side);
    let right = area.right.max(min_side);
    let top = area.top;
    let content = Rect::new(left, top, (w - left - right).max(0.0), (h - top).max(0.0));
    let close = dp.map(|d| {
        let s = CLOSE_DP * d;
        Rect::new(content.right() - s, content.y, s, s)
    });
    let reserved = close.map_or(0.0, |c| c.w + pad);
    Layout { content, close, text_x: content.x + pad, text_right: content.right() - pad - reserved }
}

/// The layout now: the screen, its safe area and whether it is a phone.
fn layout_now(pad: f32) -> Layout {
    layout((screen_width(), screen_height()), display::safe_area(), display::dp_px(), pad)
}

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
    /// The line's caret and selection.
    edit: super::text_edit::TextEdit,
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
        let key = is_key_pressed(keymap::key(Action::Console));
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
        // The close box (a phone): a tap on it closes the console and takes the tap, so it
        // does not reach the game under it (`widgets::swallow_input`).
        let (mx, my) = mouse_position();
        if let Some(c) = layout_now(10.0 * chrome::k()).close {
            if is_mouse_button_pressed(MouseButton::Left) && c.contains(vec2(mx, my)) {
                self.open = false;
                swallow_input();
                return (true, None);
            }
        }
        let chars = typed_chars();
        if key || esc_pressed() || chars.iter().copied().any(is_toggle) {
            self.open = false;
            return (true, None);
        }
        let typed: Vec<char> = chars.into_iter().filter(|c| !is_toggle(*c)).collect();
        let rules = EditRules { max_chars: 200, multiline: false, allow: |c| !is_toggle(c) };
        edit_keys(&mut self.line, &mut self.edit, &typed, rules);
        if is_key_pressed(KeyCode::Up) && !self.recall.is_empty() {
            let at = self.recall_at.map_or(self.recall.len() - 1, |i| i.saturating_sub(1));
            self.recall_at = Some(at);
            self.line = self.recall[at].clone();
            self.edit = Default::default();
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
            self.edit = Default::default();
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
            self.edit = Default::default();
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
        let l = layout_now(pad);
        let (w, top) = (screen_width(), l.content.y);
        let h = top + pad * 2.0 + pitch * (SHOWN as f32 + 1.0) + 6.0 * k;
        draw_rectangle(0.0, 0.0, w, h, Color::new(0.02, 0.02, 0.02, 0.86));
        draw_line(0.0, h, w, h, 2.0 * k, chrome::GOLD);
        let end = self.log.len() - self.scroll.min(self.log.len());
        let start = end.saturating_sub(SHOWN);
        let text_w = l.text_right - l.text_x;
        for (i, (line, kind)) in self.log[start..end].iter().enumerate() {
            let color = match kind {
                Kind::Typed => DIM,
                Kind::Said => INK,
                Kind::Error => chrome::RED_TEXT,
            };
            text_fit(line, l.text_x, top + pad + pitch * (i as f32 + 0.8), text_w, size, color);
        }
        let ly = top + pad + pitch * (SHOWN as f32 + 0.8) + 4.0 * k;
        let field_x = l.content.x + pad * 0.5;
        draw_rectangle(field_x, ly - size - 2.0 * k, l.text_right + pad * 0.5 - field_x, pitch + 2.0 * k, FIELD_BG);
        text("> ", l.text_x, ly, size, ACCENT);
        let x = l.text_x + measure("> ", size).width;
        let mut edit = self.edit;
        let look = LineLook { size, color: ACCENT, centred: false, shadow: false };
        edit_line(field_id("console"), &self.line, &mut edit, true, false, Rect::new(x, ly - size, l.text_right - x, pitch), ly, look);
        if self.scroll > 0 {
            let note = tr("PgDn: newer lines");
            let nw = measure(note, size * 0.85).width;
            text(note, l.text_right - nw, top + pad + pitch * 0.8, size * 0.85, DIM);
        }
        if let Some(c) = l.close {
            // The tap that closes it is taken in `input`; this only draws the box.
            chrome::close_button(c.x, c.y, c.w, true);
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
    fn on_desktop_the_console_takes_the_whole_screen() {
        let l = layout((1280.0, 720.0), SafeArea::default(), None, 10.0);
        assert_eq!(l.content, Rect::new(0.0, 0.0, 1280.0, 720.0));
        assert!(l.close.is_none(), "no close box on desktop");
        assert_eq!((l.text_x, l.text_right), (10.0, 1270.0), "as before the phone layout");
    }

    #[test]
    fn a_phone_keeps_the_text_off_the_corners_and_shows_a_finger_sized_close_box() {
        let dp = 2.625;
        let l = layout((1080.0, 2400.0), SafeArea::default(), Some(dp), 10.0);
        assert_eq!(l.content.x, 24.0 * dp, "24 dp clear on the left, insets or not");
        assert_eq!(l.content.right(), 1080.0 - 24.0 * dp);
        let c = l.close.expect("a close box on a phone");
        assert!(c.w >= 44.0 * dp - 0.5, "at least 44 dp");
        assert_eq!(c.right(), l.content.right(), "top right of the content");
        assert_eq!(c.y, 0.0);
        assert!(l.text_right <= c.x - 10.0 + 0.5, "the text stops before the box");
    }

    #[test]
    fn a_cutout_moves_the_content_in_while_the_band_stays_full_width() {
        let dp = 2.625;
        let area = SafeArea { left: 90.0, top: 110.0, ..Default::default() };
        let l = layout((2400.0, 1080.0), area, Some(dp), 10.0);
        assert_eq!(l.content.x, 90.0, "the cutout's inset beats the 24 dp minimum");
        assert_eq!(l.content.y, 110.0, "the text starts below the top inset");
        assert_eq!(l.content.right(), 2400.0 - 24.0 * dp);
        assert_eq!(l.text_x, 100.0);
    }

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
