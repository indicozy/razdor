//! Razdor's own (the owner's decision, 2026-11-01): the phone layout of the army window (and the
//! hero window's shared pieces). Pure geometry in screen px at the window's scale `k` (one design px
//! per dp on a phone, see `kit::phone_window`), so the layouts are testable without a screen.
//!
//! Upright: one column (the unit's band with its full stats, the item filter row, the pack, the hint
//! strip, the army's cards, «Закрыть» at the foot). Sideways: two columns (the band and «Закрыть» on
//! the left; the filter, the pack, the strip and the cards on the right). A unit's cards carry no
//! stat strip on a phone (the band has the full stats); the touch targets are at least 40 dp.

use macroquad::prelude::Rect;

/// Cards are drawn at the original's card proportions (88 × 128 design px).
const CARD_RATIO: f32 = 128.0 / 88.0;

/// The army window's rectangles on a phone at scale `k`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Army {
    /// The selected unit's band (picture and full stats).
    pub band: Rect,
    /// The area of the nine filter buttons (eight groups and the search): one row across the top
    /// sideways, two rows (five and four) upright.
    pub filter: Rect,
    /// The nine buttons, each touch-tall, 8 dp apart (the last one the search).
    pub cells: [Rect; 9],
    /// The pack, or the selected unit's promotion tree (the same place).
    pub pack: Rect,
    /// The hint strip (text only, not a touch target): its height is the caller's (the hint wraps
    /// to it in full on a phone, `items_view::army_hint_phone`).
    pub strip: Rect,
    /// The army's grid of cards: its top left cell and the pitch between cells.
    pub grid: Rect,
    pub pitch: (f32, f32),
    pub card: (f32, f32),
    /// «Закрыть» (touch-tall).
    pub close: Rect,
}

/// The dismiss zone at the foot of the pack's place (the tree shown): its rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dismiss {
    /// The rectangle of the whole zone.
    pub zone: Rect,
    /// The benching switch's button, when the unit can be benched (touch-tall).
    pub toggle: Option<Rect>,
    /// The unit's face (square, left).
    pub face: Rect,
    /// The Dismiss (or Bury) button, right of the face.
    pub button: Rect,
}

/// Touch height in dp.
const TOUCH: f32 = 44.0;
const GAP: f32 = 6.0;
/// The gap between touch targets in dp (a crowded pair is closer than 8 dp to a small target).
const TOUCH_GAP: f32 = 8.0;

/// The nine filter buttons from (`x`, `y`) across `w`, `per_row` to a row, each touch-tall.
fn filter_cells(x: f32, y: f32, w: f32, k: f32, per_row: usize) -> [Rect; 9] {
    let h = TOUCH * k;
    let gap = TOUCH_GAP * k;
    let cell_w = ((w - gap * (per_row as f32 - 1.0)) / per_row as f32).max(1.0);
    std::array::from_fn(|i| {
        let (row, col) = ((i / per_row) as f32, (i % per_row) as f32);
        Rect::new(x + col * (cell_w + gap), y + row * (h + gap), cell_w, h)
    })
}

/// Upright: the army window laid out in one column inside `inner` (the window's inner rectangle),
/// for a formation of `cols` × `lines` cards; the hint strip `strip_h` px tall.
pub fn upright(inner: Rect, k: f32, cols: usize, lines: usize, strip_h: f32) -> Army {
    let pad = 6.0 * k;
    let gap = GAP * k;
    let x = inner.x + pad;
    let w = (inner.w - 2.0 * pad).max(1.0);
    let cols = cols.max(1) as f32;
    let lines = lines.max(1) as f32;
    let pitch_x = w / cols;
    let card_w = (pitch_x - gap).max(1.0);
    let card_h = card_w * CARD_RATIO;
    let pitch_y = card_h + gap;
    let grid_h = lines * card_h + (lines - 1.0) * gap;
    let band_h = (0.26 * inner.h).clamp(170.0 * k, 230.0 * k);
    // The filter: two rows of five and four touch-tall buttons.
    let filter_h = 2.0 * TOUCH * k + TOUCH_GAP * k;
    let close_h = 52.0 * k;
    let fixed = band_h + filter_h + strip_h + grid_h + close_h + 5.0 * gap;
    let pack_h = (inner.h - 2.0 * pad - fixed).max(120.0 * k);
    let mut y = inner.y + pad;
    let band = Rect::new(x, y, w, band_h);
    y += band_h + gap;
    let filter = Rect::new(x, y, w, filter_h);
    let cells = filter_cells(x, y, w, k, 5);
    y += filter_h + gap;
    let pack = Rect::new(x, y, w, pack_h);
    y += pack_h + gap;
    let strip = Rect::new(x, y, w, strip_h);
    y += strip_h + gap;
    let grid = Rect::new(x + (w - cols * pitch_x) / 2.0, y, cols * pitch_x, grid_h);
    y += grid_h + gap;
    let close = Rect::new(x + w * 0.2, y, w * 0.6, close_h);
    Army { band, filter, cells, pack, strip, grid, pitch: (pitch_x, pitch_y), card: (card_w, card_h), close }
}

/// Sideways (the hint strip `strip_h` px tall): the filter row runs across the top; under it two columns. The left column holds the band
/// over «Закрыть»; the right the pack, the strip and the cards.
pub fn sideways(inner: Rect, k: f32, cols: usize, lines: usize, strip_h: f32) -> Army {
    let pad = 6.0 * k;
    let gap = GAP * k;
    let cols = cols.max(1) as f32;
    let lines = lines.max(1) as f32;
    // The filter row sits 16 dp under the title's top, clear of the window's close button (8 dp).
    let top = inner.y + 16.0 * k;
    let bottom = inner.y + inner.h - pad;
    let fx = inner.x + pad;
    let fw = (inner.w - 2.0 * pad).max(1.0);
    let filter_h = TOUCH * k;
    let filter = Rect::new(fx, top, fw, filter_h);
    let cells = filter_cells(fx, top, fw, k, 9);
    let below = filter.bottom() + gap;
    let lx = inner.x + pad;
    let left_w = (inner.w * 0.42).floor();
    let rx = lx + left_w + gap;
    let rw = (inner.x + inner.w - pad - rx).max(1.0);
    let close_h = 52.0 * k;
    // The band takes the left column above «Закрыть» (its stat lines need the height).
    let band = Rect::new(lx, below, left_w, (bottom - below - close_h - gap).max(1.0));
    let close = Rect::new(lx + left_w * 0.2, bottom - close_h, left_w * 0.6, close_h);
    // The cards: about 42% of the right column's height, two lines at most.
    let grid_budget = 0.42 * (bottom - below);
    let card_h = ((grid_budget - (lines - 1.0) * gap) / lines).max(1.0);
    let card_w = card_h / CARD_RATIO;
    let pitch_x = (rw / cols).min(card_w + gap);
    let pitch_y = card_h + gap;
    let grid_h = lines * card_h + (lines - 1.0) * gap;
    let grid_y = bottom - grid_h;
    let strip = Rect::new(rx, grid_y - gap - strip_h, rw, strip_h);
    let pack = Rect::new(rx, below, rw, (strip.y - gap - below).max(1.0));
    let grid = Rect::new(rx + (rw - cols * pitch_x) / 2.0, grid_y, cols * pitch_x, grid_h);
    Army { band, filter, cells, pack, strip, grid, pitch: (pitch_x, pitch_y), card: (card_w, card_h), close }
}

/// The dismiss zone at the foot of `pack` for a unit that can be benched (`bench`) or not: the
/// benching switch above (when `bench`), then the face and the Dismiss button, all touch-tall.
pub fn dismiss(pack: Rect, bench: bool, k: f32) -> Dismiss {
    let h = TOUCH * k;
    let pad = 4.0 * k;
    let zone_h = h + pad * 2.0 + if bench { h + pad } else { 0.0 };
    let zone = Rect::new(pack.x, pack.bottom() - zone_h, pack.w, zone_h);
    let toggle = bench.then(|| Rect::new(zone.x + pad, zone.y + pad, zone.w - 2.0 * pad, h));
    let row_y = zone.y + zone.h - pad - h;
    let face = Rect::new(zone.x + pad, row_y, h, h);
    let button = Rect::new(face.right() + pad, row_y, zone.w - h - 3.0 * pad, h);
    Dismiss { zone, toggle, face, button }
}

/// A diary box's heading, in design px: the row reserved above each box on a phone (its text
/// measured by `kit::Text`, placed above its box, never over the box above).
pub const HEAD: f32 = 22.0;

/// The hero window's rectangles on a phone at scale `k`: the band, the pack, the four resource
/// boxes (2 × 2), the diary column (the description box, the list with its bar, the entry with its
/// bar) and the bottom buttons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hero {
    pub band: Rect,
    pub pack: Rect,
    pub res: [Rect; 4],
    /// The item description box (the top of the diary column; the list and the entry follow it).
    pub desc: Rect,
    /// The diary's list (its tabs are drawn over it) and its scroll bar.
    pub list: Rect,
    pub list_bar: Rect,
    pub entry: Rect,
    pub entry_bar: Rect,
    /// «Весь список», «Закрыть» (upright only) and «Развернуть».
    pub whole: Rect,
    pub close: Option<Rect>,
    pub expand: Rect,
    /// The area the diary column scrolls in (sideways: the right column; upright: the window's inside).
    pub pane: Rect,
    /// The column's natural height in px from the pane's top (sideways; upright it is the pane's).
    pub content: f32,
}

/// Upright: the hero window in one column inside `inner`: the band, the pack, the resources (2 × 2),
/// the diary (description, tabs and list, entry) and the buttons in one row at the foot.
pub fn hero_upright(inner: Rect, k: f32) -> Hero {
    let pad = 6.0 * k;
    let gap = GAP * k;
    let x = inner.x + pad;
    let w = (inner.w - 2.0 * pad).max(1.0);
    let bottom = inner.bottom() - pad;
    let btn_h = 52.0 * k;
    let bottom_y = bottom - btn_h;
    let band_h = (0.22 * inner.h).clamp(150.0 * k, 180.0 * k);
    let res_h = 46.0 * k;
    let res_grid = 2.0 * res_h + gap;
    let desc_h = 52.0 * k;
    let tabs_h = 40.0 * k;
    let list_h = 100.0 * k;
    let entry_min = 80.0 * k;
    // Each diary box has its heading in a row of its own above it (`HEAD`), measured not overlapped.
    let others = band_h + res_grid + desc_h + tabs_h + list_h + btn_h + 6.0 * gap + 2.0 * HEAD * k;
    let pack_h = (inner.h - 2.0 * pad - others - entry_min).clamp(100.0 * k, 200.0 * k);
    let mut y = inner.y + pad;
    let band = Rect::new(x, y, w, band_h);
    y += band_h + gap;
    let pack = Rect::new(x, y, w, pack_h);
    y += pack_h + gap;
    let res = res_boxes(x, y, w, res_h, gap);
    y += res_grid + gap;
    y += HEAD * k;
    let desc = Rect::new(x, y, w, desc_h);
    y += desc_h + gap;
    // The tabs are drawn over the list's top (`tabs_h` above it): the list starts under them.
    y += tabs_h;
    let list = Rect::new(x, y, w - 18.0 * k, list_h);
    let list_bar = Rect::new(list.right(), list.y, 18.0 * k, list_h);
    y += list_h + gap + HEAD * k;
    let entry_h = (bottom_y - gap - y).max(1.0);
    let entry = Rect::new(x, y, w - 18.0 * k, entry_h);
    let entry_bar = Rect::new(entry.right(), entry.y, 18.0 * k, entry_h);
    let third = (w - 2.0 * gap) / 3.0;
    let whole = Rect::new(x, bottom_y, third, btn_h);
    let close = Rect::new(x + third + gap, bottom_y, third, btn_h);
    let expand = Rect::new(x + 2.0 * (third + gap), bottom_y, third, btn_h);
    Hero { band, pack, res, desc, list, list_bar, entry, entry_bar, whole, close: Some(close), expand, pane: inner, content: 0.0 }
}

/// Sideways: the hero window in two columns: the band over the pack on the left; the resources, the
/// diary (tabs, list, entry) and the buttons on the right, which scroll by `scroll` px when they do
/// not fit (`pane` is the right column; `content` its height). The list keeps three touch-tall rows;
/// the entry takes the rest, at least 70 dp. The artifact description takes the entry's place only
/// while an item is described (`desc`), so the pane has no box of its own for it.
pub fn hero_sideways(inner: Rect, k: f32, scroll: f32) -> Hero {
    let pad = 6.0 * k;
    let gap = GAP * k;
    let lw = (inner.w * 0.44).floor();
    let lx = inner.x + pad;
    let rx = lx + lw + gap;
    let rw = (inner.right() - pad - rx).max(1.0);
    let top = inner.y + pad;
    let bottom = inner.bottom() - pad;
    let band_h = ((bottom - top - gap) * 0.5).max(1.0);
    let band = Rect::new(lx, top, lw, band_h);
    let pack = Rect::new(lx, band.bottom() + gap, lw, (bottom - band.bottom() - gap).max(1.0));
    let pane = Rect::new(rx, top, rw, (bottom - top).max(1.0));
    // The natural layout (no scroll): the resources, the tabs over the list, the list (three
    // touch-tall rows, the list's bar 12 dp short of the pane's own bar), the entry, the buttons.
    let res_h = 46.0 * k;
    let res_top = top;
    let tabs_h = 34.0 * k;
    let list_top = res_top + 2.0 * res_h + 2.0 * gap + tabs_h;
    let list_h = 3.0 * TOUCH * k + 8.0 * k;
    let btn_h = 40.0 * k;
    // The entry fills the pane when it can, else it is 70 dp and the pane scrolls.
    let fill_bottom = bottom - btn_h - gap;
    let entry_h = (fill_bottom - (list_top + list_h + gap + HEAD * k)).max(70.0 * k);
    let entry_top = list_top + list_h + gap + HEAD * k;
    let btn_top = entry_top + entry_h + gap;
    let content = btn_top + btn_h - pane.y + pad;
    let dy = -scroll;
    // The resources and the buttons stop 12 dp short of the pane's right edge, where its bar is.
    let bw = (rw - 12.0 * k).max(1.0);
    let res = res_boxes(rx, res_top + dy, bw, res_h, gap);
    let list = Rect::new(rx, list_top + dy, rw - 30.0 * k, list_h);
    let list_bar = Rect::new(list.right(), list.y, 18.0 * k, list_h);
    let entry = Rect::new(rx, entry_top + dy, rw - 30.0 * k, entry_h);
    let entry_bar = Rect::new(entry.right(), entry.y, 18.0 * k, entry_h);
    let half = (bw - gap) / 2.0;
    let whole = Rect::new(rx, btn_top + dy, half, btn_h);
    let expand = Rect::new(rx + half + gap, btn_top + dy, half, btn_h);
    Hero { band, pack, res, desc: entry, list, list_bar, entry, entry_bar, whole, close: None, expand, pane, content }
}

/// Four resource boxes in two rows of two, `h` tall, `gap` apart, from (`x`, `y`) over `w`.
fn res_boxes(x: f32, y: f32, w: f32, h: f32, gap: f32) -> [Rect; 4] {
    let half = (w - gap) / 2.0;
    [0, 1, 2, 3].map(|i| {
        let (col, row) = ((i % 2) as f32, (i / 2) as f32);
        Rect::new(x + col * (half + gap), y + row * (h + gap), half, h)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hint strip three lines of the floor size tall at 2.75 px per dp (`items_view::HINT_LINES`).
    const STRIP: f32 = 60.0 * 2.75;

    fn inner_upright() -> Rect {
        // A 1080x2400 phone at 2.75 px per dp: the window about 1064 px wide and 2200 px tall.
        Rect::new(8.0, 40.0, 1064.0, 2200.0)
    }

    #[test]
    fn upright_targets_are_touch_sized_and_inside_the_window() {
        let k = 2.75;
        let inner = inner_upright();
        let a = upright(inner, k, 6, 2, STRIP);
        assert!(a.filter.h >= 40.0 * k && a.close.h >= 40.0 * k);
        for r in [a.band, a.filter, a.pack, a.strip, a.grid, a.close] {
            assert!(r.x >= inner.x - 0.5 && r.right() <= inner.right() + 0.5, "{r:?}");
            assert!(r.y >= inner.y - 0.5 && r.bottom() <= inner.bottom() + 0.5, "{r:?}");
        }
        assert!(a.pack.h >= 120.0 * k, "the pack keeps at least a few rows: {:?}", a.pack);
    }

    #[test]
    fn upright_cards_are_touch_sized_for_six_columns() {
        let k = 2.75;
        let a = upright(inner_upright(), k, 6, 2, STRIP);
        assert!(a.card.0 >= 40.0 * k, "a card is at least 40 dp wide: {:?}", a.card);
        let x = a.grid.x + 5.0 * a.pitch.0;
        assert!(x + a.card.0 <= a.grid.right() + 0.5);
    }

    #[test]
    fn upright_rows_do_not_overlap() {
        let a = upright(inner_upright(), 2.75, 6, 2, STRIP);
        let rows = [a.band, a.filter, a.pack, a.strip, a.grid, a.close];
        for (i, p) in rows.iter().enumerate() {
            for q in rows.iter().skip(i + 1) {
                let overlap = p.x < q.right() && q.x < p.right() && p.y < q.bottom() && q.y < p.bottom();
                assert!(!overlap, "{p:?} overlaps {q:?}");
            }
        }
    }

    #[test]
    fn sideways_keeps_both_columns_in_the_window() {
        let k = 2.75;
        let inner = Rect::new(8.0, 8.0, 2384.0, 1000.0);
        let a = sideways(inner, k, 6, 2, STRIP);
        for r in [a.band, a.filter, a.pack, a.strip, a.grid, a.close] {
            assert!(r.x >= inner.x - 0.5 && r.right() <= inner.right() + 0.5, "{r:?}");
            assert!(r.y >= inner.y - 0.5 && r.bottom() <= inner.bottom() + 0.5, "{r:?}");
        }
        assert!(a.band.right() <= a.pack.x, "the band is left of the pack column");
        assert!(a.close.h >= 40.0 * k && a.filter.h >= 40.0 * k);
    }

    #[test]
    fn hero_upright_stacks_its_parts_inside_the_window() {
        let k = 2.75;
        // A small phone's window (1080 x 2400 px at 2.75 px per dp): about 2200 px inside.
        let inner = Rect::new(8.0, 120.0, 1064.0, 2200.0);
        let h = hero_upright(inner, k);
        let rows = [h.band, h.pack, h.res[0], h.res[2], h.desc, h.list, h.entry, h.whole, h.expand];
        for r in rows {
            assert!(r.x >= inner.x - 0.5 && r.right() <= inner.right() + 0.5, "{r:?}");
            assert!(r.y >= inner.y - 0.5 && r.bottom() <= inner.bottom() + 0.5, "{r:?}");
        }
        assert!(h.whole.h >= 40.0 * k && h.expand.h >= 40.0 * k && h.close.unwrap().h >= 40.0 * k);
        assert!(h.entry.h >= 80.0 * k, "the entry keeps room for its text: {:?}", h.entry);
        assert!(h.desc.bottom() <= h.list.y, "the list starts under the description: {h:?}");
    }

    #[test]
    fn hero_sideways_has_two_columns_and_touch_buttons() {
        let k = 2.75;
        let inner = Rect::new(8.0, 8.0, 2384.0, 1000.0);
        let h = hero_sideways(inner, k, 0.0);
        assert!(h.band.right() <= h.res[0].x && h.pack.right() <= h.res[0].x, "the left column is left of the right one");
        assert!(h.whole.h >= 40.0 * k && h.expand.h >= 40.0 * k);
        assert!(h.close.is_none());
        assert!(h.entry.h > 0.0 && h.list.h > 0.0);
    }

    #[test]
    fn dismiss_keeps_touch_rows_inside_the_pack() {
        let pack = Rect::new(0.0, 0.0, 300.0, 300.0);
        let d = dismiss(pack, true, 2.75);
        assert!(d.toggle.is_some());
        assert!(d.face.h >= 40.0 * 2.75 && d.button.h >= 40.0 * 2.75);
        assert!(d.zone.y >= pack.y && d.zone.bottom() <= pack.bottom() + 0.5);
        let plain = dismiss(pack, false, 2.75);
        assert!(plain.toggle.is_none());
        assert!(plain.button.bottom() <= pack.bottom() + 0.5);
    }

    /// Every pair of filter buttons in a row is at least 8 dp apart, and each is touch-tall.
    fn check_filter(cells: [Rect; 9], k: f32) {
        for (i, a) in cells.iter().enumerate() {
            assert!(a.h >= 40.0 * k && a.w >= 40.0 * k, "a filter button is touch-sized: {a:?}");
            for b in cells.iter().skip(i + 1) {
                let same_row = (a.y - b.y).abs() < 0.5;
                if same_row {
                    let gap = if a.x < b.x { b.x - a.right() } else { a.x - b.right() };
                    assert!(gap >= 8.0 * k - 0.5, "{a:?} and {b:?} are {gap} px apart");
                }
            }
        }
    }

    #[test]
    fn filter_buttons_are_touch_sized_and_apart() {
        let k = 2.75;
        check_filter(upright(inner_upright(), k, 6, 2, STRIP).cells, k);
        check_filter(sideways(Rect::new(8.0, 8.0, 2384.0, 1000.0), k, 6, 2, STRIP).cells, k);
    }

    #[test]
    fn sideways_filter_clears_the_title() {
        let k = 2.75;
        let inner = Rect::new(8.0, 8.0, 2384.0, 1000.0);
        let a = sideways(inner, k, 6, 2, STRIP);
        assert!(a.filter.y >= inner.y + 8.0 * k, "the filter row sits below the title's close button");
    }

    #[test]
    fn hero_sideways_list_keeps_three_touch_rows_and_scrolls() {
        let k = 2.75;
        let inner = Rect::new(8.0, 8.0, 2384.0, 1000.0);
        let h = hero_sideways(inner, k, 0.0);
        assert!(h.list.h >= 3.0 * 44.0 * k, "three touch-tall rows: {:?}", h.list);
        assert!(h.entry.h >= 70.0 * k - 0.5);
        assert!(h.res[0].y >= h.pane.y - 0.5 && h.expand.bottom() >= h.pane.y);
        let moved = hero_sideways(inner, k, 40.0);
        assert!((h.res[0].y - moved.res[0].y - 40.0).abs() < 0.01, "the column moves up by the scroll");
        assert_eq!(h.content, moved.content, "the content height does not depend on the scroll");
        assert!(h.band.right() <= h.pane.x, "the left column is left of the pane");
    }
}
