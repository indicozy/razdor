//! The «Итоги» page (Razdor's, the chronicle's summary; `rules::chronicle::Summary`): the map as
//! the hero explored it, cropped to the land he knows, his route on it (cool at the start, warm at
//! the end), the battles and captures marked where they happened, the highlights and the totals. It
//! opens after a map ends (a victory, a defeat or the hero's fall, unless the setting «Итоги после
//! карты» is off), before the next screen, and from the chronicle tab («Летопись», the whole story
//! so far). It is a window in the game's own frame, centred; on a phone it fills the screen.

use macroquad::prelude::*;

use razdor::dt::markup::{Align, Ink};
use razdor::i18n::tr;
use razdor::rules::chronicle::{Mark, Summary};
use razdor::rules::game::Game;
use razdor::rules::map::Tile;

use super::assets::Assets;
use super::chrome;
use super::display;
use super::dt_font::{with_original_face, Face};
use super::dt_font::sheet_size;
use super::items_view::{diary_reader_body_size, diary_scale, HeroView};
use super::minimap;
use super::widgets::*;
use super::window_layout;
use super::Screen;

/// Where the page goes when it closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    /// Back to the chronicle tab (the summary was asked for from it).
    Diary(HeroView),
    /// A campaign map's victory: the next map (or the main menu) follows, as it would have.
    Victory,
    /// The hero fell: the main menu.
    Defeat,
}

/// The page on screen: what it shows, where it goes, and the map's cells it crops to (the land
/// explored, see [`explored_box`]).
#[derive(Debug)]
pub struct View {
    pub after: After,
    pub summary: Summary,
    pub cells: (i32, i32, i32, i32),
}

/// What a frame of the page leaves to the app.
pub enum Exit {
    Back(Box<Screen>),
    Victory,
    Defeat,
    /// «Смотреть запись»: the game's record, back to this page.
    Watch,
}

/// The page for `after`: a map's summary (after a map, the chapter just played) or the story so
/// far (from the chronicle tab). None when the record holds no chapter. `died`: the hero fell.
pub fn open(game: &Game, after: After, died: bool) -> Option<Screen> {
    let summary = game.chronicle.summary(from_chapter(game, after), game.clock.total_minutes() as u64, died)?;
    minimap::snap_fog();
    let map = &game.world.map;
    let cells = explored_box(map.w, map.h, |t| game.fog.explored(t), 3);
    Some(Screen::Summary(Box::new(View { after, summary, cells })))
}

/// The first chapter the totals cover: the story so far from the chronicle tab, the map just played
/// after a map ends.
fn from_chapter(game: &Game, after: After) -> usize {
    match after {
        After::Diary(_) => 0,
        After::Victory | After::Defeat => game.chronicle.chapters.len().saturating_sub(1),
    }
}

/// The box of the explored cells of a `w` × `h` map, `margin` cells wider on each side and kept
/// inside the map (x0, y0, x1, y1, end exclusive). With no explored cell, the whole map.
pub fn explored_box(w: i32, h: i32, explored: impl Fn(Tile) -> bool, margin: i32) -> (i32, i32, i32, i32) {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for y in 0..h {
        for x in 0..w {
            if explored((x, y)) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x0 > x1 {
        return (0, 0, w, h);
    }
    ((x0 - margin).max(0), (y0 - margin).max(0), (x1 + 1 + margin).min(w), (y1 + 1 + margin).min(h))
}

/// The window's box on a screen `sw` × `sh`: centred in the 4:3 frame (800 × 600 at the interface
/// scale, so the page's text fits at the diary's size), the screen less a margin when it is smaller;
/// a phone's is the whole screen less its safe area.
pub fn window_rect(sw: f32, sh: f32, phone: bool, k: f32, safe: display::SafeArea) -> Rect {
    if phone {
        let m = 6.0 * display::dp_px().unwrap_or(1.0);
        return Rect::new(safe.left + m, safe.top + m, (sw - safe.left - safe.right - 2.0 * m).max(0.0), (sh - safe.top - safe.bottom - 2.0 * m).max(0.0));
    }
    let w = (800.0 * k).min(sw - 16.0).max(0.0);
    let h = (600.0 * k).min(sh - 16.0).max(0.0);
    Rect::new((sw - w) / 2.0, (sh - h) / 2.0, w, h)
}

/// The page's parts inside the window's inner box: the map's pane (a square's box, the picture
/// letterboxed in it), the highlights and the totals table, and the button at the foot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Page {
    pub pane: Rect,
    pub highlights: Rect,
    pub totals: Rect,
    pub button: Rect,
}

/// The page's layout in the inner box `inner` at a pad `pad`. The button row is `button_h` tall at
/// the foot. `reserve`: the heights the highlights and the totals need (the totals' table is always
/// the foot of the text column; upright, the highlights' need takes its room under the map).
pub fn layout(inner: Rect, upright: bool, pad: f32, button_h: f32, reserve: (f32, f32)) -> Page {
    let (hl_h, tot_h) = reserve;
    let button = Rect::new(inner.center().x - 80.0 * pad / 8.0, inner.bottom() - pad - button_h, 160.0 * pad / 8.0, button_h);
    let body_top = inner.y + pad;
    let body_bottom = button.y - pad;
    let avail = (body_bottom - body_top).max(0.0);
    if upright {
        let room = (avail - hl_h - tot_h - 2.0 * pad).max(avail * 0.3);
        let side = (inner.w - 2.0 * pad).min(room).max(0.0);
        let pane = Rect::new(inner.center().x - side / 2.0, body_top, side, side);
        let totals = Rect::new(inner.x + pad, body_bottom - tot_h, inner.w - 2.0 * pad, tot_h);
        let highlights = Rect::new(inner.x + pad, pane.bottom() + pad, inner.w - 2.0 * pad, (totals.y - pad - (pane.bottom() + pad)).max(0.0));
        Page { pane, highlights, totals, button }
    } else {
        // Sideways the map and the totals share the left column (the map square at its top, the
        // table under it); the highlights take the whole right column.
        let th = reserve.1;
        let ph = (avail - th - pad).max(0.0);
        let pane = Rect::new(inner.x + pad, body_top, ph, ph);
        let totals = Rect::new(inner.x + pad, pane.bottom() + pad, pane.w.max(inner.w * 0.36), th);
        let x = totals.right() + pad;
        let col = (inner.right() - pad - x).max(0.0);
        let highlights = Rect::new(x, body_top, col, avail);
        Page { pane, highlights, totals, button }
    }
}

/// The largest square-proportioned box of `w`:`h` (the picture's aspect) fitting inside `pane`,
/// centred.
pub fn letterbox(pane: Rect, aspect_w: f32, aspect_h: f32) -> Rect {
    if pane.w <= 0.0 || pane.h <= 0.0 || aspect_w <= 0.0 || aspect_h <= 0.0 {
        return pane;
    }
    let a = aspect_w / aspect_h;
    let (w, h) = if pane.w / pane.h > a { (pane.h * a, pane.h) } else { (pane.w, pane.w / a) };
    Rect::new(pane.x + (pane.w - w) / 2.0, pane.y + (pane.h - h) / 2.0, w, h)
}

/// The first of `sizes` (largest first) at which `fits` holds; the smallest when none does.
pub fn fit_size(sizes: &[f32], fits: impl Fn(f32) -> bool) -> f32 {
    sizes.iter().copied().find(|&s| fits(s)).unwrap_or_else(|| sizes.last().copied().unwrap_or(0.0))
}

/// The route's colour at its `i`-th of `n` cells: cool blue at the start, warm orange at the end.
pub fn route_colour(i: usize, n: usize) -> Color {
    let t = if n <= 1 { 0.0 } else { i as f32 / (n - 1) as f32 };
    let (cool, warm) = ([70.0, 160.0, 255.0], [255.0, 110.0, 40.0]);
    let c = |j: usize| (cool[j] + (warm[j] - cool[j]) * t) / 255.0;
    Color::new(c(0), c(1), c(2), 1.0)
}

/// The totals' rows: label and value, in the order the page lists them.
fn total_rows(s: &Summary) -> [(&'static str, u32); 8] {
    let t = &s.totals;
    [
        (tr("Days"), t.days as u32),
        (tr("Battles won"), t.battles_won),
        (tr("Battles lost"), t.battles_lost),
        (tr("Our losses"), t.own_losses),
        (tr("Their losses"), t.foe_losses),
        (tr("Places taken"), t.towns_taken),
        (tr("Artefacts got"), t.artefacts),
        (tr("Quests done"), t.quests_done),
    ]
}

/// Whether the totals' two columns hold their label and number at the body size `s` in width `w`
/// (the column's gap a third of the size).
fn totals_fit(rows: &[(&str, u32)], w: f32, s: f32, face: Face) -> bool {
    let col_w = w / 2.0;
    with_original_face(face, || {
        rows.iter().all(|(label, value)| measure_exact(label, s).width + measure_exact(&value.to_string(), s).width + s / 3.0 <= col_w)
    })
}

/// The height the totals' table needs at the body size `s`: its heading (at the title size `h`,
/// as drawn) and four rows.
fn totals_need(s: f32, h: f32) -> f32 {
    h * 1.8 + 4.0 * s * 1.5
}

/// The line step of the body text at size `s`: the diary entry's (1.15 of the size, whole pixels).
fn line_step(s: f32) -> f32 {
    (s * 1.15).round()
}

/// One sentence's rows at width `w` in `face` at size `s`: wrapped as the diary's entry text is
/// (`markup::wrap`), left aligned (`last`: a justified row would spread its words).
fn body_rows(sentence: &str, w: f32, s: f32, face: Face) -> Vec<MarkupRow> {
    with_original_face(face, || {
        let width = |c: char| measure(c.encode_utf8(&mut [0; 4]), s).width;
        razdor::dt::markup::wrap(sentence, w, width).into_iter().map(|r| MarkupRow { text: r.text, ink: Ink::Plain, align: Align::Justify, last: true }).collect()
    })
}

/// The height the highlights need at size `s` in width `w`: their heading (at the title size `h`),
/// their lines and the gaps between them.
fn highlights_need(sentences: &[String], w: f32, s: f32, h: f32, face: Face) -> f32 {
    let lines: usize = sentences.iter().map(|t| body_rows(t, w, s, face).len()).sum();
    h * 1.8 + lines as f32 * line_step(s) + sentences.len() as f32 * s * 0.6
}

/// Draws the page and takes its input: its button, Enter, Esc or the window's cross leave it.
/// Returns where it goes.
pub fn frame(view: &mut View, game: &Game, assets: &Assets) -> Option<Exit> {
    let dp = display::dp_px();
    let phone = dp.is_some();
    let _scale = display::enter_window_scale(dp);
    clear_background(Color::from_rgba(20, 18, 16, 255));
    let k = chrome::k();
    let (sw, sh) = (screen_width(), screen_height());
    let win = window_rect(sw, sh, phone, k, display::safe_area());
    let title = tr("Summary");
    let (inner, closed) = chrome::window(win, title, chrome::Skin::Marble, true);

    // The text: the body size of the diary's own entries (`diary_body_size`, the desktop's and the
    // phone's), the title of the map on top.
    // On the desktop the page's text base is the hero window's (`diary_scale`), so its text is the
    // diary entry's at each resolution: the chronicle's entries are drawn in the message face at
    // the entry's title size (`diary_title_size`), and so is the page's text.
    let ts = if phone { display::text_for(k, crate::ui::dialog::TEXT_REF) } else { display::text_for(diary_scale(k), crate::ui::dialog::TEXT_REF) };
    // The phone's full-size body (the diary's reader, 100 % of the message size: the phone's list
    // is 0.8 of it, too small for a page that is read whole).
    let body = if phone { diary_reader_body_size(ts) } else { sheet_size(Face::Title, ts) };
    // The body face: the message face, whose native size is the full one (the bitmap body face
    // draws a phone's text at about 13 sp; the diary's entries' text is drawn in it too).
    let face = Face::Subtitle;
    let heading = sheet_size(Face::Title, ts);
    let pad = 8.0 * k;
    let button_h = 36.0 * k * display::button_grow();
    let upright = phone && window_layout::portrait();
    let highlights = razdor::rules::chronicle_text::highlight_lines(game, &view.summary);
    let rows = total_rows(&view.summary);
    // Upright, the highlights' need is reserved under the map before the map takes the rest.
    let hl_reserve = if upright { highlights_need(&highlights, inner.w - 2.0 * pad, body, heading, face) } else { 0.0 };
    let page = layout(inner, upright, pad, button_h, (hl_reserve, totals_need(body, heading)));
    let size = fit_size(&[body, body * 0.92, body * 0.85], |s| {
        highlights_need(&highlights, page.highlights.w, s, heading, face) <= page.highlights.h && totals_need(s, heading) <= page.totals.h && totals_fit(&rows, page.totals.w, s, face)
    });

    // The map: the land explored, cropped (`view.cells`), letterboxed in the pane on the inset.
    let (x0, y0, x1, y1) = view.cells;
    let (cw, ch) = ((x1 - x0).max(1) as f32, (y1 - y0).max(1) as f32);
    let pic = letterbox(page.pane, cw, ch);
    chrome::text_box_base(page.pane);
    draw_map(game, assets, &view.summary, view.cells, pic, k);
    chrome::silver_frame(page.pane, k.max(1.0));

    // The highlights under a heading, then the totals table under its own.
    let hl = page.highlights;
    with_original_face(Face::Title, || chrome::shadow_text(tr("Highlights"), hl.x, hl.y + heading, heading, ACCENT));
    draw_wrapped(&highlights, hl.x, hl.y + heading * 1.8, hl.w, size, face);
    let tot = page.totals;
    with_original_face(Face::Title, || chrome::shadow_text(tr("Totals"), tot.x, tot.y + heading, heading, ACCENT));
    draw_totals(&rows, tot.x, tot.y + heading * 1.8, tot.w, size, face);

    let label = match view.after {
        After::Diary(_) => tr("Close"),
        After::Victory | After::Defeat => tr("Next"),
    };
    // A game that keeps its record offers the record beside the page's button.
    let b = page.button;
    let (watch_r, b) = if game.replay.is_some() {
        let g = 8.0 * k;
        let w = (b.w - g) / 2.0;
        (Some(Rect::new(page.button.x, page.button.y, w, page.button.h)), Rect::new(page.button.x + w + g, page.button.y, w, page.button.h))
    } else {
        (None, b)
    };
    if watch_r.is_some_and(|r| button(r.x, r.y, r.w, r.h, tr("Watch the record"), true)) {
        return Some(Exit::Watch);
    }
    let pressed = button(b.x, b.y, b.w, b.h, label, true);
    if !(pressed || closed || key(KeyCode::Enter) || key(KeyCode::Escape)) {
        return None;
    }
    Some(match view.after {
        After::Diary(hero) => Exit::Back(Box::new(Screen::Hero(hero))),
        After::Victory => Exit::Victory,
        After::Defeat => Exit::Defeat,
    })
}

/// The sentences as paragraphs of the body face at size `s`, one line each, from `y` down.
fn draw_wrapped(sentences: &[String], x: f32, y: f32, w: f32, s: f32, face: Face) {
    let mut y = y;
    for sentence in sentences {
        for row in body_rows(sentence, w, s, face) {
            draw_markup_row_face(&row, x, y + s * 0.85, w, s, face, Some(INK));
            y += line_step(s);
        }
        y += s * 0.6;
    }
}

/// The totals as a two-column table on the text grid: each row's label at the left of its column,
/// its number at the column's right, both in the body size.
fn draw_totals(rows: &[(&str, u32)], x: f32, y: f32, w: f32, s: f32, face: Face) {
    let per_column = rows.len().div_ceil(2);
    let col_w = w / 2.0;
    for (i, (label, value)) in rows.iter().enumerate() {
        let (c, n) = (i / per_column, i % per_column);
        let cx = x + c as f32 * col_w;
        let cy = y + n as f32 * s * 1.5 + s * 0.85;
        let v = value.to_string();
        let vw = with_original_face(face, || measure(&v, s).width);
        let label = MarkupRow { text: label.to_string(), ink: Ink::Plain, align: Align::Justify, last: true };
        draw_markup_row_face(&label, cx, cy, col_w, s, face, Some(DIM));
        // The number sits a gap (a tenth of the column) short of the column's right edge.
        let number = MarkupRow { text: v, ink: Ink::Plain, align: Align::Justify, last: true };
        draw_markup_row_face(&number, cx + col_w - vw - col_w * 0.1, cy, col_w, s, face, Some(INK));
    }
}

/// The map's cropped picture in `pic` (the letterboxed box), the route and the marks over it. A cell
/// outside the crop is not drawn.
fn draw_map(game: &Game, assets: &Assets, s: &Summary, cells: (i32, i32, i32, i32), pic: Rect, k: f32) {
    if pic.w <= 0.0 || pic.h <= 0.0 {
        return;
    }
    let (x0, y0, x1, y1) = cells;
    let (cw, ch) = ((x1 - x0).max(1) as f32, (y1 - y0).max(1) as f32);
    draw_rectangle(pic.x, pic.y, pic.w, pic.h, BLACK);
    minimap::picture(game, assets.dt.as_ref(), super::world_view::surface_color, pic, cells);
    let (kx, ky) = (pic.w / cw, pic.h / ch);
    let at = |t: Tile| vec2(pic.x + (t.0 - x0) as f32 * kx + kx / 2.0, pic.y + (t.1 - y0) as f32 * ky + ky / 2.0);
    let inside = |t: Tile| t.0 >= x0 && t.1 >= y0 && t.0 < x1 && t.1 < y1;
    let n = s.route.len();
    let width = (2.5 * k).max(2.0);
    for (i, pair) in s.route.windows(2).enumerate().filter(|(_, p)| inside(p[0]) && inside(p[1])) {
        let (a, b) = (at(pair[0]), at(pair[1]));
        draw_line(a.x, a.y, b.x, b.y, width, route_colour(i, n));
    }
    let marks = s.marks.iter().filter(|m| !matches!(m, Mark::Battle { cell, .. } if !inside(*cell)));
    for mark in marks {
        match *mark {
            Mark::Battle { cell, won } => {
                let c = at(cell);
                let arm = (kx.min(ky) * 1.1).max(7.0 * k);
                let col = if won { Color::new(0.94, 0.94, 0.94, 1.0) } else { Color::new(0.92, 0.22, 0.18, 1.0) };
                draw_line(c.x - arm, c.y - arm, c.x + arm, c.y + arm, 4.0 * k, BLACK);
                draw_line(c.x + arm, c.y - arm, c.x - arm, c.y + arm, 4.0 * k, BLACK);
                draw_line(c.x - arm, c.y - arm, c.x + arm, c.y + arm, 2.4 * k, col);
                draw_line(c.x + arm, c.y - arm, c.x - arm, c.y + arm, 2.4 * k, col);
            }
            Mark::Capture { location } => {
                let Some(l) = game.world.locations.iter().find(|l| l.id == location) else { continue };
                if !inside(l.tile) {
                    continue;
                }
                let c = at(l.tile);
                let gold = Color::new(0.95, 0.75, 0.25, 1.0);
                let h = (kx.min(ky) * 2.2).max(12.0 * k);
                draw_line(c.x, c.y - h, c.x, c.y + h * 0.3, 2.0 * k, BLACK);
                draw_triangle(vec2(c.x, c.y - h), vec2(c.x + h * 0.8, c.y - h * 0.7), vec2(c.x, c.y - h * 0.4), gold);
            }
        }
    }
    if s.died {
        if let Some(&last) = s.route.last().filter(|&&t| inside(t)) {
            let c = at(last);
            let r = (kx.min(ky) * 1.1).max(7.0 * k);
            draw_circle(c.x, c.y, r, Color::new(0.94, 0.93, 0.9, 1.0));
            draw_circle(c.x - r * 0.4, c.y - r * 0.15, r * 0.25, BLACK);
            draw_circle(c.x + r * 0.4, c.y - r * 0.15, r * 0.25, BLACK);
            draw_rectangle(c.x - r * 0.4, c.y + r * 0.35, r * 0.8, r * 0.3, BLACK);
        }
    }
    draw_rectangle_lines(pic.x, pic.y, pic.w, pic.h, 1.0 * k.max(1.0), Color::new(0.62, 0.64, 0.62, 1.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page's parts stay inside the window's inner box and do not overlap, at the desktop's sizes
    /// and on phones held either way (the window's inner box at the scale of each).
    #[test]
    fn the_page_fits_its_window_at_every_size() {
        for (w, h, upright, k) in [(800.0f32, 600.0f32, false, 0.8f32), (1280.0, 800.0, false, 1.0), (1920.0, 1080.0, false, 1.4), (393.0, 873.0, true, 1.0), (873.0, 393.0, false, 1.0)] {
            let win = window_rect(w, h, upright || w < h, k, display::SafeArea::default());
            let inner = Rect::new(win.x + 10.0 * k, win.y + 30.0 * k, win.w - 20.0 * k, win.h - 40.0 * k);
            let body = 14.0 * k;
            let l = layout(inner, upright, 8.0 * k, 36.0 * k, (60.0 * k, totals_need(body, body * 1.3)));
            for r in [l.pane, l.highlights, l.totals, l.button] {
                assert!(r.x >= inner.x - 0.01 && r.y >= inner.y - 0.01 && r.right() <= inner.right() + 0.01 && r.bottom() <= inner.bottom() + 0.01, "{r:?} leaves {inner:?} at {w}x{h}");
            }
            assert!(l.highlights.intersect(l.totals).is_none(), "the highlights and the totals overlap at {w}x{h}");
            assert!(l.totals.h >= totals_need(body, body * 1.3) - 0.01, "the totals table gets its height at {w}x{h}");
        }
    }

    #[test]
    fn the_map_is_cropped_to_the_explored_land_with_a_margin() {
        // A 100 x 80 map with the land explored in 40..=49 x 20..=29.
        let explored = |t: Tile| (40..=49).contains(&t.0) && (20..=29).contains(&t.1);
        assert_eq!(explored_box(100, 80, explored, 3), (37, 17, 53, 33));
        // At the map's edge the margin stops at the edge.
        assert_eq!(explored_box(100, 80, |t| t == (0, 0), 3), (0, 0, 4, 4));
        // Nothing explored: the whole map.
        assert_eq!(explored_box(100, 80, |_| false, 3), (0, 0, 100, 80));
    }

    #[test]
    fn the_picture_letterboxes_inside_its_pane_and_keeps_its_aspect() {
        let pane = Rect::new(0.0, 0.0, 200.0, 100.0);
        let wide = letterbox(pane, 40.0, 20.0);
        assert!((wide.w / wide.h - 2.0).abs() < 1e-4 && wide.w <= 200.0 && wide.h <= 100.0);
        assert!((wide.x - 0.0).abs() < 1e-4 && (wide.y - 0.0).abs() < 1e-4, "2:1 fills the 2:1 pane");
        let tall = letterbox(pane, 10.0, 20.0);
        assert!((tall.h - 100.0).abs() < 1e-4 && (tall.x - 75.0).abs() < 1e-4, "a tall crop is centred: {tall:?}");
    }

    #[test]
    fn the_route_runs_from_cool_to_warm() {
        let (start, end) = (route_colour(0, 10), route_colour(9, 10));
        assert!(start.b > start.r, "{start:?} is not cool");
        assert!(end.r > end.b, "{end:?} is not warm");
        assert_eq!(route_colour(0, 1).b, route_colour(0, 10).b, "one cell is the start colour");
    }

    #[test]
    fn the_text_takes_the_largest_size_that_fits() {
        assert_eq!(fit_size(&[14.0, 12.0, 10.0], |s| s <= 12.5), 12.0);
        assert_eq!(fit_size(&[14.0, 12.0], |_| true), 14.0);
        assert_eq!(fit_size(&[14.0, 12.0], |_| false), 12.0, "none fits: the smallest");
    }
}
