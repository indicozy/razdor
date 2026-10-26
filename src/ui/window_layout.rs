//! Shared geometry and production visual tuning for building and army windows.
use macroquad::prelude::*;
use razdor::local;
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

/// The stacked window (a phone held upright, the owner's experiment) is 590 design px wide.
const PW: f32 = 590.0;
/// Which stacked layout a window uses: the building window keeps its panes on top, with the tab
/// row and Exit at the bottom; the army and hero windows have the unit's band on top and the
/// right side (pack, formation) under it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stack {
    Building,
    Army,
}
/// The building window's height: the panes at the landscape height, then the tab row and Exit
/// under them (746 design px; the 1080×2376 phone's window at the width's scale).
fn building_ph() -> f32 { lower_y() + lower_height() + LOWER_GAP + 2.0 * TAB_H + 2.0 * EDGE }
/// The unit band of the army and hero windows (420 design px): the band fills the 586 px width;
/// its text half (352 px) holds the stats and the hero's bonus paragraph at a readable size (the
/// text is never below the landscape column's), and its picture half (234 px) takes the unit's
/// figure scaled to the band's height (`unit_sheet`: its proportions kept, never cropped), about
/// two thirds of the landscape scale. 420 is the owner's experiment: at 272 the text and the
/// picture were too small on a phone held upright. The window stays width-limited at 1080x2376
/// with the map above and below it (`stacked_army_window_is_width_limited_on_a_phone`).
const BAND_H: f32 = 420.0;
/// The army and hero windows' height: the title, the band, then the right side (672 px).
const ARMY_PH: f32 = BAND_H + 672.0;
/// The army and hero windows' «Закрыть» button under the formation (stacked): 64 design px
/// tall, 2 px under the right side's bottom (599.4 + the band).
const CLOSE_Y: f32 = 602.0;
/// The window's top, under its title bar.
const TOP: f32 = 28.0;
/// The building's tab row and Exit: 64 design px tall (a touch target above 48 dp on any phone),
/// 6 px apart; the tab row 8 px under the lower pane.
const TAB_H: f32 = 64.0;
const EDGE: f32 = 6.0;
const TAB_GAP: f32 = 4.0;
const LOWER_GAP: f32 = 8.0;
/// Exit's width: 40% of the window's inner width.
const EXIT_SHARE: f32 = 0.4;

local! {
    static STACK: std::cell::Cell<Stack> = std::cell::Cell::new(Stack::Building);
}

/// The stacked layout in use: set by [`scaled`] for the window it draws.
fn stack() -> Stack {
    STACK.with(|s| s.get())
}

/// Runs `f` with the stacked layout `st` in use.
fn within<R>(st: Stack, f: impl FnOnce() -> R) -> R {
    let prev = STACK.with(|s| s.replace(st));
    let r = f();
    STACK.with(|s| s.set(prev));
    r
}

fn height_of(st: Stack) -> f32 {
    match st {
        Stack::Building => building_ph(),
        Stack::Army => ARMY_PH,
    }
}

/// How far the right side moves down under the stacked layout's top part.
fn shift_of(st: Stack) -> f32 {
    match st {
        Stack::Building => 0.0,
        Stack::Army => BAND_H,
    }
}

/// The top of the building's tab row (stacked); Exit sits under it, the window's bottom under Exit.
fn tabs_top() -> f32 {
    building_ph() - 2.0 * TAB_H - 2.0 * EDGE
}

/// Whether the window is stacked: a phone (`display::dp_px`) held upright.
pub fn portrait() -> bool {
    super::display::dp_px().is_some() && screen_height() > screen_width()
}

/// A design position moved for the stacked layout: the right side (x ≥ 247) goes from its
/// landscape origin (248, 28) down by the layout's top part (`shift_of`). Stacked, the left
/// column (x < 247) has no place of its own any more (the tabs and the unit band have their own
/// geometry); its old remap is kept. Landscape positions are unchanged.
pub fn place_in(st: Stack, x: f32, y: f32, portrait: bool) -> (f32, f32) {
    if !portrait {
        (x, y)
    } else if x >= 247.0 {
        (x - 246.0, y + shift_of(st))
    } else {
        (x + 171.0, y)
    }
}

fn place(x: f32, y: f32, portrait: bool) -> (f32, f32) {
    place_in(stack(), x, y, portrait)
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

fn left_in(win: Rect, k: f32, portrait: bool) -> Rect {
    if portrait {
        return band_halves(army_band_in(win, k)).0;
    }
    rect_in(win, k, 2.0, 27.0, 244.0, 571.0, portrait)
}
fn upper_in(win: Rect, k: f32, portrait: bool) -> Rect { rect_in(win, k, 248.0, 28.0, 586.0, upper_height(), portrait) }
fn divider_in(win: Rect, k: f32, portrait: bool) -> Rect { rect_in(win, k, 248.0, 28.0 + upper_height(), 586.0, DIVIDER, portrait) }
fn lower_in(win: Rect, k: f32, portrait: bool) -> Rect { rect_in(win, k, 248.0, lower_y(), 586.0, lower_height(), portrait) }

/// The window's scale when it is stacked: the largest that fits the screen's width and the
/// height above the bottom bar, never below the interface scale. Called outside the window's
/// own scale ([`scaled`]).
fn portrait_k_for(st: Stack) -> f32 {
    stacked_scale(screen_width(), screen_height(), chrome::bar_height(), height_of(st), chrome::k())
}

/// The stacked scale for a `screen_w`×`screen_h` screen with a `bar`-px bottom bar: the width's
/// scale (`screen_w / PW`) or the height's (above the bar), whichever is smaller, never below `min_k`.
fn stacked_scale(screen_w: f32, screen_h: f32, bar: f32, height: f32, min_k: f32) -> f32 {
    let fit = (screen_w / PW).min((screen_h - bar - 4.0) / height);
    fit.max(min_k)
}

/// Runs `f` (a building, army or hero window's whole frame) at the stacked window's own scale
/// and with its stacked layout, when the phone is held upright; anywhere else `f` runs as it is.
pub fn scaled<R>(st: Stack, f: impl FnOnce() -> R) -> R {
    if portrait() {
        let k = portrait_k_for(st);
        within(st, || super::display::with_window_scale(Some(k), f))
    } else {
        f()
    }
}

/// Margin of a title window from the screen's sides, px (its width is the screen's less this
/// on each side).
const TITLE_MARGIN: f32 = 8.0;
/// The title windows' bottom sits this many px (at the interface scale) above the footer's
/// baseline, so the version and copyright lines stay clear of it.
const TITLE_FOOT_GAP: f32 = 16.0;

/// The scale and the rectangle of a title window (`video`: its size in video px, 960×720 frame)
/// on a phone held upright: the largest scale that spans the screen's width less
/// [`TITLE_MARGIN`] and fits between the top safe inset `top` and the footer, never below
/// `base_k` (the interface scale); centred horizontally, and vertically between the inset and
/// the footer. `sw` × `sh` is the screen, `dp` the phone's px per dp (`None` on the desktop,
/// and on a screen that is not taller than wide: those get None and are not moved).
pub fn portrait_title(screen: (f32, f32), dp: Option<f32>, top: f32, base_k: f32, video: (f32, f32)) -> Option<(f32, Rect)> {
    let ((sw, sh), dp) = (screen, dp.filter(|d| *d > 0.0)?);
    if sh <= sw {
        return None;
    }
    let bottom = super::main_menu::portrait_foot_top(sw, sh, base_k, dp) - TITLE_FOOT_GAP * base_k;
    let (vw, vh) = video;
    let fit_w = (sw - 2.0 * TITLE_MARGIN) / vw;
    let fit_h = (bottom - top - 2.0 * TITLE_MARGIN) / vh;
    let k = fit_w.min(fit_h).max(base_k);
    let (w, h) = ((vw * k).round(), (vh * k).round());
    let y = (top + ((bottom - top) - h) / 2.0).max(top).round();
    Some((k, Rect::new(((sw - w) / 2.0).round(), y, w, h)))
}

/// The interface scale (`chrome::k` outside any window's scale).
fn interface_k() -> f32 {
    super::display::scale(super::chrome::fit_k())
}

/// [`portrait_title`] for the phone on screen now (`None` where it is not held upright).
fn portrait_title_now(video: (f32, f32)) -> Option<(f32, Rect)> {
    portrait_title((screen_width(), screen_height()), super::display::dp_px(), super::display::safe_area().top, interface_k(), video)
}

/// The window scale of a title window of `video` size, as a guard: set until it drops. A
/// screen that is not held upright keeps the scale it has (none on the title screens).
pub fn title_scale(video: (f32, f32)) -> super::display::WindowScale {
    let k = portrait_title_now(video).map(|(k, _)| k);
    super::display::enter_window_scale(k.or_else(super::display::window_scale))
}

/// The rectangle of a title window of `video` size (video px, at its own scale) on a phone held
/// upright; `None` elsewhere, where the window keeps its own placement.
pub fn title_rect(video: (f32, f32)) -> Option<Rect> {
    portrait_title_now(video).map(|(_, r)| r)
}

/// The bottom edge the title screens' windows may reach: the footer's top on a phone held
/// upright, else the screen's bottom.
pub fn title_bottom() -> f32 {
    match super::display::dp_px() {
        Some(dp) if screen_height() > screen_width() => {
            let k = interface_k();
            super::main_menu::portrait_foot_top(screen_width(), screen_height(), k, dp) - TITLE_FOOT_GAP * k
        }
        _ => screen_height(),
    }
}

/// The window's rectangle: the building and army windows' size at the interface scale (the
/// stacked one when the phone is held upright), centred over the view above the bottom bar.
pub fn window() -> Rect {
    let k = chrome::k();
    if portrait() {
        return centred((PW * k).round(), (height_of(stack()) * k).round());
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

/// The landscape left column (the tabs, the unit card). Stacked, it is the unit band's left half
/// (the army and hero windows' picture); the building's tabs use [`building_tabs`] instead.
pub fn left(win: Rect) -> Rect { left_in(win, chrome::k(), portrait()) }

/// The army and hero windows' unit band (stacked): under the title, the full width. Its left
/// half is the unit's picture with its worn items, its right half the name and the stats.
pub fn army_band(win: Rect) -> Rect { army_band_in(win, chrome::k()) }

fn army_band_in(win: Rect, k: f32) -> Rect {
    Rect::new(win.x + 2.0 * k, win.y + TOP * k, 586.0 * k, BAND_H * k)
}

/// The band's two halves: the picture (left) and the text (right).
pub fn band_halves(band: Rect) -> (Rect, Rect) {
    // The picture takes 40 % and the text 60 %: the text half wider than the landscape column, so
    // its text grows with it (the stat lines already fill the column's width at its size).
    let w = (band.w * 0.4).round();
    (Rect::new(band.x, band.y, w, band.h), Rect::new(band.x + w, band.y, band.w - w, band.h))
}

/// The building window's tab row (stacked): `n` tabs of equal width side by side across the
/// window, 64 px tall, at the window's bottom with Exit under them.
pub fn building_tabs(win: Rect, n: usize) -> Vec<Rect> { building_tabs_in(win, chrome::k(), n) }

fn building_tabs_in(win: Rect, k: f32, n: usize) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    let n_f = n as f32;
    let w = (586.0 - TAB_GAP * (n_f - 1.0)) / n_f;
    (0..n)
        .map(|i| {
            let x = 2.0 + i as f32 * (w + TAB_GAP);
            Rect::new(win.x + x * k, win.y + tabs_top() * k, w * k, TAB_H * k)
        })
        .collect()
}

/// The building window's Exit (stacked): 40% of the width, centred under the tab row, the same
/// height as the tabs.
pub fn building_exit(win: Rect) -> Rect { building_exit_in(win, chrome::k()) }

fn building_exit_in(win: Rect, k: f32) -> Rect {
    let w = 586.0 * EXIT_SHARE;
    let x = 2.0 + (586.0 - w) / 2.0;
    Rect::new(win.x + x * k, win.y + (building_ph() - EDGE - TAB_H) * k, w * k, TAB_H * k)
}

/// The army and hero windows' «Закрыть» button (stacked): under the formation, in thumb reach; the
/// title's cross stays too. Same size and width as the building's Exit.
pub fn army_close(win: Rect) -> Rect { army_close_in(win, chrome::k()) }

fn army_close_in(win: Rect, k: f32) -> Rect {
    let w = 586.0 * EXIT_SHARE;
    let x = 2.0 + (586.0 - w) / 2.0;
    Rect::new(win.x + x * k, win.y + (BAND_H + CLOSE_Y) * k, w * k, TAB_H * k)
}

pub fn upper(win: Rect) -> Rect { upper_in(win, chrome::k(), portrait()) }
pub fn divider(win: Rect) -> Rect { divider_in(win, chrome::k(), portrait()) }
pub fn lower(win: Rect) -> Rect { lower_in(win, chrome::k(), portrait()) }
pub fn lower_content(win: Rect) -> Rect { let r=lower(win); let k=chrome::k(); Rect::new(r.x+8.0*k,r.y+8.0*k,r.w-14.0*k,r.h-16.0*k) }

/// The stacked army window's seam: the 2 px line under the unit band, across the window (drawn
/// by [`frame`]; a window that does not call `frame` draws it itself when stacked).
pub fn seam(win: Rect) {
    if !portrait() || stack() != Stack::Army {
        return;
    }
    let k = chrome::k();
    let px = chrome::opx();
    let y = win.y + (TOP + BAND_H) * k;
    chrome::silver_line(vec2(win.x + 2.0 * k, y), vec2(win.right() - 2.0 * px, y), 2.0 * px);
}

/// The army and building windows' lines, drawn last so tab artwork cannot cover them: the
/// window's frame and title line (the 896×640 window made by 0x48d710 without inner frames,
/// 4dc126), the 2 px line between the left column and the right side from under the title
/// line (y 29) to the frame, and the 2 px lines over and under the hint strip across the
/// right side (4cd330, 4bb7c4: (264, 29)–(266, 638), (266, 319)–(894, 321),
/// (266, 346)–(894, 348) of the 896×640 window; Razdor's 836×600 layout puts them at its own
/// column and strip). Stacked, the strip's lines run across the window, and the army windows'
/// seam lies under the band ([`seam`]).
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

    fn stacked_win() -> Rect { Rect::new(10.0, 20.0, 590.0, building_ph()) }

    #[test]
    fn stacked_places_the_right_side_under_the_top_part() {
        assert_eq!(place_in(Stack::Building, 300.0, 100.0, true), (54.0, 100.0), "the building's panes stay in their rows");
        assert_eq!(place_in(Stack::Army, 300.0, 100.0, true), (54.0, 100.0 + BAND_H), "the army's right side is under the band");
        assert_eq!(place_in(Stack::Building, 300.0, 100.0, false), (300.0, 100.0), "landscape is unchanged");
        assert_eq!(place_in(Stack::Army, 100.0, 100.0, false), (100.0, 100.0));
    }

    #[test]
    fn landscape_helpers_keep_their_geometry() {
        let w = win();
        assert_eq!(left_in(w, 1.0, false), Rect::new(12.0, 47.0, 244.0, 571.0));
        assert_eq!(upper_in(w, 1.0, false), Rect::new(258.0, 48.0, 586.0, upper_height()));
        let d = divider_in(w, 1.0, false);
        assert_eq!((d.x, d.y, d.w), (258.0, 20.0 + 28.0 + upper_height(), 586.0));
        assert_eq!(lower_in(w, 1.0, false).y, 20.0 + lower_y());
        assert_eq!(lower_in(w, 1.0, false).h, lower_height(), "landscape's lower pane keeps its height");
        assert_eq!(at_in(w, 1.0, 300.0, 100.0, 10.0, 10.0, false).x, 10.0 + 300.0);
    }

    #[test]
    fn stacked_building_panes_stack_and_the_lower_one_keeps_the_landscape_height() {
        let w = stacked_win();
        let p = true;
        assert_eq!(within(Stack::Building, || rect_in(w, 1.0, 248.0, 28.0, 586.0, upper_height(), p)), Rect::new(12.0, 48.0, 586.0, upper_height()));
        let lower = within(Stack::Building, || lower_in(w, 1.0, p));
        assert_eq!(lower.y, 20.0 + lower_y());
        assert_eq!(lower.h, lower_height(), "the lower pane keeps the landscape height");
        let tabs = building_tabs_in(w, 1.0, 1);
        assert!((tabs[0].y - (lower.bottom() + LOWER_GAP)).abs() < 0.01, "the tab row is LOWER_GAP under the lower pane");
    }

    #[test]
    fn building_tabs_are_a_row_of_big_touch_targets_under_the_panes() {
        let w = stacked_win();
        let lower_bottom = w.y + tabs_top() - LOWER_GAP;
        for n in 1..=5 {
            let tabs = building_tabs_in(w, 1.0, n);
            assert_eq!(tabs.len(), n);
            for t in &tabs {
                assert!(t.h >= 64.0, "a touch target: {} px", t.h);
                assert!(t.y >= lower_bottom + LOWER_GAP - 0.01);
                assert!(t.x >= w.x + 2.0 && t.right() <= w.right() - 2.0, "inside the window: {t:?}");
                assert_eq!(t.y, tabs[0].y, "one row");
                assert_eq!(t.w, tabs[0].w, "equal widths");
            }
            for pair in tabs.windows(2) {
                assert!((pair[1].x - pair[0].right() - TAB_GAP).abs() < 0.01, "a small gap between tabs");
            }
        }
        assert!(building_tabs_in(w, 1.0, 0).is_empty());
    }

    #[test]
    fn building_exit_is_centred_under_the_tab_row_at_the_bottom() {
        let w = stacked_win();
        let exit = building_exit_in(w, 1.0);
        assert_eq!(exit.h, TAB_H, "the same height as the tabs");
        assert!((exit.w - 586.0 * 0.4).abs() < 0.01, "40% of the width");
        assert_eq!(exit.x + exit.w / 2.0, w.x + 2.0 + 586.0 / 2.0, "centred");
        assert_eq!(exit.bottom(), w.bottom() - EDGE, "at the window's bottom");
        let tabs = building_tabs_in(w, 1.0, 3);
        assert!(exit.y >= tabs[0].bottom(), "under the tab row");
    }

    #[test]
    fn stacked_army_band_fills_the_width_and_the_right_side_follows_it() {
        let w = Rect::new(10.0, 20.0, 590.0, ARMY_PH);
        let band = army_band_in(w, 1.0);
        assert_eq!((band.x, band.y, band.w), (12.0, 48.0, 586.0));
        assert_eq!(band.h, BAND_H, "the band's height");
        let (pic, txt) = band_halves(band);
        assert_eq!((pic.w, txt.w), (234.0, 352.0));
        assert_eq!((pic.x, txt.x), (12.0, 246.0));
        let right = within(Stack::Army, || point_in(w, 1.0, 248.0, 28.0, true));
        assert_eq!(right.y, 20.0 + 28.0 + BAND_H, "the right side starts under the band");
        let bottom = within(Stack::Army, || point_in(w, 1.0, 248.0, 599.4, true));
        assert!(bottom.y - w.y <= ARMY_PH, "the right side fits the window");
        assert_eq!(left_in(w, 1.0, true), pic, "the left column is the picture half of the band");
        let close = army_close_in(w, 1.0);
        assert!(close.y >= w.y + BAND_H + 600.0, "the close button is under the formation");
        assert!(close.bottom() - w.y <= ARMY_PH, "and inside the window: {}", close.bottom() - w.y);
    }

    #[test]
    fn stacked_army_window_is_width_limited_on_a_phone() {
        // 1080x2376 at 2.75 dp: the window spans the width (scale 1080 / 590), with the map
        // above and below it; the bottom bar takes its own height at the bottom.
        let (sw, sh, bar) = (1080.0, 2376.0, 90.0);
        let k = stacked_scale(sw, sh, bar, ARMY_PH, 1.0);
        assert!((k - sw / PW).abs() < 1e-4, "width-limited: {k}");
        let h = (ARMY_PH * k).round();
        assert!(h <= sh - bar - 4.0 - 2.0 * 100.0, "the map shows above and below: {h}");
        assert!(h <= sh - bar - 4.0);
        // A screen too short for the window falls back to the height's scale.
        let k_short = stacked_scale(sw, 1600.0, bar, ARMY_PH, 1.0);
        assert!(k_short < sw / PW, "height-limited on a short screen: {k_short}");
    }

    #[test]
    fn stacked_helpers_move_the_halves() {
        let w = win();
        let a = within(Stack::Army, || at_in(w, 1.0, 300.0, 100.0, 10.0, 10.0, true));
        // y 100 is in the upper pane: mapped by its scale first, then moved down by the band.
        let yy = 28.0 + (100.0 - 28.0) * upper_height() / 272.0;
        assert_eq!((a.x, a.y), (10.0 + 54.0, 20.0 + yy + BAND_H));
        assert_eq!(at_in(w, 1.0, 100.0, 100.0, 10.0, 10.0, true).x, 10.0 + 271.0, "the old left remap");
    }
}

#[cfg(test)]
mod title_tests {
    use super::*;

    /// The interface scale of a 1080-wide phone (`chrome::fit_k`): 1080 / 1024 over the 0.9375 reference.
    const BASE: f32 = 1.125;
    /// The video sizes of the title windows: the scenario list and the new-hero, load, authors,
    /// settings (its height with the Android row), «Ещё…», keys and custom battle windows.
    const VIDEOS: [(f32, f32); 8] = [(594.0, 482.0), (594.0, 480.0), (594.0, 576.0), (594.0, 632.0), (900.0, 650.0), (760.0, 420.0), (632.0, 516.0), (900.0, 600.0)];

    fn foot_bottom(sw: f32, sh: f32, dp: f32) -> f32 {
        super::super::main_menu::portrait_foot_top(sw, sh, BASE, dp) - TITLE_FOOT_GAP * BASE
    }

    #[test]
    fn a_title_window_spans_the_phone_held_upright_between_the_inset_and_the_footer() {
        for (sw, sh) in [(1080.0, 2376.0), (1080.0, 1920.0)] {
            let dp = 2.75;
            let top = 66.0;
            let bottom = foot_bottom(sw, sh, dp);
            for video in VIDEOS {
                let (k, r) = portrait_title((sw, sh), Some(dp), top, BASE, video).expect("held upright");
                assert!(r.w >= 0.95 * sw, "{video:?} on {sw}x{sh}: width {} of {sw}", r.w);
                assert!(r.x >= 0.0 && r.x + r.w <= sw, "{video:?} off the sides: {r:?}");
                assert!(r.y >= top && r.y + r.h <= bottom + 0.5, "{video:?} off the inset or the footer: {r:?} bottom {bottom}");
                assert!(k >= BASE, "never below the interface scale");
                assert!((r.w - video.0 * k).abs() <= 1.0, "the window keeps its proportions");
            }
        }
    }

    #[test]
    fn a_title_window_is_centred_between_the_inset_and_the_footer() {
        let (sw, sh, dp, top) = (1080.0, 2376.0, 2.75, 66.0);
        let bottom = foot_bottom(sw, sh, dp);
        let (_, r) = portrait_title((sw, sh), Some(dp), top, BASE, (594.0, 482.0)).expect("held upright");
        let space = (top + bottom) / 2.0;
        assert!((r.center().y - space).abs() <= 1.0, "centre {} against {space}", r.center().y);
    }

    #[test]
    fn the_scenario_list_is_width_limited_on_the_phone_at_about_1_8_times() {
        let (k, r) = portrait_title((1080.0, 2376.0), Some(2.75), 66.0, BASE, (594.0, 482.0)).expect("held upright");
        assert!((k - 1064.0 / 594.0).abs() < 0.01, "k {k}");
        assert_eq!(r.w.round(), 1064.0);
    }

    #[test]
    fn a_landscape_phone_and_the_desktop_keep_their_windows() {
        assert_eq!(portrait_title((2376.0, 1080.0), Some(2.75), 0.0, BASE, (594.0, 482.0)), None);
        assert_eq!(portrait_title((1920.0, 1080.0), None, 0.0, BASE, (594.0, 482.0)), None);
        assert_eq!(portrait_title((1080.0, 2376.0), None, 0.0, BASE, (594.0, 482.0)), None);
    }
}
