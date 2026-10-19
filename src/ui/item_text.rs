//! An item's description box, as the original lays its item text (interface.md §9.2,
//! 0x499eb0) in a text list: every line centred and word-wrapped, top to bottom the name in
//! the white Benguiat (0xae2498); for an item with a description the usage line in grey
//! (0xae24d0), an empty line and the description in yellow (0xae24c0); the effect text as
//! one paragraph in orange (0xae24c4); the bonus line in white (0xae24bc). The texts come from
//! [`razdor::rules::item_text`].
//!
//! Where it stands differs by window:
//! - the market (0x4b9c40): the item's 55 px picture centred 8 px below the box's top, the
//!   text centred in the rest below 63 px (top margin `63 + (H − text) / 2`);
//! - the army and hero windows' description box (0x4c280c): no picture, the text centred
//!   (top margin `(H − text) / 2`, at least 4);
//! - the event window's item tooltip (0x4cbab0): 272 px wide, 16 px margin, as tall as its
//!   text plus 32. Razdor's army window pops the same box up by the pointer.

use macroquad::prelude::*;

use razdor::dt::markup::{Align, Ink};
use razdor::rules::content::ItemId;
use razdor::rules::game::Game;
use razdor::rules::item_text;

use super::assets::Assets;
use super::chrome;
use super::dt_font::{sheet_size, with_original_face, Face};
use super::widgets::{draw_markup_row_face, measure, MarkupRow};

/// How the box places the text (and the picture).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemBox {
    /// The market's description (0x4b9c40): the picture over the text.
    Market,
    /// The hero window's description box (0x4c280c), framed without corners (0x4d18c5).
    Inventory,
    /// The event window's tooltip (0x4cbab0), also Razdor's pop-up in the army window.
    Tooltip,
}

/// The tooltip's width and margin, in the original's pixels (0x4cbab0).
pub const TOOLTIP_W: f32 = 272.0;
const TOOLTIP_MARGIN: f32 = 16.0;
/// The market's picture: 55 px, 8 px below the box's top; the text below 63 (0x4b9da9).
const PICTURE: f32 = 55.0;
const PICTURE_TOP: f32 = 8.0;
/// The side margin of the market and inventory lists (Razdor's: the tooltip's 16 px; the
/// original's own value for those two lists is not read out).
const SIDE: f32 = 16.0;

/// The fonts' colours: the SanSerif tints of 4dbc26 (white less a delta per channel).
const WHITE_TEXT: Color = Color::new(1.0, 1.0, 1.0, 1.0);
/// 0xae24c0: less 25, 25, 125.
const YELLOW: Color = Color::new(230.0 / 255.0, 230.0 / 255.0, 130.0 / 255.0, 1.0);
/// 0xae24c4: less 20, 90, 180.
const ORANGE: Color = Color::new(235.0 / 255.0, 165.0 / 255.0, 75.0 / 255.0, 1.0);
/// 0xae24d0: less 100 each.
const GREY: Color = Color::new(155.0 / 255.0, 155.0 / 255.0, 155.0 / 255.0, 1.0);

/// A `[Skills]` label of the install in Russian, else ours ([`super::spell_badges::label`]).
fn label(key: &'static str) -> String {
    super::spell_badges::label(key)
}

/// One wrapped line with its font.
struct Line {
    row: MarkupRow,
    face: Face,
    size: f32,
    color: Color,
}

/// The item's lines at `s` screen pixels per original pixel, wrapped to `room`.
fn lines(game: &Game, item: ItemId, room: f32, s: f32) -> Vec<Line> {
    let d = game.content.item(item);
    let (title, body) = (sheet_size(Face::Subtitle, s), sheet_size(Face::Body, s));
    let mut out = Vec::new();
    let mut add = |text: &str, face: Face, size: f32, color: Color| {
        with_original_face(face, || {
            let width = |c: char| measure(c.encode_utf8(&mut [0; 4]), size).width;
            for r in razdor::dt::markup::wrap(text, room, width) {
                out.push(Line { row: MarkupRow { text: r.text, ink: Ink::Plain, align: Align::Centre, last: r.last }, face, size, color });
            }
        });
    };
    add(&d.name, Face::Subtitle, title, WHITE_TEXT);
    if !d.description.is_empty() {
        if let Some(key) = item_text::usage_key(d) {
            add(&label(key), Face::Body, body, GREY);
        }
        // An empty line in the description's font, then the description.
        add("", Face::Body, body, YELLOW);
        add(&d.description, Face::Body, body, YELLOW);
    }
    let effects = item_text::effect_text(d, &label);
    // An empty effect text adds no line (0x49a59d).
    if !effects.is_empty() {
        add(effects.trim_end(), Face::Body, body, ORANGE);
    }
    if let Some(b) = &d.bonus {
        let name = super::unit_sheet::bonus_title(b);
        add(&item_text::bonus_line(&name, &label), Face::Body, body, WHITE_TEXT);
    }
    out
}

/// A line's advance: the face's line at its size (the list's rows are as tall as their font).
fn advance(l: &Line) -> f32 {
    (l.size * 1.15).round()
}

fn height(lines: &[Line]) -> f32 {
    lines.iter().map(advance).sum()
}

/// The tooltip's height for `item` at `s` screen pixels per original pixel: its text plus
/// twice the margin (0x4cbb15).
pub fn tooltip_height(game: &Game, item: ItemId, s: f32) -> f32 {
    height(&lines(game, item, (TOOLTIP_W - 2.0 * TOOLTIP_MARGIN) * s, s)) + 2.0 * TOOLTIP_MARGIN * s
}

/// Draws `item`'s description box in `r`, laid out as `kind`, at `s` screen pixels per
/// original pixel. A text taller than its room is drawn smaller until it fits (Razdor's: the
/// original's list cuts it).
pub fn draw(game: &Game, assets: &Assets, item: ItemId, r: Rect, kind: ItemBox, s: f32) {
    if kind == ItemBox::Inventory {
        chrome::text_box_no_corners(r);
    } else {
        chrome::text_box(r);
    }
    let side = if kind == ItemBox::Tooltip { TOOLTIP_MARGIN } else { SIDE };
    let above = if kind == ItemBox::Market { PICTURE_TOP + PICTURE } else { 0.0 };
    let room_h = |s: f32| r.h - above * s - if kind == ItemBox::Tooltip { 2.0 * TOOLTIP_MARGIN * s } else { 0.0 };
    let mut scale = s;
    let mut text = lines(game, item, r.w - 2.0 * side * scale, scale);
    while height(&text) > room_h(scale) && scale > s * 0.6 {
        scale *= 0.9;
        text = lines(game, item, r.w - 2.0 * side * scale, scale);
    }
    let h = height(&text);
    let top = match kind {
        // 0x4b9d97: 63 + (H − text − 55 − 8) / 2.
        ItemBox::Market => {
            let p = PICTURE * scale;
            assets.draw_item(item, r.x + ((r.w - p) / 2.0).floor(), r.y + PICTURE_TOP * scale, p);
            above * scale + ((r.h - h - above * scale) / 2.0).floor()
        }
        // 0x49a671: (H − text) / 2, at least 4.
        ItemBox::Inventory => ((r.h - h) / 2.0).floor().max(4.0 * scale),
        // 0x4cbb15: 16.
        ItemBox::Tooltip => TOOLTIP_MARGIN * scale,
    };
    let room = r.w - 2.0 * side * scale;
    let mut y = r.y + top;
    for l in &text {
        draw_markup_row_face(&l.row, r.x + side * scale, y + l.size * 0.85, room, l.size, l.face, Some(l.color));
        y += advance(l);
    }
}
