//! Shared geometry and production visual tuning for building and army windows.
use macroquad::prelude::*;
use super::chrome;
pub const WIDTH: f32 = 836.0;
pub const HEIGHT: f32 = 600.0;
const DIVIDER: f32 = 28.7;
#[derive(Clone, Copy)]
pub struct Visual {
    pub quest_brightness: f32, pub quest_gamma: f32, pub quest_opacity: f32, pub quest_cutoff: f32,
    pub brown_brightness: f32, pub brown_gamma: f32,
    pub white_brightness: f32, pub white_gamma: f32, pub white_ornament: f32,
    pub paper_edge_width: f32, pub paper_edge_opacity: f32,
    pub bar_ornament_brightness: f32, pub bar_ornament_gamma: f32, pub bar_ornament_opacity: f32,
    pub bar_gradient_bottom: f32,
}
pub fn visual() -> Visual {
    Visual {
        quest_brightness: 1.070, quest_gamma: 1.167, quest_opacity: 1.0, quest_cutoff: 0.0,
        brown_brightness: 1.057, brown_gamma: 0.805,
        white_brightness: 1.007, white_gamma: 0.914, white_ornament: 0.576,
        paper_edge_width: 3.724, paper_edge_opacity: 0.400,
        bar_ornament_brightness: 2.000, bar_ornament_gamma: 0.904,
        bar_ornament_opacity: 1.000, bar_gradient_bottom: 0.388,
    }
}
fn upper_height() -> f32 { (570.0 - DIVIDER) / 2.0 }
fn lower_y() -> f32 { 28.0 + upper_height() + DIVIDER }
fn lower_height() -> f32 { upper_height() }

/// A phone held upright (the owner's experiment): the window is 590 × 1172 design px, its left
/// column on top (centred) and the right side below it, each at its own size.
const PW: f32 = 590.0;
const PH: f32 = 1172.0;
/// The seam between the stacked halves (27 + 571 + 1): the 2 px line drawn across the window.
const SEAM_Y: f32 = 599.0;

/// Whether the window is stacked: a phone (`display::dp_px`) held upright.
pub fn portrait() -> bool {
    super::display::dp_px().is_some() && screen_height() > screen_width()
}

/// A design position moved for the stacked layout: the right side (x ≥ 247) goes from its
/// landscape origin (248, 28) to (2, 601) under the left column, which is centred across the
/// width (x + 171). Landscape positions are unchanged.
pub fn place(x: f32, y: f32, portrait: bool) -> (f32, f32) {
    if !portrait {
        (x, y)
    } else if x >= 247.0 {
        (x - 246.0, y + 573.0)
    } else {
        (x + 171.0, y)
    }
}

fn point_in(win: Rect, k: f32, x: f32, y: f32, portrait: bool) -> Vec2 {
    let (px, py) = place(x, y, portrait);
    vec2(win.x + px * k, win.y + py * k)
}

fn rect_in(win: Rect, k: f32, x: f32, y: f32, w: f32, h: f32, portrait: bool) -> Rect {
    let p = point_in(win, k, x, y, portrait);
    Rect::new(p.x, p.y, w * k, h * k)
}

fn at_in(win: Rect, k: f32, x: f32, y: f32, w: f32, h: f32, portrait: bool) -> Rect {
    let (yy,hh) = if y >= 326.0 { let s=lower_height()/272.0; (lower_y()+(y-326.0)*s,h*s) }
    else if y >= 300.0 { let s=DIVIDER/26.0; (28.0+upper_height()+(y-300.0)*s,h*s) }
    else if y >= 28.0 { let s=upper_height()/272.0; (28.0+(y-28.0)*s,h*s) }
    else {(y,h)};
    let p = point_in(win, k, x, yy, portrait);
    Rect::new(p.x, p.y, w*k, hh*k)
}

fn left_in(win: Rect, k: f32, portrait: bool) -> Rect { rect_in(win, k, 2.0, 27.0, 244.0, 571.0, portrait) }
fn upper_in(win: Rect, k: f32, portrait: bool) -> Rect { rect_in(win, k, 248.0, 28.0, 586.0, upper_height(), portrait) }
fn divider_in(win: Rect, k: f32, portrait: bool) -> Rect { rect_in(win, k, 248.0, 28.0 + upper_height(), 586.0, DIVIDER, portrait) }
fn lower_in(win: Rect, k: f32, portrait: bool) -> Rect { rect_in(win, k, 248.0, lower_y(), 586.0, lower_height(), portrait) }

/// The window's scale when it is stacked: the largest that fits the screen's width and the
/// height above the bottom bar, never below the interface scale. Called outside the window's
/// own scale ([`scaled`]).
pub fn portrait_k() -> f32 {
    let fit = (screen_width() / PW).min((screen_height() - chrome::bar_height() - 4.0) / PH);
    fit.max(chrome::k())
}

/// Runs `f` (a building, army or hero window's whole frame) at the stacked window's own scale
/// when the phone is held upright; anywhere else `f` runs as it is.
pub fn scaled<R>(f: impl FnOnce() -> R) -> R {
    if portrait() {
        super::display::with_window_scale(Some(portrait_k()), f)
    } else {
        f()
    }
}

/// The window's rectangle: the building and army windows' size at the interface scale (the
/// stacked one when the phone is held upright), centred over the view above the bottom bar.
pub fn window() -> Rect {
    let k = chrome::k();
    if portrait() {
        return centred((PW * k).round(), (PH * k).round());
    }
    landscape_window()
}

/// The landscape window, whatever the screen: the spellbook's window, which stays side by side.
pub fn landscape_window() -> Rect {
    let k = chrome::k();
    centred((WIDTH * k).round(), (HEIGHT * k).round())
}

fn centred(w: f32, h: f32) -> Rect {
    Rect::new(((screen_width()-w)/2.0).round(), ((screen_height()-chrome::bar_height()-h)/2.0).max(2.0).round(), w, h)
}

/// A design position of the window (its x and y as the design has them, before the stacking):
/// the screen point the window draws it at. Use it for anything that sits at a design position
/// instead of the helpers below.
pub fn point(win: Rect, x: f32, y: f32) -> Vec2 {
    point_in(win, chrome::k(), x, y, portrait())
}

/// Map design coordinates to the fixed, symmetric upper and lower panes.
pub fn at(win: Rect, x: f32, y: f32, w: f32, h: f32) -> Rect {
    at_in(win, chrome::k(), x, y, w, h, portrait())
}

pub fn left(win: Rect) -> Rect { left_in(win, chrome::k(), portrait()) }
/// The stacked window's seam: the 2 px line across it between the two halves (drawn by
/// [`frame`]; a window that does not call `frame` draws it itself when stacked).
pub fn seam(win: Rect) {
    let k = chrome::k();
    let px = chrome::opx();
    let y = win.y + SEAM_Y * k;
    chrome::silver_line(vec2(win.x + 2.0 * k, y), vec2(win.right() - 2.0 * px, y), 2.0 * px);
}

/// The army and building windows' lines, drawn last so tab artwork cannot cover them: the
/// window's frame and title line (the 896×640 window made by 0x48d710 without inner frames,
/// 4dc126), the 2 px line between the left column and the right side from under the title
/// line (y 29) to the frame, and the 2 px lines over and under the hint strip across the
/// right side (4cd330, 4bb7c4: (264, 29)–(266, 638), (266, 319)–(894, 321),
/// (266, 346)–(894, 348) of the 896×640 window; Razdor's 836×600 layout puts them at its own
/// column and strip). Stacked, the seam is a 2 px line across the window at `SEAM_Y`.
pub fn frame(win: Rect) {
    let k = chrome::k();
    let px = chrome::opx();
    chrome::window_frames(win, px);
    let strip = divider(win);
    let from = if portrait() {
        seam(win);
        win.x + 2.0 * k
    } else {
        let x = win.x + 247.0 * k;
        chrome::silver_line(vec2(x, win.y + 29.0 * px), vec2(x, win.bottom() - 2.0 * px), 2.0 * px);
        x + px
    };
    for y in [strip.y + px, strip.bottom() - px] {
        chrome::silver_line(vec2(from, y), vec2(win.right() - 2.0 * px, y), 2.0 * px);
    }
}
pub fn panel_frame(r: Rect) {
    let gap = 2.0 * chrome::k();
    chrome::silver_frame(Rect::new(r.x + gap, r.y + gap, r.w - 2.0 * gap, r.h - 2.0 * gap), chrome::k());
}
pub fn upper(win: Rect) -> Rect { upper_in(win, chrome::k(), portrait()) }
pub fn divider(win: Rect) -> Rect { divider_in(win, chrome::k(), portrait()) }
pub fn lower(win: Rect) -> Rect { lower_in(win, chrome::k(), portrait()) }
pub fn lower_content(win: Rect) -> Rect { let r=lower(win); let k=chrome::k(); Rect::new(r.x+8.0*k,r.y+8.0*k,r.w-14.0*k,r.h-16.0*k) }
pub fn quest_rects(panel: Rect) -> (Rect,Rect) {
    let map=|x:f32,y:f32,w:f32,h:f32| Rect::new(panel.x+x*panel.w,panel.y+y*panel.h,w*panel.w,h*panel.h);
    (map(0.0000,0.0000,0.7389,0.1989),map(0.7471,0.0313,0.2459,0.1440))
}
pub fn army_grid_left(panel: Rect, columns: u8, pitch: f32, trailing: f32) -> f32 {
    (panel.x+(panel.w-(columns as f32*pitch-trailing))/2.0).round()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win() -> Rect { Rect::new(10.0, 20.0, 836.0, 600.0) }

    #[test]
    fn stacked_places_the_right_side_under_the_left_column() {
        assert_eq!(place(300.0, 100.0, true), (54.0, 673.0));
        assert_eq!(place(100.0, 100.0, true), (271.0, 100.0));
        assert_eq!(place(247.0, 28.0, true), (1.0, 601.0), "the seam's x belongs to the right side");
        assert_eq!(place(300.0, 100.0, false), (300.0, 100.0), "landscape is unchanged");
        assert_eq!(place(100.0, 100.0, false), (100.0, 100.0));
    }

    #[test]
    fn landscape_helpers_keep_their_geometry() {
        let w = win();
        assert_eq!(left_in(w, 1.0, false), Rect::new(12.0, 47.0, 244.0, 571.0));
        assert_eq!(upper_in(w, 1.0, false), Rect::new(258.0, 48.0, 586.0, upper_height()));
        let d = divider_in(w, 1.0, false);
        assert_eq!((d.x, d.y, d.w), (258.0, 20.0 + 28.0 + upper_height(), 586.0));
        assert_eq!(lower_in(w, 1.0, false).y, 20.0 + lower_y());
        assert_eq!(at_in(w, 1.0, 300.0, 100.0, 10.0, 10.0, false).x, 10.0 + 300.0);
    }

    #[test]
    fn stacked_helpers_move_the_halves() {
        let w = win();
        assert_eq!(left_in(w, 1.0, true), Rect::new(10.0 + 173.0, 47.0, 244.0, 571.0));
        assert_eq!(upper_in(w, 1.0, true), Rect::new(12.0, 20.0 + 601.0, 586.0, upper_height()));
        assert_eq!(lower_in(w, 1.0, true).y, 20.0 + lower_y() + 573.0);
        let a = at_in(w, 1.0, 300.0, 100.0, 10.0, 10.0, true);
        // y 100 is in the upper pane: mapped by its scale first, then moved down by the stack.
        let yy = 28.0 + (100.0 - 28.0) * upper_height() / 272.0;
        assert_eq!((a.x, a.y), (10.0 + 54.0, 20.0 + yy + 573.0));
        assert_eq!(at_in(w, 1.0, 100.0, 100.0, 10.0, 10.0, true).x, 10.0 + 271.0);
    }
}
