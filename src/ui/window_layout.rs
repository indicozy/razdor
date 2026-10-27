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
pub const BAND_H: f32 = 420.0;
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

/// The window scale of a title window of `video` size on a phone held sideways (a phone choice, not in
/// the original: the windows were too small to use with a finger, as the exit windows were,
/// [`sideways_exit_k`]): one original px per dp, never above the fit to the screen's width less the
/// safe area's sides and to the height between the top inset and the bar, never below `base_k`. None
/// off the phone (`dp` None) and with the large windows off: the scale stays as it is.
pub fn sideways_title_k(base_k: f32, dp: Option<f32>, large: bool, (sw, sh): (f32, f32), bar: f32, safe: super::display::SafeArea, video: (f32, f32)) -> Option<f32> {
    dp?;
    if !large {
        return None;
    }
    let avail = (sw - safe.left - safe.right, sh - safe.top);
    Some(super::display::popup_scale(base_k, large, dp, video, avail, bar))
}

/// A window of `video` size on a phone held sideways, as a title window: its scale ([`sideways_title_k`])
/// and its rectangle, centred between the safe area's sides and between the top inset and the `bar`
/// (as an exit window, [`exit_rect`]). None where [`sideways_title_k`] is.
pub fn sideways_window(base_k: f32, dp: Option<f32>, large: bool, screen: (f32, f32), bar: f32, safe: super::display::SafeArea, video: (f32, f32)) -> Option<(f32, Rect)> {
    let k = sideways_title_k(base_k, dp, large, screen, bar, safe, video)?;
    let (w, h) = ((video.0 * k).round(), (video.1 * k).round());
    Some((k, exit_rect(screen, bar, safe, (w, h))))
}

/// [`sideways_window`] for the phone on screen now (None unless it is held sideways).
fn sideways_now(video: (f32, f32)) -> Option<(f32, Rect)> {
    if portrait() {
        return None;
    }
    sideways_window(chrome::k(), super::display::dp_px(), super::display::large_windows(), (screen_width(), screen_height()), chrome::bar_height(), super::display::safe_area(), video)
}

/// [`sideways_window`] for a window over the map: it never reaches the bottom bar. The title
/// rule's floor (the base scale) may make it taller than the room between the top inset and the
/// bar; then it scales down to that room (its bottom on the bar's top edge), centred there as
/// [`exit_rect`] places it. A window that fits keeps [`sideways_window`]'s scale and rectangle.
pub fn sideways_game_window(base_k: f32, dp: Option<f32>, large: bool, screen: (f32, f32), bar: f32, safe: super::display::SafeArea, video: (f32, f32)) -> Option<(f32, Rect)> {
    let (k, r) = sideways_window(base_k, dp, large, screen, bar, safe, video)?;
    let room = screen.1 - safe.top - bar;
    if r.h <= room {
        return Some((k, r));
    }
    let h = room.floor();
    let k = h / video.1;
    Some((k, exit_rect(screen, bar, safe, ((video.0 * k).round(), h))))
}

/// [`sideways_game_window`] for the phone on screen now (None unless it is held sideways).
fn sideways_game_now(video: (f32, f32)) -> Option<(f32, Rect)> {
    if portrait() {
        return None;
    }
    sideways_game_window(chrome::k(), super::display::dp_px(), super::display::large_windows(), (screen_width(), screen_height()), chrome::bar_height(), super::display::safe_area(), video)
}

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
    let bottom = super::main_menu::portrait_foot_top(sw, sh, base_k, dp, top) - TITLE_FOOT_GAP * base_k;
    Some(fit_between(sw, top, bottom, base_k, video))
}

/// The gap between a window over the map and the top of the bottom bar, at the interface scale.
const GAME_BAR_GAP: f32 = 8.0;

/// The scale and the rectangle of a window over the map (a settings, save, load or game menu
/// window, opened in game) on a phone held upright: as [`portrait_title`], but between the top
/// safe inset `top` and the top of the bottom bar, `bar` px tall at the interface scale (the bar
/// is not scaled with the window). `None` where the phone is not held upright.
pub fn portrait_game(screen: (f32, f32), dp: Option<f32>, top: f32, bar: f32, base_k: f32, video: (f32, f32)) -> Option<(f32, Rect)> {
    let ((sw, sh), _) = (screen, dp.filter(|d| *d > 0.0)?);
    if sh <= sw {
        return None;
    }
    Some(fit_between(sw, top, sh - bar - GAME_BAR_GAP * base_k, base_k, video))
}

/// The largest scale (never below `base_k`) at which a `video` window spans `sw` less the margins
/// and fits between `top` and `bottom`, with its rectangle centred there (both px).
fn fit_between(sw: f32, top: f32, bottom: f32, base_k: f32, video: (f32, f32)) -> (f32, Rect) {
    let (vw, vh) = video;
    let fit_w = (sw - 2.0 * TITLE_MARGIN) / vw;
    let fit_h = (bottom - top - 2.0 * TITLE_MARGIN) / vh;
    let k = fit_w.min(fit_h).max(base_k);
    let (w, h) = ((vw * k).round(), (vh * k).round());
    let y = (top + ((bottom - top) - h) / 2.0).max(top).round();
    (k, Rect::new(((sw - w) / 2.0).round(), y, w, h))
}

/// The interface scale (`chrome::k` outside any window's scale).
fn interface_k() -> f32 {
    super::display::scale(super::chrome::fit_k())
}

/// [`portrait_title`] for the phone on screen now (`None` where it is not held upright).
fn portrait_title_now(video: (f32, f32)) -> Option<(f32, Rect)> {
    portrait_title((screen_width(), screen_height()), super::display::dp_px(), super::display::safe_area().top, interface_k(), video)
}

/// The window scale of a title window of `video` size, as a guard: set until it drops. On a phone
/// held upright it is [`portrait_title`]'s; held sideways [`sideways_title_k`]'s. Any other screen
/// keeps the scale it has (none on the title screens).
pub fn title_scale(video: (f32, f32)) -> super::display::WindowScale {
    let k = portrait_title_now(video).map(|(k, _)| k).or_else(|| sideways_now(video).map(|(k, _)| k));
    super::display::enter_window_scale(k.or_else(super::display::window_scale))
}

/// The rectangle of a title window of `video` size (video px, at its own scale) on a phone held
/// upright, or sideways (centred between the safe area's sides and the top inset and the bar, as an
/// exit window: [`exit_rect`]); `None` elsewhere, where the window keeps its own placement.
pub fn title_rect(video: (f32, f32)) -> Option<Rect> {
    portrait_title_now(video).map(|(_, r)| r).or_else(|| sideways_now(video).map(|(_, r)| r))
}

/// [`portrait_game`] for the phone on screen now (`None` where it is not held upright).
fn portrait_game_now(video: (f32, f32)) -> Option<(f32, Rect)> {
    portrait_game((screen_width(), screen_height()), super::display::dp_px(), super::display::safe_area().top, chrome::bar_height(), interface_k(), video)
}

/// The window scale of a window over the map of `video` size, as a guard (see [`title_scale`]):
/// on a phone held upright it is the largest that fits between the top inset and the bar.
pub fn game_scale(video: (f32, f32)) -> super::display::WindowScale {
    let k = portrait_game_now(video).map(|(k, _)| k);
    super::display::enter_window_scale(k.or_else(super::display::window_scale))
}

/// The rectangle of a window over the map of `video` size (see [`game_scale`]); `None` elsewhere.
pub fn game_rect(video: (f32, f32)) -> Option<Rect> {
    portrait_game_now(video).map(|(_, r)| r)
}

/// The window scale of a window over the map that has a title-screen twin (the save and load books,
/// the settings' «Ещё…», the keys window, the settings over the map): [`game_scale`] on a phone held
/// upright; held sideways, the title windows' scale ([`sideways_now`]), so it is as large as the
/// same window from the main menu (a phone choice, not in the original: the windows were small in
/// landscape). The desktop and the other windows over the map ([`game_scale`]) keep theirs.
pub fn over_map_scale(video: (f32, f32)) -> super::display::WindowScale {
    let k = portrait_game_now(video).map(|(k, _)| k).or_else(|| sideways_game_now(video).map(|(k, _)| k));
    super::display::enter_window_scale(k.or_else(super::display::window_scale))
}

/// The rectangle of a window over the map with a title-screen twin (see [`over_map_scale`]): upright
/// as [`game_rect`], sideways as the title's, never over the bar ([`sideways_game_now`]); `None` on the desktop.
pub fn over_map_rect(video: (f32, f32)) -> Option<Rect> {
    portrait_game_now(video).map(|(_, r)| r).or_else(|| sideways_game_now(video).map(|(_, r)| r))
}

/// The design size of the spell book on a phone held upright (`w` px wide: two columns of cards), as
/// tall as the screen takes at that width between the top inset `top` and the bar `bar`, so its rows
/// fill the height (a phone choice, not in the original). `screen`, `dp` and `base_k` as in
/// [`portrait_game`]; None where the phone is not held upright.
pub fn upright_book_video_at(screen: (f32, f32), dp: Option<f32>, top: f32, bar: f32, base_k: f32, w: f32) -> Option<(f32, f32)> {
    let ((sw, sh), _) = (screen, dp.filter(|d| *d > 0.0)?);
    if sh <= sw {
        return None;
    }
    let bottom = sh - bar - GAME_BAR_GAP * base_k;
    let k = ((sw - 2.0 * TITLE_MARGIN) / w).max(base_k);
    Some((w, ((bottom - top - 2.0 * TITLE_MARGIN) / k).floor()))
}

/// [`upright_book_video_at`] for the phone on screen now.
pub fn upright_book_video(w: f32) -> Option<(f32, f32)> {
    upright_book_video_at((screen_width(), screen_height()), super::display::dp_px(), super::display::safe_area().top, chrome::bar_height(), interface_k(), w)
}

/// The exit windows' size in original px: the game's «Выход из игры» and the battle's leave
/// window (634×516, the original's pixels; drawn at `chrome::opx`, 0.9375 of the window scale).
pub const EXIT_ORIG: (f32, f32) = (634.0, 516.0);

/// The window scale of an exit window on a phone held sideways (the owner's and a player's
/// request: the windows were too small to use with a finger): the large-window scale of
/// `display::popup_scale` for [`EXIT_ORIG`], one original px per dp and never above the fit to
/// the screen's width less the safe area's sides, and to the height between the top inset and the
/// bar. `base_k` is the window scale it has now; the result is never below it. `None` off the
/// phone (`dp` None): the desktop keeps its own scale. Large windows off: `base_k`.
pub fn sideways_exit_k(base_k: f32, dp: Option<f32>, large: bool, (sw, sh): (f32, f32), bar: f32, safe: super::display::SafeArea) -> Option<f32> {
    dp?;
    let avail = (sw - safe.left - safe.right, sh - safe.top);
    let o = super::display::popup_scale(base_k * 0.9375, large, dp, EXIT_ORIG, avail, bar);
    Some(o / 0.9375)
}

/// The rectangle of an exit window of `(w, h)` px on a screen `(sw, sh)` with a `bar`-px bottom
/// bar and the safe area `safe`: centred between the sides' insets, and vertically between the
/// top inset and the bar (not above the top). On the desktop (no insets) it is the window's
/// old centring.
pub fn exit_rect((sw, sh): (f32, f32), bar: f32, safe: super::display::SafeArea, (w, h): (f32, f32)) -> Rect {
    let x = safe.left + (sw - safe.left - safe.right - w) / 2.0;
    let y = safe.top + ((sh - safe.top - bar - h) / 2.0).max(0.0);
    Rect::new(x, y, w, h)
}

/// The window scale of an exit window (the game's «Выход из игры», the battle's leave window)
/// for the screen now: on a phone held upright [`game_scale`]; on a phone held sideways
/// [`sideways_exit_k`]; on the desktop the scale it has (none set: the interface scale).
pub fn exit_scale() -> super::display::WindowScale {
    if portrait() {
        return game_scale((EXIT_ORIG.0 * 0.9375, EXIT_ORIG.1 * 0.9375));
    }
    let k = sideways_exit_k(
        chrome::k(),
        super::display::dp_px(),
        super::display::large_windows(),
        (screen_width(), screen_height()),
        chrome::bar_height(),
        super::display::safe_area(),
    );
    super::display::enter_window_scale(k.or_else(super::display::window_scale))
}

/// [`title_scale`] for a window opened from the title screens, or [`over_map_scale`] for one over
/// the map (`in_game`): the same guard either way, so a window's frame reads one place.
pub fn window_scale(in_game: bool, video: (f32, f32)) -> super::display::WindowScale {
    if in_game { over_map_scale(video) } else { title_scale(video) }
}

/// [`title_rect`] or [`over_map_rect`] by where the window is opened.
pub fn window_rect(in_game: bool, video: (f32, f32)) -> Option<Rect> {
    if in_game { over_map_rect(video) } else { title_rect(video) }
}

/// The bottom edge the title screens' windows may reach: the footer's top on a phone held
/// upright, else the screen's bottom.
pub fn title_bottom() -> f32 {
    match super::display::dp_px() {
        Some(dp) if screen_height() > screen_width() => {
            let k = interface_k();
            super::main_menu::portrait_foot_top(screen_width(), screen_height(), k, dp, super::display::safe_area().top) - TITLE_FOOT_GAP * k
        }
        _ => screen_height(),
    }
}

/// How far (up to 0 px) a window moves so that it sits above the on-screen keyboard: its bottom
/// goes up to the keyboard's top edge (`screen_h` - `keyboard`, physical px), never above `top`
/// (the safe area's top). A window taller than the space left (`top` to the keyboard's edge)
/// moves so that its field `field` is at the top, which keeps the field in sight (the confirm
/// buttons may be under the keyboard then). No keyboard, or a window already above it: 0.
pub fn keyboard_lift(win: Rect, field: Rect, screen_h: f32, keyboard: f32, top: f32) -> f32 {
    if keyboard <= 0.0 {
        return 0.0;
    }
    let limit = screen_h - keyboard;
    if win.bottom() <= limit {
        return 0.0;
    }
    let dy = if win.h <= limit - top { (limit - win.bottom()).max(top - win.y) } else { top - field.y };
    dy.min(0.0)
}

/// `win` moved by [`keyboard_lift`] for the keyboard the phone reports now (`display::keyboard_inset`),
/// with `field` the window's name field at its unmoved place. Desktop and a hidden keyboard: `win`.
pub fn keyboard_lifted(win: Rect, field: Rect) -> Rect {
    let dy = keyboard_lift(win, field, screen_height(), super::display::keyboard_inset(), super::display::safe_area().top);
    Rect::new(win.x, win.y + dy, win.w, win.h)
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
    // The caller's window is the one it gave `chrome::window_with_title_color`: on a phone that
    // strip grew upward, and the frame follows it.
    chrome::window_frames_titled(win, px);
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

    /// A 1080x2376 phone with a 900 px keyboard: the keyboard's top edge is at 1476 px.
    const SCREEN_H: f32 = 2376.0;
    const KB: f32 = 900.0;

    #[test]
    fn a_window_that_fits_above_the_keyboard_moves_up_until_its_bottom_meets_it() {
        // Bottom at 1500 px, 24 px under the keyboard's top: it moves up 24 px and fits.
        let w = Rect::new(100.0, 900.0, 800.0, 600.0);
        let field = Rect::new(300.0, 1000.0, 400.0, 60.0);
        let dy = keyboard_lift(w, field, SCREEN_H, KB, 0.0);
        assert_eq!(dy, -24.0);
        assert_eq!(w.bottom() + dy, SCREEN_H - KB);
    }

    #[test]
    fn a_window_above_the_keyboard_or_with_no_keyboard_does_not_move() {
        let w = Rect::new(100.0, 300.0, 800.0, 600.0); // bottom 900: above 1476
        let field = Rect::new(300.0, 400.0, 400.0, 60.0);
        assert_eq!(keyboard_lift(w, field, SCREEN_H, KB, 0.0), 0.0);
        let low = Rect::new(100.0, 1200.0, 800.0, 600.0);
        assert_eq!(keyboard_lift(low, field, SCREEN_H, 0.0, 0.0), 0.0, "no keyboard: no move");
    }

    #[test]
    fn a_window_taller_than_the_space_puts_its_field_at_the_top() {
        // 1500 px tall; the space above the keyboard below the safe top (100) is 1376 px, so the
        // field (y 900) goes to the top and the window moves with it.
        let w = Rect::new(100.0, 700.0, 800.0, 1500.0);
        let field = Rect::new(300.0, 900.0, 400.0, 60.0);
        let dy = keyboard_lift(w, field, SCREEN_H, KB, 100.0);
        assert_eq!(dy, -800.0);
        assert_eq!(field.y + dy, 100.0);
    }

    #[test]
    fn a_window_that_fits_never_goes_above_the_safe_top() {
        // Bottom 2000, the space under the safe top (200) to the keyboard is 1276 px: 1000 px fits.
        let w = Rect::new(100.0, 1000.0, 800.0, 1000.0);
        let dy = keyboard_lift(w, w, SCREEN_H, KB, 200.0);
        assert_eq!(dy, -524.0);
        assert!(w.y + dy >= 200.0);
    }

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
        let top = 66.0;
        super::super::main_menu::portrait_foot_top(sw, sh, BASE, dp, top) - TITLE_FOOT_GAP * BASE
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

    /// The bottom bar's height at the interface scale (`chrome::bar_height`: 80 design px).
    fn bar_of() -> f32 { (80.0 * BASE).round() }

    /// The in-game windows' bottom: the bar's top less the gap (`GAME_BAR_GAP`).
    fn bar_top(sh: f32) -> f32 { sh - bar_of() - GAME_BAR_GAP * BASE }

    #[test]
    fn an_in_game_window_spans_the_phone_held_upright_between_the_inset_and_the_bar() {
        // The game's windows: the settings, its «Ещё…» and keys, the save and load windows, the game menu (its art 634×516 at 0.9375).
        let videos = VIDEOS.into_iter().chain([(634.0 * 0.9375, 516.0 * 0.9375)]);
        for (sw, sh) in [(1080.0, 2376.0), (1080.0, 1920.0)] {
            let (dp, top) = (2.75, 66.0);
            let bottom = bar_top(sh);
            for video in videos.clone() {
                let (k, r) = portrait_game((sw, sh), Some(dp), top, bar_of(), BASE, video).expect("held upright");
                assert!(r.w >= 0.95 * sw, "{video:?} on {sw}x{sh}: width {} of {sw}", r.w);
                assert!(r.x >= 0.0 && r.x + r.w <= sw, "{video:?} off the sides: {r:?}");
                assert!(r.y >= top && r.y + r.h <= bottom + 0.5, "{video:?} off the inset or the bar: {r:?} bottom {bottom}");
                assert!(k >= BASE, "never below the interface scale");
                assert!((r.w - video.0 * k).abs() <= 1.0, "the window keeps its proportions");
            }
        }
    }

    #[test]
    fn an_in_game_window_is_centred_between_the_inset_and_the_bar() {
        let (sw, sh, dp, top) = (1080.0, 2376.0, 2.75, 66.0);
        let (_, r) = portrait_game((sw, sh), Some(dp), top, bar_of(), BASE, (594.0, 482.0)).expect("held upright");
        let space = (top + bar_top(sh)) / 2.0;
        assert!((r.center().y - space).abs() <= 1.0, "centre {} against {space}", r.center().y);
    }

    #[test]
    fn the_in_game_settings_window_on_the_phone_is_width_limited() {
        // 594 × 606 (the settings window's height at the phone's rows): the width sets the scale.
        let (k, r) = portrait_game((1080.0, 2376.0), Some(2.75), 66.0, bar_of(), BASE, (594.0, 606.0)).expect("held upright");
        assert!((k - 1064.0 / 594.0).abs() < 0.01, "k {k}");
        assert_eq!(r.w.round(), 1064.0);
    }

    #[test]
    fn a_landscape_phone_and_the_desktop_keep_their_in_game_windows() {
        assert_eq!(portrait_game((2376.0, 1080.0), Some(2.75), 0.0, bar_of(), BASE, (594.0, 482.0)), None);
        assert_eq!(portrait_game((1920.0, 1080.0), None, 0.0, bar_of(), BASE, (594.0, 482.0)), None);
        assert_eq!(portrait_game((1080.0, 2376.0), None, 0.0, bar_of(), BASE, (594.0, 482.0)), None);
    }
}

#[cfg(test)]
mod exit_tests {
    use super::*;
    use super::super::display::SafeArea;

    const NO_SAFE: SafeArea = SafeArea { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0, corner_bl: 0.0, corner_br: 0.0, corner_tl: 0.0, corner_tr: 0.0 };
    /// A 2400×1080 phone held sideways at 2.625 px per dp, its bar 120 px tall; the interface
    /// scale of such a screen is 1.5 (`chrome::fit_k` over the 0.9375 reference).
    const SIDE: (f32, f32) = (2400.0, 1080.0);
    const BAR: f32 = 120.0;
    const BASE: f32 = 1.5;

    #[test]
    fn the_desktop_keeps_its_exit_window_scale_and_centring() {
        // No phone: no scale of its own, whatever the large windows setting says.
        assert_eq!(sideways_exit_k(BASE, None, true, SIDE, BAR, NO_SAFE), None);
        assert_eq!(sideways_exit_k(BASE, None, false, SIDE, BAR, NO_SAFE), None);
        // The desktop's rectangle is the old centring: (sw - w) / 2 across, and the height
        // between the top and the bar, never above the top.
        let (w, h) = (634.0 * 1.4, 516.0 * 1.4);
        let r = exit_rect((1920.0, 1080.0), 80.0, NO_SAFE, (w, h));
        assert_eq!(r.x, (1920.0 - w) / 2.0);
        assert_eq!(r.y, ((1080.0 - 80.0 - h) / 2.0).max(0.0));
        assert_eq!(exit_rect((1920.0, 300.0), 80.0, NO_SAFE, (w, h)).y, 0.0);
    }

    #[test]
    fn a_phone_held_sideways_gets_a_window_at_least_a_fifth_larger_that_fits() {
        let k = sideways_exit_k(BASE, Some(2.625), true, SIDE, BAR, NO_SAFE).expect("a phone");
        assert!(k >= 1.2 * BASE, "{k} is not 1.2 times the interface scale {BASE}");
        // The art (634×516 original px) fits the screen between the top and the bar.
        let (w, h) = (EXIT_ORIG.0 * k * 0.9375, EXIT_ORIG.1 * k * 0.9375);
        let r = exit_rect(SIDE, BAR, NO_SAFE, (w, h));
        assert!(r.x >= 0.0 && r.x + r.w <= SIDE.0 + 0.01, "{r:?} is wider than the screen");
        assert!(r.y >= 0.0 && r.y + r.h <= SIDE.1 - BAR + 0.01, "{r:?} is taller than the screen above the bar");
        assert!((r.x + r.w / 2.0 - SIDE.0 / 2.0).abs() < 0.01, "not centred across the screen");
    }

    #[test]
    fn the_sideways_window_stays_inside_the_safe_area_sides() {
        let safe = SafeArea { left: 110.0, right: 110.0, ..NO_SAFE };
        let k = sideways_exit_k(BASE, Some(2.625), true, SIDE, BAR, safe).expect("a phone");
        let (w, h) = (EXIT_ORIG.0 * k * 0.9375, EXIT_ORIG.1 * k * 0.9375);
        let r = exit_rect(SIDE, BAR, safe, (w, h));
        assert!(r.x >= safe.left - 0.01 && r.x + r.w <= SIDE.0 - safe.right + 0.01, "{r:?} is under the cutouts");
        assert!((r.x + r.w / 2.0 - SIDE.0 / 2.0).abs() < 0.01, "not centred between the cutouts");
    }

    #[test]
    fn large_windows_off_keep_the_phone_s_scale() {
        let k = sideways_exit_k(BASE, Some(2.625), false, SIDE, BAR, NO_SAFE).expect("a phone");
        assert!((k - BASE).abs() < 1e-4, "{k} is not the base {BASE}");
    }
}

#[cfg(test)]
mod sideways_title_tests {
    use super::*;
    use super::super::display::SafeArea;

    const NO_SAFE: SafeArea = SafeArea { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0, corner_bl: 0.0, corner_br: 0.0, corner_tl: 0.0, corner_tr: 0.0 };
    /// The phone held sideways, as the exit windows' test: 2400×1080 at 2.625 px per dp, its bar 120 px.
    const SIDE: (f32, f32) = (2400.0, 1080.0);
    const BAR: f32 = 120.0;
    const BASE: f32 = 1.5;
    /// The scenario list's video size (594 × 482).
    const SCENARIO: (f32, f32) = (594.0, 482.0);

    #[test]
    fn the_desktop_and_large_windows_off_keep_the_title_window_scale() {
        assert_eq!(sideways_title_k(BASE, None, true, (1920.0, 1080.0), 80.0, NO_SAFE, SCENARIO), None, "the desktop");
        assert_eq!(sideways_title_k(BASE, Some(2.625), false, SIDE, BAR, NO_SAFE, SCENARIO), None, "large windows off");
    }

    #[test]
    fn a_phone_held_sideways_scales_a_title_window_to_the_height_above_the_bar() {
        let k = sideways_title_k(BASE, Some(2.625), true, SIDE, BAR, NO_SAFE, SCENARIO).expect("a phone");
        // The height limits it: (1080 − 120 − 4) / 482 ≈ 1.98, below one px per dp (2.625).
        assert!((k - (1080.0 - BAR - 4.0) / 482.0).abs() < 0.01, "{k}");
        assert!(k >= 1.25 * BASE, "{k} is not at least a quarter larger than the base {BASE}");
        let r = exit_rect(SIDE, BAR, NO_SAFE, (SCENARIO.0 * k, SCENARIO.1 * k));
        assert!(r.x >= 0.0 && r.x + r.w <= SIDE.0 + 0.01, "{r:?} is wider than the screen");
        assert!(r.y >= 0.0 && r.y + r.h <= SIDE.1 - BAR + 0.01, "{r:?} is taller than the screen above the bar");
        assert!((r.x + r.w / 2.0 - SIDE.0 / 2.0).abs() < 0.01, "not centred across the screen");
    }

    #[test]
    fn a_small_title_window_takes_one_original_px_per_dp_when_the_screen_has_room() {
        // A 300 × 200 window on the phone: its width and height fit at one px per dp (2.625).
        let k = sideways_title_k(BASE, Some(2.625), true, SIDE, BAR, NO_SAFE, (300.0, 200.0)).expect("a phone");
        assert!((k - 2.625).abs() < 1e-4, "{k}");
    }

    #[test]
    fn a_sideways_title_window_stays_inside_the_safe_area() {
        let safe = SafeArea { left: 110.0, right: 110.0, top: 60.0, ..NO_SAFE };
        let k = sideways_title_k(BASE, Some(2.625), true, SIDE, BAR, safe, SCENARIO).expect("a phone");
        let r = exit_rect(SIDE, BAR, safe, (SCENARIO.0 * k, SCENARIO.1 * k));
        assert!(r.x >= safe.left - 0.01 && r.x + r.w <= SIDE.0 - safe.right + 0.01, "{r:?} is under the cutouts");
        assert!(r.y >= safe.top - 0.01 && r.y + r.h <= SIDE.1 - BAR + 0.01, "{r:?} is off the top inset or the bar");
    }
}

#[cfg(test)]
mod over_map_sideways_tests {
    use super::*;
    use super::super::display::SafeArea;

    const NO_SAFE: SafeArea = SafeArea { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0, corner_bl: 0.0, corner_br: 0.0, corner_tl: 0.0, corner_tr: 0.0 };
    /// The phone held sideways, as the exit and title tests: 2400×1080 at 2.625 px per dp, its bar 120 px.
    const SIDE: (f32, f32) = (2400.0, 1080.0);
    const BAR: f32 = 120.0;
    const BASE: f32 = 1.5;
    /// The in-game save and load books (594 × 482), and the other windows over the map with a title twin.
    const BOOK: (f32, f32) = (594.0, 482.0);
    /// The advanced settings (900 × 650): at the base scale 1.5 of this screen they are 15 px taller than the
    /// space above the bar, so they scale down to it ([`sideways_game_window`]).
    const ADVANCED: (f32, f32) = (900.0, 650.0);
    const IN_GAME: [(f32, f32); 4] = [BOOK, (760.0, 420.0), (594.0, 606.0), ADVANCED];

    #[test]
    fn the_desktop_and_large_windows_off_keep_the_in_game_window() {
        assert_eq!(sideways_window(BASE, None, true, (1920.0, 1080.0), 80.0, NO_SAFE, BOOK), None, "the desktop");
        assert_eq!(sideways_window(BASE, Some(2.625), false, SIDE, BAR, NO_SAFE, BOOK), None, "large windows off");
    }

    #[test]
    fn a_phone_held_sideways_gives_the_in_game_save_and_load_books_the_main_menu_s_size() {
        let (k, r) = sideways_window(BASE, Some(2.625), true, SIDE, BAR, NO_SAFE, BOOK).expect("a phone");
        // The height limits it, as the main menu's load window: (1080 − 120 − 4) / 482 ≈ 1.98 (over the base 1.5).
        assert!((k - (1080.0 - BAR - 4.0) / 482.0).abs() < 0.01, "{k}");
        assert!(k >= 1.25 * BASE, "{k} is not a quarter larger than the base {BASE}");
        assert!(r.x >= 0.0 && r.x + r.w <= SIDE.0 + 0.01, "{r:?} is wider than the screen");
        assert!(r.y >= 0.0 && r.y + r.h <= SIDE.1 - BAR + 0.01, "{r:?} is taller than the screen above the bar");
        assert!((r.x + r.w / 2.0 - SIDE.0 / 2.0).abs() < 0.01, "not centred across the screen");
        assert!((r.y + r.h / 2.0 - (SIDE.1 - BAR) / 2.0).abs() < 0.01, "not centred above the bar");
    }

    /// A tall window over the map (the advanced settings, 900 × 650 at 2.625 px per dp on 2400×1080) stays
    /// inside the room above the bar: it scales down below the base, centred there, and never reaches the bar.
    #[test]
    fn a_tall_window_over_the_map_stays_above_the_bar_sideways() {
        let (k, r) = sideways_game_window(BASE, Some(2.625), true, SIDE, BAR, NO_SAFE, ADVANCED).expect("a phone");
        assert!(k < BASE, "{k}: the advanced window scales down to the room above the bar");
        assert!(r.y >= 0.0 && r.y + r.h <= SIDE.1 - BAR + 0.01, "{r:?} reaches the bar");
        assert!((r.y + r.h - (SIDE.1 - BAR)).abs() < 1.0, "{r:?} fills the room above the bar");
        assert!((r.x + r.w / 2.0 - SIDE.0 / 2.0).abs() < 0.01, "not centred across the screen");
        assert!((r.y + r.h / 2.0 - (SIDE.1 - BAR) / 2.0).abs() < 1.0, "not centred above the bar");
        assert!(r.x >= 0.0 && r.x + r.w <= SIDE.0 + 0.01, "{r:?} is wider than the screen");
    }

    /// Every in-game window with a title twin keeps its title size where it fits, and never reaches the bar.
    #[test]
    fn the_in_game_windows_with_a_title_twin_fit_the_safe_area_above_the_bar() {
        let cutouts = SafeArea { left: 110.0, right: 110.0, top: 60.0, ..NO_SAFE };
        for video in IN_GAME {
            for safe in [NO_SAFE, cutouts] {
                let (k, r) = sideways_game_window(BASE, Some(2.625), true, SIDE, BAR, safe, video).expect("a phone");
                assert!(r.y >= safe.top - 0.01 && r.y + r.h <= SIDE.1 - BAR + 0.01, "{video:?}: {r:?} is off the top inset or the bar");
                assert!(r.x >= safe.left - 0.01 && r.x + r.w <= SIDE.0 - safe.right + 0.01, "{video:?}: {r:?} is under the cutouts");
                assert!((r.x + r.w / 2.0 - (SIDE.0 + safe.left - safe.right) / 2.0).abs() < 0.01, "{video:?}: not centred between the cutouts");
                // Where it fits, it is the title's window (`sideways_window`), unchanged.
                let (title_k, title_r) = sideways_window(BASE, Some(2.625), true, SIDE, BAR, safe, video).expect("a phone");
                if title_r.y + title_r.h <= SIDE.1 - BAR {
                    assert!((k - title_k).abs() < 1e-4 && title_r == r, "{video:?}: a window that fits keeps its size");
                }
                assert!(k <= 2.625 + 1e-4, "{video:?}: {k} is above the phone's own scale");
            }
        }
    }
}
