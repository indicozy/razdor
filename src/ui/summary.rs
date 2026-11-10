//! The «Итоги» page (Razdor's, the chronicle's summary; `rules::chronicle::Summary`): the map as
//! the hero explored it, his route on it (cool at the start, warm at the end), the battles and the
//! captures marked where they happened, the highlights and the totals. It opens after a map ends
//! (a victory, a defeat or the hero's fall, unless the setting «Итоги после карты» is off), before
//! the next screen, and from the chronicle tab («Летопись», the whole story so far). Every part
//! fits one screen at the desktop's sizes and on a phone held either way (`layout`, `fit_size`).

use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::rules::chronicle::{Mark, Summary};
use razdor::rules::game::Game;
use razdor::rules::map::Tile;

use super::assets::Assets;
use super::chrome;
use super::display;
use super::items_view::HeroView;
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

/// The page on screen: what it shows and where it goes.
#[derive(Debug)]
pub struct View {
    pub after: After,
    pub summary: Summary,
}

/// What a frame of the page leaves to the app.
pub enum Exit {
    Back(Box<Screen>),
    Victory,
    Defeat,
}

/// The page for `after`: a map's summary (after a map, the chapter just played) or the story so
/// far (from the chronicle tab). None when the record holds no chapter. `died`: the hero fell.
pub fn open(game: &Game, after: After, died: bool) -> Option<Screen> {
    let summary = game.chronicle.summary(from_chapter(game, after), game.clock.total_minutes() as u64, died)?;
    minimap::snap_fog();
    Some(Screen::Summary(Box::new(View { after, summary })))
}

/// The first chapter the totals cover: the story so far from the chronicle tab, the map just played
/// after a map ends.
fn from_chapter(game: &Game, after: After) -> usize {
    match after {
        After::Diary(_) => 0,
        After::Victory | After::Defeat => game.chronicle.chapters.len().saturating_sub(1),
    }
}

/// The page's boxes in screen px: the title, the map, the highlights and the totals beside or
/// under it, and the button row at the foot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub title: Rect,
    pub map: Rect,
    pub highlights: Rect,
    pub totals: Rect,
    pub button: Rect,
}

/// The page's layout on a screen `w` × `h` px, `k` px per unit: beside the map when held
/// sideways (`upright` false), under it when upright. `reserve`: the heights the highlights and the
/// totals need at the page's text size (upright only). Upright, the totals sit at the foot, the
/// highlights above them, and the map (a square) takes what the text does not need. Sideways the
/// map is a square at the left and the text is the column beside it.
pub fn layout(w: f32, h: f32, upright: bool, k: f32, reserve: (f32, f32)) -> Layout {
    let (highlights_h, totals_h) = reserve;
    let m = 10.0 * k;
    let title_h = 36.0 * k;
    let button_h = 40.0 * k;
    let top = m + title_h + 6.0 * k;
    let bottom = h - m - button_h - m;
    let button = Rect::new(w / 2.0 - 90.0 * k, h - m - button_h, 180.0 * k, button_h);
    let title = Rect::new(m, m, w - 2.0 * m, title_h);
    if upright {
        let avail = (bottom - top).max(0.0);
        let totals = Rect::new(m, bottom - totals_h, w - 2.0 * m, totals_h.min(avail));
        let room = (avail - highlights_h - totals_h - 3.0 * m).max(avail * 0.3);
        let side = (w - 2.0 * m).min(room).max(0.0);
        let map = Rect::new((w - side) / 2.0, top, side, side);
        let y = map.bottom() + m;
        let highlights = Rect::new(m, y, w - 2.0 * m, (totals.y - m - y).max(0.0));
        Layout { title, map, highlights, totals, button }
    } else {
        let side = (bottom - top).min(w * 0.5 - m).max(0.0);
        let map = Rect::new(m, top, side, side);
        let x = map.right() + m;
        let col = (w - x - m).max(0.0);
        let highlights = Rect::new(x, top, col, ((bottom - top) * 0.55).max(0.0));
        let totals = Rect::new(x, highlights.bottom() + m, col, (bottom - highlights.bottom() - m).max(0.0));
        Layout { title, map, highlights, totals, button }
    }
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

/// The height the totals need at text size `s` in two columns (the phone's table), the heading too.
fn totals_need(rows: usize, s: f32) -> f32 {
    2.1 * s + (rows as f32 / 2.0).ceil() * s * 1.9
}

/// The totals' columns at text size `s` in a box `h` high: one column when the rows fit under the
/// heading, else two (the phone held upright). None when even two do not fit.
fn totals_columns(rows: usize, s: f32, h: f32) -> Option<usize> {
    let line = s * 1.9;
    let head = s * 2.1;
    if head + rows as f32 * line <= h {
        Some(1)
    } else if head + (rows as f32 / 2.0).ceil() * line <= h {
        Some(2)
    } else {
        None
    }
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

/// Draws the page and takes its input: its button, Enter or Esc leave it. Returns where it goes.
pub fn frame(view: &mut View, game: &Game, assets: &Assets) -> Option<Exit> {
    let dp = display::dp_px();
    let _scale = display::enter_window_scale(dp);
    clear_background(Color::from_rgba(20, 18, 16, 255));
    let k = chrome::k();
    let g = display::button_grow();
    let (sw, sh) = (screen_width(), screen_height());
    // The text (the highlights, then the totals in two columns, one size for both) and the layout
    // that holds it: the totals' height is reserved at the page's size, the map takes the rest.
    let highlights = razdor::rules::chronicle_text::highlight_lines(game, &view.summary);
    let rows = total_rows(&view.summary);
    let base = 14.0 * k * g;
    let sizes = [base, base * 0.92, base * 0.85, base * 0.78];
    let upright = dp.is_some() && window_layout::portrait();
    let highlight_need = |s: f32, w: f32| {
        let lines: usize = highlights.iter().map(|h| wrap(h, w, s).len()).sum();
        lines as f32 * s * 1.3 + highlights.len() as f32 * s * 0.5 + s * 1.6
    };
    // Upright, the text at the page's size is reserved first (the map takes what is left).
    let reserve = if upright { (highlight_need(base, sw - 20.0 * k), totals_need(rows.len(), base)) } else { (0.0, totals_need(rows.len(), base)) };
    let lay = layout(sw, sh, upright, k, reserve);
    let size = fit_size(&sizes, |s| highlight_need(s, lay.highlights.w) <= lay.highlights.h && totals_columns(rows.len(), s, lay.totals.h).is_some());
    let columns = totals_columns(rows.len(), size, lay.totals.h).unwrap_or(2);

    let title = tr("Summary");
    let subtitle = view.summary.title.clone();
    let title_size = (18.0 * k * g).round();
    chrome::shadow_centered(title, lay.title.x + lay.title.w / 2.0, lay.title.y + lay.title.h * 0.42, title_size, ACCENT);
    if !subtitle.is_empty() {
        text_centered(&subtitle, lay.title.x + lay.title.w / 2.0, lay.title.y + lay.title.h * 0.92, (13.0 * k * g).round(), DIM);
    }

    draw_map(game, assets, &view.summary, lay.map, k);
    draw_highlights(&highlights, lay.highlights, size, k);
    draw_totals(&rows, lay.totals, size, columns);

    // The button: Close from the chronicle, Next after a map (the next map or the main menu).
    let label = match view.after {
        After::Diary(_) => tr("Close"),
        After::Victory | After::Defeat => tr("Next"),
    };
    let pressed = button(lay.button.x, lay.button.y, lay.button.w, lay.button.h, label, true);
    if !(pressed || key(KeyCode::Enter) || key(KeyCode::Escape)) {
        return None;
    }
    Some(match view.after {
        After::Diary(hero) => Exit::Back(Box::new(Screen::Hero(hero))),
        After::Victory => Exit::Victory,
        After::Defeat => Exit::Defeat,
    })
}

/// The highlights: a heading, then each sentence wrapped to the box.
fn draw_highlights(lines: &[String], r: Rect, size: f32, k: f32) {
    chrome::shadow_text(tr("Highlights"), r.x, r.y + size, size * 1.1, ACCENT);
    let mut y = r.y + size * 2.1;
    for sentence in lines {
        for line in wrap(sentence, r.w, size) {
            text(&line, r.x, y, size, INK);
            y += size * 1.3;
        }
        y += size * 0.5 + k;
    }
}

/// The totals under a heading, in `columns` columns of label and value (the value at the column's
/// right, with a gap before the next column).
fn draw_totals(rows: &[(&str, u32)], r: Rect, size: f32, columns: usize) {
    chrome::shadow_text(tr("Totals"), r.x, r.y + size, size * 1.1, ACCENT);
    let per_column = rows.len().div_ceil(columns.max(1));
    let col_w = r.w / columns.max(1) as f32;
    for (i, (label, value)) in rows.iter().enumerate() {
        let (c, n) = (i / per_column, i % per_column);
        let x = r.x + c as f32 * col_w;
        let y = r.y + size * 2.1 + n as f32 * size * 1.9;
        text(label, x, y, size, DIM);
        let v = value.to_string();
        let w = measure(&v, size).width;
        text(&v, x + col_w - w - 16.0 * size / 14.0, y, size, INK);
    }
}

/// The map: its picture (ground and fog as they stand), the route and the marks, in `r`.
fn draw_map(game: &Game, assets: &Assets, s: &Summary, r: Rect, k: f32) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    draw_rectangle(r.x, r.y, r.w, r.h, BLACK);
    minimap::picture(game, assets.dt.as_ref(), super::world_view::surface_color, r);
    let map = &game.world.map;
    let (kx, ky) = (r.w / map.w.max(1) as f32, r.h / map.h.max(1) as f32);
    let at = |t: Tile| vec2(r.x + (t.0 as f32 + 0.5) * kx, r.y + (t.1 as f32 + 0.5) * ky);
    // A cell off the map is not drawn: the route and the marks stay inside the picture.
    let on_map = |t: Tile| t.0 >= 0 && t.1 >= 0 && t.0 < map.w && t.1 < map.h;
    let n = s.route.len();
    for (i, pair) in s.route.windows(2).enumerate().filter(|(_, p)| on_map(p[0]) && on_map(p[1])) {
        let (a, b) = (at(pair[0]), at(pair[1]));
        draw_line(a.x, a.y, b.x, b.y, 2.5 * k, route_colour(i, n));
    }
    let marks = s.marks.iter().filter(|m| !matches!(m, Mark::Battle { cell, .. } if !on_map(*cell)));
    for mark in marks {
        match *mark {
            Mark::Battle { cell, won } => {
                let c = at(cell);
                let (arm, col) = (6.0 * k, if won { Color::new(0.92, 0.92, 0.92, 1.0) } else { Color::new(0.92, 0.22, 0.18, 1.0) });
                draw_line(c.x - arm, c.y - arm, c.x + arm, c.y + arm, 3.0 * k, BLACK);
                draw_line(c.x + arm, c.y - arm, c.x - arm, c.y + arm, 3.0 * k, BLACK);
                draw_line(c.x - arm, c.y - arm, c.x + arm, c.y + arm, 1.6 * k, col);
                draw_line(c.x + arm, c.y - arm, c.x - arm, c.y + arm, 1.6 * k, col);
            }
            Mark::Capture { location } => {
                let Some(l) = game.world.locations.iter().find(|l| l.id == location) else { continue };
                let c = at(l.tile);
                let gold = Color::new(0.95, 0.75, 0.25, 1.0);
                draw_line(c.x, c.y - 11.0 * k, c.x, c.y + 3.0 * k, 1.5 * k, BLACK);
                draw_triangle(vec2(c.x, c.y - 11.0 * k), vec2(c.x + 9.0 * k, c.y - 8.0 * k), vec2(c.x, c.y - 5.0 * k), gold);
            }
        }
    }
    if s.died {
        if let Some(&last) = s.route.last().filter(|&&t| on_map(t)) {
            let c = at(last);
            draw_circle(c.x, c.y, 6.5 * k, Color::new(0.94, 0.93, 0.9, 1.0));
            draw_circle(c.x - 2.5 * k, c.y - 1.0 * k, 1.8 * k, BLACK);
            draw_circle(c.x + 2.5 * k, c.y - 1.0 * k, 1.8 * k, BLACK);
            draw_rectangle(c.x - 2.5 * k, c.y + 3.0 * k, 5.0 * k, 2.0 * k, BLACK);
        }
    }
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0 * k, Color::new(0.62, 0.64, 0.62, 1.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page's boxes never overlap and stay on the screen, at the desktop's sizes and on phones
    /// held either way (the scale of each: the fit at 800×600 and 1280×800, a phone's dp).
    #[test]
    fn the_page_fits_its_screen_at_every_size() {
        for (w, h, upright, k) in [(800.0, 600.0, false, 0.8), (1280.0, 800.0, false, 1.0), (1920.0, 1080.0, false, 1.4), (393.0, 873.0, true, 1.0), (873.0, 393.0, false, 1.0)] {
            let l = layout(w, h, upright, k, (60.0 * k, super::totals_need(8, 14.0 * k)));
            for r in [l.title, l.map, l.highlights, l.totals, l.button] {
                assert!(r.x >= 0.0 && r.y >= 0.0 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01, "{r:?} leaves {w}x{h}");
            }
            assert!(l.map.bottom() <= l.button.y + 0.01 || l.map.x >= l.button.right() || l.map.right() <= l.button.x, "the map runs into the button");
            assert!(l.highlights.h > 40.0 * k, "{w}x{h}: the highlights get {}", l.highlights.h);
            assert!(l.totals.h > 60.0 * k, "{w}x{h}: the totals get {}", l.totals.h);
            assert!(l.highlights.intersect(l.totals).is_none(), "the highlights and the totals overlap at {w}x{h}");
        }
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
