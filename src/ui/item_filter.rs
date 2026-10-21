//! The inventory filter (a Razdor extra, issue #1): a search line over the hero's backpack
//! and the market's lists. Typing a letter starts it (Ctrl+F or a click on it too), the list
//! keeps the items whose name, type, stats, bonus or description hold every word typed
//! (`rules::items::filter_match`: any case, Ё as Е), Enter takes the first match and Esc
//! clears it.

use crate::ui::input::{is_key_down, is_key_pressed};
use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::rules::content::{ArtefactType, Content, ItemId};
use razdor::rules::items::filter_match;
use razdor::rules::pack_order::{ItemGroup, PackSort};
use razdor::search::Match;

use super::assets::Assets;
use super::chrome;
use super::widgets::*;

/// The group buttons in their row, left to right (the owner's approved order; `All` has none:
/// a lit button is shown again by a click on it).
pub const ROW: [ItemGroup; 8] = [ItemGroup::Helms, ItemGroup::Armour, ItemGroup::Weapons, ItemGroup::Shields, ItemGroup::Rings, ItemGroup::Amulets, ItemGroup::Potions, ItemGroup::Other];

/// What the filter's keys did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    /// Enter: the screen takes the first match.
    pub pick: bool,
    /// The filter used this frame's Esc (or holds the keyboard): the screen must not close.
    pub keys_taken: bool,
}

/// The name of a group (the hint shown over its button).
pub fn group_label(group: ItemGroup) -> &'static str {
    match group {
        ItemGroup::All => tr("All"),
        ItemGroup::Weapons => tr("Weapons"),
        ItemGroup::Armour => tr("Armour"),
        ItemGroup::Helms => tr("Helms"),
        ItemGroup::Shields => tr("Shields"),
        ItemGroup::Rings => tr("Rings"),
        ItemGroup::Amulets => tr("Amulets"),
        ItemGroup::Potions => tr("Potions"),
        ItemGroup::Other => tr("Other"),
    }
}

/// The item whose picture stands for a group: the lowest id of the kinds it holds, a blow
/// weapon first for Weapons. `None` when the content has no such item.
pub fn representative(content: &Content, group: ItemGroup) -> Option<ItemId> {
    let first = |pred: &dyn Fn(ArtefactType) -> bool| content.items.iter().filter(|d| pred(d.kind)).min_by_key(|d| d.id).map(|d| ItemId(d.id));
    if group == ItemGroup::Weapons {
        first(&|k| k == ArtefactType::BlowWeapon).or_else(|| first(&|k| group.holds(k)))
    } else {
        first(&|k| group.holds(k))
    }
}

/// The row of group buttons in `r` (Razdor's, the owner's approved design): one square-ish
/// button per group of [`ROW`], evenly spaced, each showing a picture of its group's
/// [`representative`] fitted inside it, its name as a hint while hovered. A click on a group
/// shows only that group and lights its button; a click on the lit button shows everything
/// again. Returns the group shown.
pub fn group_row(content: &Content, assets: &Assets, r: Rect, group: ItemGroup) -> ItemGroup {
    let k = chrome::k();
    let gap = 3.0 * k;
    let w = (r.w - gap * (ROW.len() - 1) as f32) / ROW.len() as f32;
    let mut shown = group;
    for (i, g) in ROW.into_iter().enumerate() {
        let b = Rect::new(r.x + i as f32 * (w + gap), r.y, w, r.h);
        let on = group == g;
        let (hover, down, pressed) = button_state(b, true);
        let bg = if on {
            Color::new(0.55, 0.42, 0.18, 1.0)
        } else if down {
            Color::new(0.36, 0.28, 0.18, 1.0)
        } else if hover {
            Color::new(0.28, 0.22, 0.14, 1.0)
        } else {
            Color::new(0.22, 0.18, 0.12, 1.0)
        };
        draw_rectangle(b.x, b.y, b.w, b.h, bg);
        draw_rectangle_lines(b.x, b.y, b.w, b.h, 1.0, if on { ACCENT } else { DIM });
        // The picture fitted inside the button with a small padding, square.
        let pad = 3.0 * k;
        let side = (b.w.min(b.h) - 2.0 * pad).max(0.0);
        if let Some(item) = representative(content, g) {
            assets.draw_item(item, b.x + (b.w - side) / 2.0, b.y + (b.h - side) / 2.0, side);
        }
        if pressed {
            claim();
            shown = if on { ItemGroup::All } else { g };
        }
        if hover {
            tooltip_later(&[(group_label(g).to_string(), INK)]);
        }
    }
    shown
}

/// The label of the backpack's order button (short: it shares the row with the groups).
pub fn sort_short_label(sort: PackSort) -> &'static str {
    match sort {
        PackSort::Shown => tr("As found"),
        PackSort::NameUp => tr("A–Z"),
        PackSort::NameDown => tr("Z–A"),
        PackSort::PriceDown => tr("Price ↓"),
        PackSort::PriceUp => tr("Price ↑"),
    }
}

/// The items of `list` that `query` keeps, with their matches, in order.
pub fn keep(content: &Content, list: impl IntoIterator<Item = (usize, ItemId)>, query: &str) -> Vec<(usize, Match)> {
    list.into_iter().filter_map(|(i, item)| filter_match(content, item, query, &super::unit_sheet::bonus_title).map(|m| (i, m))).collect()
}

/// The filter line in `r`, for the field `key`. `shortcuts` are the keys of the screen that
/// must keep their meaning while the line is empty and not typed in (A closes the army
/// screen, N switches the music): they do not start a search. `found` and `of` give the count
/// shown at its right.
pub fn field(key: &str, query: &mut String, r: Rect, shortcuts: &[KeyCode], found: usize, of: usize) -> Reply {
    field_with(key, query, r, shortcuts, found, of, true)
}

/// [`field`], drawn only while it is used (focused or holding a query) when `draw_idle` is
/// false: the screen draws its own content in `r` meanwhile (the market's column titles).
/// The keys work the same either way.
pub fn field_with(key: &str, query: &mut String, r: Rect, shortcuts: &[KeyCode], found: usize, of: usize, draw_idle: bool) -> Reply {
    field_hinted(key, query, r, shortcuts, found, of, draw_idle, tr("Search: type a name (Ctrl+F)"))
}

/// [`field_with`] showing `hint` while empty (the custom battle's lists, where a typed
/// letter jumps instead: the caller takes the typed characters first).
#[allow(clippy::too_many_arguments)]
pub fn field_hinted(key: &str, query: &mut String, r: Rect, shortcuts: &[KeyCode], found: usize, of: usize, draw_idle: bool, hint: &str) -> Reply {
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
                let mut ed = super::text_edit::TextEdit::at_end();
                ed.type_chars(query, &chars, 60);
                set_focus_edit(key, ed);
            } else if is_key_pressed(KeyCode::Escape) && !query.is_empty() {
                query.clear();
                reply.keys_taken = true;
            }
        } else {
            reply.keys_taken = true;
            let mut ed = focus_edit(key);
            edit_keys(query, &mut ed, &typed_chars(), EditRules::line(60));
            set_focus_edit(key, ed);
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
    if !draw_idle && !active && query.is_empty() {
        return reply;
    }
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.03, 0.03, 0.03, 0.75));
    draw_rectangle_lines(r.x, r.y, r.w, r.h, if active { 2.0 } else { 1.0 }, if active { ACCENT } else { chrome::SILVER_DARK });
    let size = (r.h * 0.72).round().min(16.0 * k);
    let ty = r.y + r.h / 2.0 + size * 0.36;
    let count = if query.trim().is_empty() { String::new() } else { razdor::trf!("{found} of {of}", found, of) };
    let cw = measure(&count, size).width;
    text(&count, r.right() - cw - 6.0 * k, ty, size, if found == 0 { chrome::RED_TEXT } else { DIM });
    let room = r.w - cw - 18.0 * k;
    if query.is_empty() && !active {
        text_fit(hint, r.x + 6.0 * k, ty, room, size, DIM);
    } else {
        let mut ed = focus_edit(key);
        let look = LineLook { size, color: INK, centred: false, shadow: false };
        edit_line(field_id(key), query, &mut ed, active, true, Rect::new(r.x + 6.0 * k, r.y, room, r.h), ty, look);
        set_focus_edit(key, ed);
    }
    reply
}

