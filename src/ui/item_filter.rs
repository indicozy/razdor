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
///
/// A Razdor extra (the players' request): a group whose lowest item does not look like the
/// group prefers the lowest item whose name holds a word of [`picture_word`]. The Shields
/// button would otherwise show the bracers («Кожаные наручи», the game's shield kind).
pub fn representative(content: &Content, group: ItemGroup) -> Option<ItemId> {
    let first = |pred: &dyn Fn(ArtefactType) -> bool| content.items.iter().filter(|d| pred(d.kind)).min_by_key(|d| d.id).map(|d| ItemId(d.id));
    if let Some(word) = picture_word(group) {
        let named = content.items.iter().filter(|d| group.holds(d.kind) && d.name.to_lowercase().contains(word)).min_by_key(|d| d.id);
        if let Some(d) = named {
            return Some(ItemId(d.id));
        }
    }
    if group == ItemGroup::Weapons {
        first(&|k| k == ArtefactType::BlowWeapon).or_else(|| first(&|k| group.holds(k)))
    } else {
        first(&|k| group.holds(k))
    }
}

/// The word (lower case) an item's name must hold for the group's picture to prefer it (see
/// [`representative`]). Shields: «щит», the shield's own name in the game's data.
fn picture_word(group: ItemGroup) -> Option<&'static str> {
    match group {
        ItemGroup::Shields => Some("щит"),
        _ => None,
    }
}

/// `N` equal cells across `r` with small even gaps, left to right (the row's buttons).
pub fn row_cells<const N: usize>(r: Rect) -> [Rect; N] {
    let gap = 4.0 * chrome::k();
    let w = (r.w - gap * (N - 1) as f32) / N as f32;
    std::array::from_fn(|i| Rect::new(r.x + i as f32 * (w + gap), r.y, w, r.h))
}

/// A button of the row: the green marble art of the original's buttons, three-sliced so its
/// ends keep their shape, Down while `lit`. A hover edge marks an unlit one. Without the art, a
/// plain green panel.
fn row_face(r: Rect, lit: bool, hover: bool) {
    let name = if r.w < 110.0 { "Btn1" } else if r.w < 145.0 { "Btn2" } else { "Btn3" };
    let state = if lit { "Down" } else { "Up" };
    match chrome::win_fx(&format!("{name}{state}"), chrome::Fx::Plain) {
        Some(t) => chrome::three_slice(&t, r, 14.0, WHITE),
        None => {
            let bg = if lit { Color::new(0.16, 0.36, 0.28, 1.0) } else { Color::new(0.10, 0.25, 0.19, 1.0) };
            draw_rectangle(r.x, r.y, r.w, r.h, bg);
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, chrome::SILVER);
        }
    }
    if lit {
        draw_rectangle_lines(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, 2.0, ACCENT);
    } else if hover {
        draw_rectangle_lines(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, 1.0, INK);
    }
}

/// The row of group buttons, one per group of [`ROW`] in `cells` (Razdor's, the owner's approved
/// design): each button shows a picture of its group's [`representative`] inside it, its name
/// as a hint while hovered. A click on a group shows only that group and holds its button down;
/// a click on the lit button shows everything again. Returns the group shown.
pub fn group_row(content: &Content, assets: &Assets, cells: &[Rect], group: ItemGroup) -> ItemGroup {
    let k = chrome::k();
    let mut shown = group;
    for (b, g) in cells.iter().zip(ROW) {
        let on = group == g;
        let (hover, down, pressed) = button_state(*b, true);
        let lit = on || down;
        row_face(*b, lit, hover);
        // The picture inside the button, a little lower while it is held down.
        let pad = 0.07 * b.h;
        let side = (b.h - 2.0 * pad).min(b.w - 2.0 * pad).max(0.0);
        let dy = if lit { k } else { 0.0 };
        if let Some(item) = representative(content, g) {
            assets.draw_item(item, b.x + (b.w - side) / 2.0, b.y + (b.h - side) / 2.0 + dy, side);
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

/// The name of the backpack's order, for the hint.
pub fn sort_label(sort: PackSort) -> &'static str {
    match sort {
        PackSort::Shown => tr("As found"),
        PackSort::NameUp => tr("By name A–Z"),
        PackSort::NameDown => tr("By name Z–A"),
        PackSort::PriceDown => tr("Price ↓"),
        PackSort::PriceUp => tr("Price ↑"),
    }
}

/// The short order name, marked small on the search button when the order is not the default.
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

/// The boxed filter line in `r`, for the field `key` (the market's list). `shortcuts` are the keys
/// of the screen that must keep their meaning while the line is empty and not typed in (N switches
/// the music): they do not start a search. `found` and `of` give the count shown at its right. It
/// is drawn only while it is used (focused or holding a query) when `draw_idle` is false: the
/// screen draws its own content in `r` meanwhile (the market's column titles).
pub fn field_with(key: &str, query: &mut String, r: Rect, shortcuts: &[KeyCode], found: usize, of: usize, draw_idle: bool) -> Reply {
    field_hinted(key, query, r, shortcuts, found, of, draw_idle, tr("Search: type a name (Ctrl+F)"))
}

/// The search button: the last cell of the backpack's row. Idle it reads «Поиск» on the row's
/// marble; a click, Ctrl+F or a typed letter makes it the field, its text on the button's face.
/// It stays lit while a query is held, Esc clears it. The found and of count and the order
/// (`sort`, marked small on the button when it is not the default) show in its hover hint. The
/// screen takes a right click for the next order.
pub fn search_button(key: &str, query: &mut String, r: Rect, shortcuts: &[KeyCode], found: usize, of: usize, sort: PackSort) -> Reply {
    let reply = handle_keys(key, query, r, shortcuts);
    let k = chrome::k();
    let active = has_focus(key);
    let (hover, _, _) = button_state(r, true);
    let typed = !query.trim().is_empty();
    row_face(r, active || typed, hover);
    // A changed order is named small at the bottom of the button (fractions of its height: the
    // marble's bevel takes the outer tenth); the label then sits in the upper half.
    let marked = sort != PackSort::Shown;
    let (mid, size_max) = if marked { (0.4, 0.32) } else { (0.5, 0.5) };
    if active || !query.is_empty() {
        let size = (r.h * size_max).round().min(16.0 * k);
        let ty = r.y + mid * r.h + size * 0.36;
        let mut ed = focus_edit(key);
        let look = LineLook { size, color: chrome::CREAM, centred: false, shadow: false };
        edit_line(field_id(key), query, &mut ed, active, true, Rect::new(r.x + 4.0 * k, r.y, r.w - 8.0 * k, r.h), ty, look);
        set_focus_edit(key, ed);
    } else {
        // The label in the button's face, smaller than the other buttons' if it must be: the
        // cell is narrow.
        let label = tr("Search");
        super::typography::Role::Button.with(|| {
            let size = fit_size(label, r.w - 6.0 * k, (r.h * size_max).round().min(16.0 * k));
            let d = measure(label, size);
            let x = r.x + (r.w - d.width) / 2.0;
            let y = r.y + mid * r.h + size * 0.36;
            text(label, x + 1.0, y + 1.0, size, Color::new(0.0, 0.0, 0.0, 0.35));
            text(label, x, y, size, chrome::CREAM);
        });
    }
    if marked {
        // The body face: the smallest of the texts (the sheets fix a face's size).
        super::typography::Role::Hint.with(|| {
            let size = fit_size(sort_short_label(sort), r.w - 6.0 * k, 10.0 * k);
            let s = ellipsize(sort_short_label(sort), r.w - 6.0 * k, size);
            let d = measure(&s, size);
            text(&s, r.x + (r.w - d.width) / 2.0, r.y + 0.92 * r.h, size, chrome::CREAM);
        });
    }
    if hover && !active {
        let mut lines = Vec::new();
        if typed {
            lines.push((razdor::trf!("{found} of {of}", found, of), INK));
        }
        lines.push((tr("Search: type a name (Ctrl+F)").to_string(), INK));
        lines.push((razdor::trf!("Order: {order}", order = sort_label(sort)), INK));
        lines.push((tr("Right click: order").to_string(), INK));
        tooltip_later(&lines);
    }
    reply
}

/// [`field_with`] showing `hint` while empty (the custom battle's lists, where a typed
/// letter jumps instead: the caller takes the typed characters first).
#[allow(clippy::too_many_arguments)]
pub fn field_hinted(key: &str, query: &mut String, r: Rect, shortcuts: &[KeyCode], found: usize, of: usize, draw_idle: bool, hint: &str) -> Reply {
    let reply = handle_keys(key, query, r, shortcuts);
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

/// The keys and the click of a filter in `r` (shared by the boxed line and the search button):
/// starts it, takes the typed text, Enter picks, Esc clears.
fn handle_keys(key: &str, query: &mut String, r: Rect, shortcuts: &[KeyCode]) -> Reply {
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
            } else if esc_pressed() && !query.is_empty() {
                query.clear();
                reply.keys_taken = true;
            }
        } else {
            reply.keys_taken = true;
            let mut ed = focus_edit(key);
            edit_keys(query, &mut ed, &typed_chars(), EditRules::line(60));
            set_focus_edit(key, ed);
            if esc_pressed() {
                query.clear();
                clear_focus();
            } else if enter_pressed() {
                reply.pick = !query.trim().is_empty();
            } else if clicked() && !mouse_in(r.x, r.y, r.w, r.h) {
                clear_focus();
            }
        }
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor::dt::data::ArtefactDef;
    use razdor::rules::content::Content;
    use std::collections::BTreeMap;

    fn item(id: u32, name: &str, kind: ArtefactType) -> ArtefactDef {
        ArtefactDef {
            id,
            name: name.to_string(),
            description: String::new(),
            icon: String::new(),
            cost: 100,
            kind,
            bonus: None,
            magic: None,
            add: Default::default(),
            percent: Default::default(),
            fixed: Default::default(),
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn shields_prefer_a_shield_over_the_bracers_of_lower_id() {
        let c = Content::new(
            Vec::new(),
            vec![
                item(42, "Кожаные наручи", ArtefactType::Shield),
                item(50, "Щит Викинга", ArtefactType::Shield),
                item(51, "Круглый щит", ArtefactType::Shield),
                item(60, "Наручи ЩИТа", ArtefactType::Ring),
                item(7, "Меч", ArtefactType::BlowWeapon),
            ],
            Vec::new(),
            Default::default(),
            razdor::rules::formation::Formation::WIDE,
        );
        // The lowest shield whose name holds «щит» (any case), not the lower id of the bracers.
        assert_eq!(representative(&c, ItemGroup::Shields), Some(ItemId(50)));
    }

    #[test]
    fn shields_without_a_named_one_keep_the_lowest_id() {
        let c = Content::new(
            Vec::new(),
            vec![item(42, "Наручи Кожаные", ArtefactType::Shield), item(43, "Наручи Железные", ArtefactType::Shield)],
            Vec::new(),
            Default::default(),
            razdor::rules::formation::Formation::WIDE,
        );
        assert_eq!(representative(&c, ItemGroup::Shields), Some(ItemId(42)));
    }

    #[test]
    fn other_groups_keep_their_lowest_id_and_weapons_their_blow_weapon() {
        let c = Content::new(
            Vec::new(),
            vec![item(5, "Лук", ArtefactType::ShotWeapon), item(9, "Щит в названии", ArtefactType::Helm), item(8, "Посох", ArtefactType::Staff), item(12, "Меч", ArtefactType::BlowWeapon)],
            Vec::new(),
            Default::default(),
            razdor::rules::formation::Formation::WIDE,
        );
        assert_eq!(representative(&c, ItemGroup::Helms), Some(ItemId(9)));
        assert_eq!(representative(&c, ItemGroup::Weapons), Some(ItemId(12)));
        assert_eq!(representative(&c, ItemGroup::Rings), None);
    }
}
