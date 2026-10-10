//! The inventory filter (a Razdor extra, issue #1): a search line over the hero's backpack
//! and the market's lists. Typing a letter starts it (Ctrl+F or a click on it too), the list
//! keeps the items whose name, type, stats, bonus or description hold every word typed
//! (`rules::items::filter_match`: any case, Ё as Е), Enter takes the first match and Esc
//! clears it.

use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::rules::content::{Content, ItemId};
use razdor::rules::items::filter_match;
use razdor::search::Match;

use super::chrome;
use super::widgets::*;

/// What the filter's keys did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    /// Enter: the screen takes the first match.
    pub pick: bool,
    /// The filter used this frame's Esc (or holds the keyboard): the screen must not close.
    pub keys_taken: bool,
}

/// The items of `list` that `query` keeps, with their matches, in order.
pub fn keep(content: &Content, list: impl IntoIterator<Item = (usize, ItemId)>, query: &str) -> Vec<(usize, Match)> {
    list.into_iter().filter_map(|(i, item)| filter_match(content, item, query).map(|m| (i, m))).collect()
}

/// The filter line in `r`, for the field `key`. `shortcuts` are the keys of the screen that
/// must keep their meaning while the line is empty and not typed in (A closes the army
/// screen, N switches the music): they do not start a search. `found` and `of` give the count
/// shown at its right.
pub fn field(key: &str, query: &mut String, r: Rect, shortcuts: &[KeyCode], found: usize, of: usize) -> Reply {
    let mut reply = Reply::default();
    let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
    if !input_blocked() {
        if !has_focus(key) {
            let shortcut = shortcuts.iter().any(|&k| is_key_pressed(k));
            let mut chars = Vec::new();
            while let Some(c) = get_char_pressed() {
                chars.push(c);
            }
            if (ctrl && is_key_pressed(KeyCode::F)) || (clicked() && mouse_in(r.x, r.y, r.w, r.h)) {
                take_focus(key);
            } else if !ctrl && !shortcut && chars.first().is_some_and(|c| c.is_alphanumeric()) {
                // A letter typed over the list starts the search with it.
                take_focus(key);
                apply_typing(query, &chars, 0, false, false, 60);
            } else if is_key_pressed(KeyCode::Escape) && !query.is_empty() {
                query.clear();
                reply.keys_taken = true;
            }
        } else {
            reply.keys_taken = true;
            let mut chars = Vec::new();
            while let Some(c) = get_char_pressed() {
                chars.push(c);
            }
            if !ctrl {
                apply_typing(query, &chars, backspace_repeat(), false, false, 60);
            }
            if is_key_pressed(KeyCode::Escape) {
                query.clear();
                clear_focus();
            } else if enter_pressed() {
                reply.pick = !query.trim().is_empty();
            } else if clicked() && !mouse_in(r.x, r.y, r.w, r.h) {
                clear_focus();
            }
        }
    }
    let k = chrome::k();
    let active = has_focus(key);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.03, 0.03, 0.03, 0.75));
    draw_rectangle_lines(r.x, r.y, r.w, r.h, if active { 2.0 } else { 1.0 }, if active { ACCENT } else { chrome::SILVER_DARK });
    let size = (r.h * 0.72).round().min(16.0 * k);
    let ty = r.y + r.h / 2.0 + size * 0.36;
    let count = if query.trim().is_empty() { String::new() } else { razdor::trf!("{found} of {of}", found, of) };
    let cw = measure(&count, size).width;
    text(&count, r.right() - cw - 6.0 * k, ty, size, if found == 0 { chrome::RED_TEXT } else { DIM });
    let room = r.w - cw - 18.0 * k;
    if query.is_empty() && !active {
        text_fit(tr("Search: type a name (Ctrl+F)"), r.x + 6.0 * k, ty, room, size, DIM);
    } else {
        let caret = if active && (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { "" };
        let mut shown = format!("{query}{caret}");
        while measure(&shown, size).width > room && !shown.is_empty() {
            shown.remove(0);
        }
        text(&shown, r.x + 6.0 * k, ty, size, INK);
    }
    reply
}

