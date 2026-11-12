//! The windows of the performance test (`bench`): the choice («Мои настройки» or «Все пресеты»), the
//! run's progress with its Cancel, and the results with «Сохранить» and «Закрыть». They draw over
//! the settings (or the map, while a run plays) and answer the buttons; the logic is in `bench`.

use macroquad::prelude::*;
use razdor::i18n::tr;
use razdor::local;
use razdor::trf;

use super::audio::{cue, Cue};
use super::bench::{self, Choice, Kind, Report};
use super::chrome::{self, Skin};
use super::widgets;
use super::App;

/// What the window's buttons asked for this frame.
enum Act {
    None,
    Start(Choice),
    Cancel,
    Save,
    Close,
}

/// The test's window over the screen, drawn last (`App::frame_content`): the dim, the window of the phase, and
/// its button's action.
pub fn modal(app: &mut App) {
    let kind = bench::kind();
    if kind == Kind::Closed {
        return;
    }
    // A phone: the window's own scale is one dp (as the update window's), fitted to the screen.
    let _scale = super::display::dp_px().map(|dp| super::display::enter_window_scale(Some(dp)));
    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.35));
    let act = match kind {
        Kind::Closed => Act::None,
        Kind::Choice(err) => choice(err.as_deref()),
        Kind::Running => running(),
        Kind::Done => results(),
    };
    match act {
        Act::None => {}
        Act::Start(choice) => {
            if let Err(e) = bench::start(app, choice) {
                bench::choice_error(e);
            }
        }
        Act::Cancel => bench::cancel(app),
        Act::Close => bench::close(),
        Act::Save => {
            if let Some(report) = bench::report() {
                save(&report);
            }
        }
    }
}

/// The window's rectangle: `w` by `h` design units (`k` each), centred in the screen above the bottom bar and
/// inside the safe area, and never wider or taller than that room.
fn centred(w: f32, h: f32, k: f32) -> Rect {
    let safe = super::display::safe_area();
    let (sw, sh) = (screen_width(), screen_height());
    let bar = chrome::bar_height();
    let room_w = sw - safe.left - safe.right;
    let room_h = sh - safe.top - bar;
    let (w, h) = (w.min(room_w - 16.0 * k), h.min(room_h - 16.0 * k));
    Rect::new(safe.left + (room_w - w) / 2.0, safe.top + (room_h - h) / 2.0, w, h)
}

/// The window's text colour (the parchment's cream, as the other windows' text).
fn ink() -> Color {
    chrome::CREAM
}

/// Wrapped lines of `text` in `width`, drawn from `y` down; returns the y below the last line.
fn paragraph(text: &str, x: f32, y: f32, width: f32, size: f32, color: Color) -> f32 {
    let mut y = y;
    for line in chrome::wrap_label(text, width, size) {
        chrome::shadow_text(&line, x, y, size, color);
        y += size * 1.3;
    }
    y
}

/// The choice window: a short hint, two buttons with how long each takes, and «Отмена».
fn choice(err: Option<&str>) -> Act {
    let k = chrome::k();
    // A phone's window is narrow: its two choices are stacked, full width, and the window is taller.
    let phone = super::display::dp_px().is_some();
    let (inner, closed) = chrome::window(centred(540.0 * k, if phone { 430.0 } else { 310.0 } * k, k), tr("Performance test"), Skin::Marble, true);
    if closed {
        return Act::Cancel;
    }
    let pad = 24.0 * k;
    let (x, w) = (inner.x + pad, inner.w - 2.0 * pad);
    let size = 15.0 * k;
    let mut y = inner.y + pad + size;
    y = paragraph(tr("Measures how fast the game runs on this device: a map near and far, with your settings or with each quality preset. Clicks and taps do nothing until it is done. Esc stops it."), x, y, w, size, ink());
    if let Some(e) = err {
        y = paragraph(e, x, y + 6.0 * k, w, size, Color::new(0.95, 0.55, 0.45, 1.0));
    }
    let gap = 16.0 * k;
    let bh = 44.0 * k;
    let by = y.max(inner.y + inner.h * 0.5) + 4.0 * k;
    let (mine, all) = if phone {
        (Rect::new(x, by, w, bh), Rect::new(x, by + bh + 34.0 * k, w, bh))
    } else {
        let bw = ((w - gap) / 2.0).min(220.0 * k);
        (Rect::new(x, by, bw, bh), Rect::new(x + bw + gap, by, bw, bh))
    };
    let (_, mine_down, mine_pressed) = widgets::button_state(mine, true);
    chrome::marble_button(mine, tr("My settings"), true, mine_down);
    let (_, all_down, all_pressed) = widgets::button_state(all, true);
    chrome::marble_button(all, tr("All presets"), true, all_down);
    chrome::shadow_text(tr("about 30 seconds"), mine.x + 4.0 * k, mine.y + bh + 18.0 * k, 13.0 * k, ink());
    chrome::shadow_text(tr("about 3 minutes"), all.x + 4.0 * k, all.y + bh + 18.0 * k, 13.0 * k, ink());
    let cancel = Rect::new(inner.x + inner.w - pad - 150.0 * k, inner.y + inner.h - pad - 40.0 * k, 150.0 * k, 40.0 * k);
    let (_, cancel_down, cancel_pressed) = widgets::button_state(cancel, true);
    chrome::marble_button(cancel, tr("Cancel"), true, cancel_down);
    if mine_pressed {
        cue(Cue::Button);
        return Act::Start(Choice::Mine);
    }
    if all_pressed {
        cue(Cue::Button);
        return Act::Start(Choice::All);
    }
    if cancel_pressed {
        cue(Cue::Button);
        return Act::Close;
    }
    Act::None
}

/// The run's window: which cell is measured, the phase's time, a bar, and «Отмена».
fn running() -> Act {
    let k = chrome::k();
    let phone = super::display::dp_px().is_some();
    let (inner, closed) = chrome::window(centred(540.0 * k, if phone { 330.0 } else { 240.0 } * k, k), tr("Performance test"), Skin::Marble, true);
    if closed {
        return Act::Cancel;
    }
    let Some(p) = bench::progress() else { return Act::None };
    let pad = 24.0 * k;
    let (x, w) = (inner.x + pad, inner.w - 2.0 * pad);
    let size = 15.0 * k;
    let mut y = inner.y + pad + size;
    chrome::shadow_text(&trf!("Cell {n} of {m}", n = p.index + 1, m = p.total), x, y, size, ink());
    y += size * 1.6;
    y = paragraph(&p.name, x, y, w, size, ink());
    let phase = if p.measuring {
        trf!("Measuring: {s} s of {total} s", s = format!("{:.0}", p.elapsed.min(bench::MEASURE_SECS)), total = format!("{:.0}", bench::MEASURE_SECS))
    } else {
        trf!("Settling: {s} s of {total} s", s = format!("{:.0}", p.elapsed.min(bench::SETTLE_SECS)), total = format!("{:.0}", bench::SETTLE_SECS))
    };
    y += size * 0.6;
    chrome::shadow_text(&phase, x, y + size, size, ink());
    // The bar: the cells done, and the current cell's share of its time.
    let bar_y = y + size * 2.0;
    let bar = Rect::new(x, bar_y, w, 12.0 * k);
    draw_rectangle(bar.x, bar.y, bar.w, bar.h, Color::new(0.0, 0.0, 0.0, 0.45));
    let done = (p.index as f32 + if p.measuring { p.elapsed as f32 / (bench::SETTLE_SECS + bench::MEASURE_SECS) as f32 } else { 0.0 }) / p.total.max(1) as f32;
    draw_rectangle(bar.x, bar.y, bar.w * done.clamp(0.0, 1.0), bar.h, Color::new(0.45, 0.78, 0.33, 1.0));
    // The hint keeps clear of the Cancel button at the foot.
    let _ = paragraph(tr("Do not touch the screen. The test stops by itself, then its results are shown."), x, bar_y + 30.0 * k, w - 170.0 * k, 13.0 * k, ink());
    let cancel = Rect::new(inner.x + inner.w - pad - 150.0 * k, inner.y + inner.h - pad - 40.0 * k, 150.0 * k, 40.0 * k);
    let (_, down, pressed) = widgets::button_state(cancel, true);
    chrome::marble_button(cancel, tr("Cancel"), true, down);
    if pressed {
        cue(Cue::Button);
        return Act::Cancel;
    }
    Act::None
}

// How far the results' cells are scrolled, in screen pixels (the list scrolls by the wheel, a finger and its bar).
local! {
    static SCROLL: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
}

/// The results window: the table (one row per cell) in a list that scrolls, the device line, what was saved, and
/// «Сохранить» and «Закрыть» under it. A phone (and its landscape) is shorter than the table: the cells are stacked,
/// each name over its values, and the list scrolls.
fn results() -> Act {
    let Some(rep) = bench::report() else { return Act::None };
    let k = chrome::k();
    let phone = super::display::dp_px().is_some();
    let (inner, closed) = chrome::window(centred(680.0 * k, 600.0 * k, k), tr("Performance test"), Skin::Marble, true);
    if closed {
        return Act::Close;
    }
    let pad = 16.0 * k;
    let x0 = inner.x + pad;
    let width = inner.w - 2.0 * pad;
    let size = 14.0 * k;
    let mut y = inner.y + 14.0 * k;
    y = paragraph(&trf!("Map: {map}. The frame times are the whole frame, vertical sync included.", map = rep.map.clone()), x0, y + size, width, 13.0 * k, ink()) + 8.0 * k;
    // The columns of the table share the width in these proportions.
    let share = [0.36, 0.11, 0.18, 0.18, 0.17];
    let cols: Vec<f32> = share.iter().map(|s| s * width).collect();
    let xs: Vec<f32> = share
        .iter()
        .scan(0.0, |acc, s| {
            let x = *acc;
            *acc += s;
            Some(x)
        })
        .map(|f| x0 + f * width)
        .collect();
    if !phone {
        let names = [tr("Cell"), tr("FPS"), tr("Frame ms, median"), tr("Frame ms, 99%"), tr("Slow frames")];
        let mut head_h: f32 = 0.0;
        for (i, name) in names.iter().enumerate() {
            let lines = chrome::wrap_label(name, cols[i] - 6.0 * k, 13.0 * k);
            head_h = head_h.max(lines.len() as f32 * 15.0 * k);
            for (n, line) in lines.iter().enumerate() {
                chrome::shadow_text(line, xs[i] + 2.0 * k, y + 13.0 * k + n as f32 * 15.0 * k, 13.0 * k, ink());
            }
        }
        y += head_h + 8.0 * k;
        draw_rectangle(x0, y - 4.0 * k, width, 1.0, Color::new(0.93, 0.87, 0.75, 0.5));
    }
    // The foot: the device line, what was saved, and the buttons. The list takes the room between.
    let d = &rep.device;
    let mut foot = vec![trf!("Renderer: {gpu}; Screen: {w}x{h}; Razdor {v}; system: {os}; Vertical sync: {vsync}", gpu = d.gpu.clone(), w = d.screen.0, h = d.screen.1, v = d.version.clone(), os = d.os.clone(), vsync = tr(&d.vsync))];
    let saved = match (&rep.file, &rep.error) {
        (Some(path), _) => {
            let mut t = trf!("Saved: {path}", path = path.display().to_string());
            if rep.copied {
                t.push_str(". ");
                t.push_str(tr("The text is on the clipboard."));
            }
            Some(t)
        }
        (None, Some(e)) => Some(trf!("Not saved: {e}", e = e.clone())),
        (None, None) => None,
    };
    foot.extend(saved);
    let foot_lines: Vec<Vec<String>> = foot.iter().map(|t| chrome::wrap_label(t, width, 13.0 * k)).collect();
    let foot_h: f32 = foot_lines.iter().map(|l| l.len() as f32 * 17.0 * k + 8.0 * k).sum::<f32>() + 40.0 * k + pad + 8.0 * k;
    let view_top = y;
    let view = Rect::new(x0, view_top, width, (inner.y + inner.h - foot_h - view_top).max(40.0 * k));
    // The cells: the list's height, and where each one is drawn (its y, its height), from the list's top.
    let row_h = |c: &bench::CellResult| -> f32 {
        if phone {
            let lines = chrome::wrap_label(&c.name, width, size).len().max(1) as f32;
            lines * 16.0 * k + 26.0 * k
        } else {
            let lines = chrome::wrap_label(&c.name, cols[0] - 6.0 * k, size).len().max(1) as f32;
            (lines * 16.0 * k).max(22.0 * k) + 4.0 * k
        }
    };
    let content: f32 = rep.cells.iter().map(row_h).sum();
    let most = (content - view.h).max(0.0);
    let mut scroll = SCROLL.with(|s| s.get()).clamp(0.0, most);
    if most > 0.0 {
        widgets::swipe_rows(view, 20.0 * k);
        if widgets::mouse_in(view.x, view.y, view.w, view.h) {
            scroll = (scroll - widgets::wheel() * 20.0 * k).clamp(0.0, most);
        }
        let bar = Rect::new(view.x + view.w + 4.0 * k, view.y, 10.0 * k, view.h);
        widgets::scrollbar_px(bar, &mut scroll, most, view.h, 20.0 * k);
    }
    SCROLL.with(|s| s.set(scroll));
    let mut cy = view.y - scroll;
    for c in &rep.cells {
        let h = row_h(c);
        if cy >= view.y && cy + h <= view.y + view.h {
            let name_lines = chrome::wrap_label(&c.name, if phone { width } else { cols[0] - 6.0 * k }, size);
            for (n, line) in name_lines.iter().enumerate() {
                chrome::shadow_text(line, x0 + 2.0 * k, cy + size + n as f32 * 16.0 * k, size, ink());
            }
            let s = &c.stats;
            if phone {
                let values = trf!("FPS {fps}, frame {med} ms, 99% {p99} ms, slow {slow}", fps = format!("{:.1}", s.fps), med = format!("{:.1}", s.frame_med), p99 = format!("{:.1}", s.frame_p99), slow = s.slow);
                chrome::shadow_text(&values, x0 + 2.0 * k, cy + h - 8.0 * k, 13.0 * k, ink());
            } else {
                let vals = [format!("{:.1}", s.fps), format!("{:.1}", s.frame_med), format!("{:.1}", s.frame_p99), format!("{}", s.slow)];
                for (i, v) in vals.iter().enumerate() {
                    chrome::shadow_text(v, xs[i + 1] + 2.0 * k, cy + size, size, ink());
                }
            }
        }
        cy += h;
    }
    // The foot under the list.
    let mut fy = view.y + view.h + 8.0 * k;
    for lines in &foot_lines {
        for line in lines {
            chrome::shadow_text(line, x0, fy + 13.0 * k, 13.0 * k, ink());
            fy += 17.0 * k;
        }
        fy += 8.0 * k;
    }
    let bh = 40.0 * k;
    let by = inner.y + inner.h - pad - bh;
    let bw = 160.0 * k;
    let save = Rect::new(inner.x + inner.w - pad - 2.0 * bw - 12.0 * k, by, bw, bh);
    let close = Rect::new(inner.x + inner.w - pad - bw, by, bw, bh);
    let (_, save_down, save_pressed) = widgets::button_state(save, true);
    chrome::marble_button(save, tr("Save"), true, save_down);
    let (_, close_down, close_pressed) = widgets::button_state(close, true);
    chrome::marble_button(close, tr("Close"), true, close_down);
    if save_pressed {
        cue(Cue::Button);
        return Act::Save;
    }
    if close_pressed {
        cue(Cue::Button);
        return Act::Close;
    }
    Act::None
}

/// Saves the report: the file in the app's folder, and its text on the clipboard.
fn save(report: &Report) {
    let text = bench::report_text(report);
    let file = bench::save(report);
    let copied = file.is_ok();
    if copied {
        macroquad::miniquad::window::clipboard_set(&text);
    }
    bench::note_saved(file, copied);
}
