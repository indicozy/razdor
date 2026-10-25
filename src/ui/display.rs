//! The window's display mode (windowed, borderless, full screen) and the interface scale,
//! both chosen in the settings.
//!
//! miniquad only turns full screen on (on X11 it cannot even turn it off again) and has no
//! borderless window, so the modes are set here on each system's own window: X11 through a
//! second connection to the X server, Windows through the window's styles, macOS through the
//! `NSWindow`. Wayland is not used (miniquad runs on X11 alone by default).

use razdor::local;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

/// How the game's window covers the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    /// An ordinary window with its frame.
    #[default]
    Windowed,
    /// A window without frame over the whole monitor, still an ordinary window to the system.
    Borderless,
    /// The system's full screen.
    Fullscreen,
}

impl DisplayMode {
    pub fn next(self) -> DisplayMode {
        match self {
            DisplayMode::Windowed => DisplayMode::Borderless,
            DisplayMode::Borderless => DisplayMode::Fullscreen,
            DisplayMode::Fullscreen => DisplayMode::Windowed,
        }
    }

    pub fn label(self) -> &'static str {
        use razdor::i18n::tr;
        match self {
            DisplayMode::Windowed => tr("Windowed"),
            DisplayMode::Borderless => tr("Borderless"),
            DisplayMode::Fullscreen => tr("Full screen"),
        }
    }
}

// ------------------------------------------------------------------------------------------
// Interface scale
// ------------------------------------------------------------------------------------------

/// The interface scales the player can pick, in screen pixels per pixel of the 960×720
/// reference (`chrome::k`); `0` is "Auto", the largest that fits the window. A scale above the
/// fit is kept to the fit (players got stuck with the settings off-screen in 1.0.0), so the
/// settings always fit the screen.
pub const SCALES: [f32; 10] = [0.0, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 3.5, 4.0];

/// The chosen scale as `f32` bits (`0.0`: auto), read by `chrome::k` every call.
static SCALE: AtomicU32 = AtomicU32::new(0);

pub fn set_scale(scale: f32) {
    SCALE.store(scale.to_bits(), Ordering::Relaxed);
}

/// The interface scale for a window whose own fit is `fit`: the chosen one kept to the fit
/// (a scale above the fit is kept to the fit, so the windows always fit the screen); Auto is
/// the fit.
pub fn scale(fit: f32) -> f32 {
    fitted_scale(fit)
}

/// The chosen scale kept to the window's fit: the bottom bar, the FPS counter and the keyboard
/// strip stay as large as the fit allows, whatever the player chose.
pub fn fitted_scale(fit: f32) -> f32 {
    let chosen = chosen_scale();
    if chosen > 0.0 { chosen.min(fit) } else { fit }
}

/// Runs `f` with the interface scale kept to the fit `fit`: inside, `chrome::k()` and the
/// bitmap fonts' native sizes (`dt_font::native_size`, which follow the interface scale) are
/// the fitted scale, so the bottom bar's text matches its fitted layout above the fit.
pub fn with_fitted<R>(fit: f32, f: impl FnOnce() -> R) -> R {
    let chosen = chosen_scale();
    if chosen <= fit {
        return f();
    }
    set_scale(fit);
    let r = f();
    set_scale(chosen);
    r
}

/// The scale chosen in the settings as it is (`0.0`: auto), not yet kept to the window's fit.
pub fn chosen_scale() -> f32 {
    f32::from_bits(SCALE.load(Ordering::Relaxed))
}

/// The scale after `chosen` in the settings' cycle against the window's fit: Auto, then each
/// scale of [`SCALES`] that fits, then Auto again. A `chosen` above the fit (saved by 1.0.0)
/// goes to Auto.
pub fn next_scale(chosen: f32, fit: f32) -> f32 {
    if chosen > fit + 0.01 {
        return 0.0;
    }
    let cycle: Vec<f32> = SCALES.iter().copied().filter(|&s| s <= 0.0 || s <= fit + 0.01).collect();
    let at = cycle.iter().position(|&s| s == chosen).unwrap_or(0);
    cycle[(at + 1) % cycle.len()]
}

/// The settings' label of a scale, against the window's own fit: Auto shows the fit, and a
/// scale above the fit shows the fit it is kept to.
pub fn scale_label(chosen: f32, fit: f32) -> String {
    use razdor::i18n::tr;
    if chosen <= 0.0 {
        format!("{} ({:.1}×)", tr("Auto"), fit)
    } else if chosen > fit + 0.01 {
        format!("{:.1}×", fit)
    } else {
        format!("{chosen}×")
    }
}

// ------------------------------------------------------------------------------------------
// Text size and large message windows
// ------------------------------------------------------------------------------------------
// Razdor's (the owner's request after players' reports: phones' text too small).

/// The text sizes the player can pick («Размер текста»), times the long texts' original size.
pub const TEXT_SIZES: [f32; 3] = [1.0, 1.25, 1.5];

/// The chosen text size as `f32` bits (`1.0`: the original's size).
static TEXT_SIZE: AtomicU32 = AtomicU32::new(0x3f80_0000);

/// Whether the 634-px pop-up message windows take the largest scale that fits the screen.
static LARGE_WINDOWS: AtomicBool = AtomicBool::new(false);

pub fn set_text_size(s: f32) {
    TEXT_SIZE.store(s.to_bits(), Ordering::Relaxed);
}

/// The chosen text size, read by every long text's layout each frame.
pub fn text_size() -> f32 {
    f32::from_bits(TEXT_SIZE.load(Ordering::Relaxed))
}

pub fn set_large_windows(on: bool) {
    LARGE_WINDOWS.store(on, Ordering::Relaxed);
}

pub fn large_windows() -> bool {
    LARGE_WINDOWS.load(Ordering::Relaxed)
}

/// The text size on a fresh install: «крупный» on phones, where the phone's text base (one px per
/// dp) times it is the 1.0.2 default's text; the original's size on the desktop.
pub fn default_text_size() -> f32 {
    if razdor::platform::MOBILE { 1.25 } else { 1.0 }
}

/// Large message windows on a fresh install: on phones only.
pub fn default_large_windows() -> bool {
    razdor::platform::MOBILE
}

/// The text size after `s` in the settings' cycle over [`TEXT_SIZES`]; a value that is not one
/// of them counts as the first one, so its next is the second.
pub fn next_text_size(s: f32) -> f32 {
    let at = TEXT_SIZES.iter().position(|&t| (t - s).abs() < 1e-3).unwrap_or(0);
    TEXT_SIZES[(at + 1) % TEXT_SIZES.len()]
}

/// The settings' label of a text size.
pub fn text_size_label(s: f32) -> &'static str {
    use razdor::i18n::tr;
    if (s - 1.5).abs() < 1e-3 {
        tr("Largest")
    } else if (s - 1.25).abs() < 1e-3 {
        tr("Large")
    } else {
        tr("Normal")
    }
}

/// [`text_o`] for the text size `t`, pure (no global state).
pub fn text_o_at(t: f32, o: f32) -> f32 {
    if t == 1.0 { o } else { crate::ui::dt_font::whole_scale(o * t) }
}

/// The phone's text base (the owner's request after players' reports): one original px per dp,
/// which is the 1.0.2 large-window scale, always, whether «Крупные окна сообщений» is on or
/// not. The text size multiplies it. `base` is the window's own scale off the phone (`dp`
/// `None`: it is returned as it is). `reference` is the message whose smallest size the scale
/// is read for (`dialog::TEXT_REF`), `screen` and `bar` as in [`popup_scale`].
pub fn phone_text_base(dp: Option<f32>, base: f32, reference: (f32, f32), screen: (f32, f32), bar: f32) -> f32 {
    match dp {
        None => base,
        Some(_) => {
            let raw = popup_scale(base, true, dp, reference, screen, bar);
            crate::ui::dt_font::whole_scale(raw).min((screen.0 - 12.0) / reference.0).max(base)
        }
    }
}

/// [`phone_text_base`] for this screen and the phone's density.
pub fn text_base(base: f32, reference: (f32, f32)) -> f32 {
    phone_text_base(dp_px(), base, reference, (macroquad::window::screen_width(), macroquad::window::screen_height()), crate::ui::chrome::bar_height())
}

/// The text's scale for the window of scale `base` and the message `reference`: the text base
/// times the text size (`text_o_at`). At the original's size on the desktop it is `base`.
pub fn text_for(base: f32, reference: (f32, f32)) -> f32 {
    text_for_at(text_size(), dp_px(), base, reference, (macroquad::window::screen_width(), macroquad::window::screen_height()), crate::ui::chrome::bar_height())
}

/// [`text_for`] for the text size `t`, the phone's density `dp` and the screen, pure (no global
/// state): the text base times the size (`phone_text_base`, `text_o_at`).
pub fn text_for_at(t: f32, dp: Option<f32>, base: f32, reference: (f32, f32), screen: (f32, f32), bar: f32) -> f32 {
    text_o_at(t, phone_text_base(dp, base, reference, screen, bar))
}

/// Whether the text is the original's own (the desktop at the original's size): then the texts
/// keep the interface scale's native metrics, as before the text size. Off, they take the sizes
/// asked for (`dt_font::with_legacy_metrics`).
pub fn native_text() -> bool {
    dp_px().is_none() && text_size() == 1.0
}

/// The window titles' size relative to the interface scale `k`: the text's scale over `k`, so
/// the titles grow with «Размер текста» and on a phone with its base, as the messages' text
/// does. 1 at the original's size on the desktop.
pub fn title_factor(k: f32) -> f32 {
    text_for(k, crate::ui::dialog::TEXT_REF) / k
}

/// The scale of a pop-up window whose smallest size is `min_w`×`min_h` original px. Off, it is
/// `base` (the interface scale's). On, it is the largest scale that fits `sw`×`sh` above a
/// `bar`-tall bottom bar (12 px of side margin in all, 4 px above the bar), never below `base`,
/// and wanted: on a phone `dp` (one original px per dp, Razdor's: the text is as large as the
/// phone's own text); on the desktop up to twice `base`.
pub fn popup_scale(base: f32, large: bool, dp: Option<f32>, (min_w, min_h): (f32, f32), (sw, sh): (f32, f32), bar: f32) -> f32 {
    if !large {
        return base;
    }
    let fit = ((sw - 12.0) / min_w).min((sh - bar - 4.0) / min_h);
    let want = dp.unwrap_or(base * 2.0);
    base.max(want.min(fit))
}

/// Screen px per dp on a phone (Razdor's; `None` on the desktop). Read once from the system's
/// density on Android (`ui::android::read_density`), 1 on iOS (its screen is in points), and
/// set by `RAZDOR_DP` in snapshots, the way a phone's layout is tested on the desktop.
static DP: AtomicU32 = AtomicU32::new(0);
/// Whether [`DP`] has been read from the system yet.
static DP_READ: AtomicBool = AtomicBool::new(false);

pub fn set_dp_px(dp: Option<f32>) {
    DP.store(dp.filter(|d| d.is_finite() && *d > 0.0).map_or(0, f32::to_bits), Ordering::Relaxed);
}

pub fn dp_px() -> Option<f32> {
    let dp = f32::from_bits(DP.load(Ordering::Relaxed));
    (dp > 0.0).then_some(dp)
}

/// Reads the system's dp once, on the first frame of the app (phones only).
fn read_dp_once() {
    if !DP_READ.swap(true, Ordering::Relaxed) {
        #[cfg(target_os = "android")]
        set_dp_px(Some(super::android::read_density() / macroquad::window::screen_dpi_scale()));
        #[cfg(target_os = "ios")]
        set_dp_px(Some(1.0));
    }
}

/// [`popup_scale`] for this screen and the current setting.
pub fn popup_o(base: f32, min_w: f32, min_h: f32) -> f32 {
    popup_scale(
        base,
        large_windows(),
        dp_px(),
        (min_w, min_h),
        (macroquad::window::screen_width(), macroquad::window::screen_height()),
        crate::ui::chrome::bar_height(),
    )
}

/// A text's scale in a box that does not scroll: the chosen text size, stepped down (1.5, then
/// 1.25, then the original's size) until `fits` takes the text at that scale (Razdor's: a
/// phone's large text must not overflow a box that cannot scroll).
pub fn text_o_fitting(o: f32, fits: impl Fn(f32) -> bool) -> f32 {
    text_o_fitting_at(text_size(), o, fits)
}

/// [`text_o_fitting`] for the text size `chosen`, pure (no global state).
pub fn text_o_fitting_at(chosen: f32, o: f32, fits: impl Fn(f32) -> bool) -> f32 {
    for &s in TEXT_SIZES.iter().rev().filter(|&&s| s <= chosen + 1e-3) {
        let t = if s == 1.0 { o } else { crate::ui::dt_font::whole_scale(o * s) };
        if fits(t) {
            return t;
        }
    }
    o
}

// ------------------------------------------------------------------------------------------
// Display mode
// ------------------------------------------------------------------------------------------

/// What the window is in now, as far as this module set it.
struct State {
    applied: Option<DisplayMode>,
    /// The mode being set (a system may take some frames to leave the last one).
    target: Option<DisplayMode>,
    /// When the mode was last set: a window manager may answer late (or the first request
    /// may reach a window it does not manage yet), so the mode is set once more after a while.
    since: f64,
    settled: bool,
    /// When `follow` first ran (the first frame), and how many start nudge steps were taken.
    started: Option<f64>,
    nudges: usize,
    sys: sys::Window,
}

local! {
    static STATE: std::cell::RefCell<State> =
        std::cell::RefCell::new(State { applied: None, target: None, since: 0.0, settled: true, started: None, nudges: 0, sys: sys::Window::default() });
}

/// Seconds after a change when it is set once more.
const SETTLE_SECONDS: f64 = 0.5;

/// A step of the start nudge ([`nudge_due`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Nudge {
    /// The window one pixel wider, kept so for some frames.
    Grow,
    /// The window back to its size.
    Restore,
}

/// The start nudge's steps, in seconds after the first frame. On Windows an OpenGL window that
/// was never resized after it appeared can stay black under the desktop compositor (players'
/// reports: black until maximized, stretched or Alt+Enter, on every windowed start); a start
/// in borderless or full screen moves the window, and never showed it. A size change that is
/// undone before the next frame (1 px wider and back at once, as 0.4.21..0.4.32 did) was not
/// enough: the grown size is now kept for half a second, frames presented at it, and only then
/// put back.
const NUDGE: [(f64, Nudge); 2] = [(0.3, Nudge::Grow), (0.8, Nudge::Restore)];

/// The start nudge step due `elapsed` seconds after the first frame with `taken` steps taken
/// already: at most one per frame, so frames are presented between them.
fn nudge_due(elapsed: f64, taken: usize) -> Option<Nudge> {
    NUDGE.get(taken).filter(|(at, _)| elapsed >= *at).map(|&(_, step)| step)
}

/// Where a window stands among the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Layer {
    /// Above every ordinary window.
    Topmost,
    /// An ordinary window.
    Normal,
    /// Below every other window.
    Bottom,
}

/// Where the window stands in `mode` while it has (`focused`) or lacks the keyboard focus.
/// Full screen is above everything only while it is the active window: kept topmost, it
/// covered whatever Alt+Tab or Win+Tab switched to (players' reports), so once another window
/// is active it goes below the others, as a game's exclusive full screen does when it is left.
#[cfg_attr(not(windows), allow(dead_code))]
fn layer(mode: DisplayMode, focused: bool) -> Layer {
    match mode {
        DisplayMode::Fullscreen if focused => Layer::Topmost,
        DisplayMode::Fullscreen => Layer::Bottom,
        _ => Layer::Normal,
    }
}

/// The window's rectangle (left, top, right, bottom) over a monitor's `m` in a frame-less
/// `mode`. Borderless is one pixel taller than the monitor: an OpenGL window covering exactly
/// the whole monitor is taken by Windows and the graphics drivers as an exclusive full-screen
/// application, and then Win+Tab and the switch to other windows misbehave (players'
/// reports); one pixel more (below the screen's edge, never seen) keeps it an ordinary window.
/// Full screen covers the monitor exactly.
#[cfg_attr(not(windows), allow(dead_code))]
fn popup_rect(mode: DisplayMode, m: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    let (l, t, r, b) = m;
    match mode {
        DisplayMode::Borderless => (l, t, r, b + 1),
        _ => (l, t, r, b),
    }
}

// ------------------------------------------------------------------------------------------
// Alt+Enter
// ------------------------------------------------------------------------------------------

/// Seconds the new mode's name stays on screen after Alt+Enter; it fades over the last 0.5.
const NOTICE_SECONDS: f64 = 2.0;

local! {
    /// The notice Alt+Enter or Ctrl+0 last set (its text) and when.
    static NOTICE: std::cell::RefCell<Option<(String, f64)>> = std::cell::RefCell::new(None);
}

/// Ctrl+0 went down this frame: the interface scale back to Auto, on any screen (a player who
/// scaled the settings off the screen in 1.0.0 had no way back).
pub fn scale_reset_pressed() -> bool {
    use crate::ui::input::{is_key_down, is_key_pressed};
    use macroquad::input::KeyCode;
    (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)) && (is_key_pressed(KeyCode::Key0) || is_key_pressed(KeyCode::Kp0))
}

/// Alt+Enter went down this frame: the next display mode, on any screen (Razdor's, the
/// players asked for it: some overlays need a window, and a battle cannot be left for the
/// settings).
pub fn hotkey_pressed() -> bool {
    use crate::ui::input::{is_key_down, is_key_pressed};
    use macroquad::input::KeyCode;
    (is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt)) && (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter))
}

/// Shows the display mode's name for a moment ([`draw_notice`]).
pub fn notice(mode: DisplayMode) {
    set_notice(format!("{}: {}", razdor::i18n::tr("Screen"), mode.label()));
}

/// Shows "Interface scale: Auto" for a moment ([`draw_notice`]), after Ctrl+0.
pub fn notice_scale() {
    set_notice(format!("{}: {}", razdor::i18n::tr("Interface scale"), razdor::i18n::tr("Auto")));
}

fn set_notice(text: String) {
    NOTICE.with(|n| *n.borrow_mut() = Some((text, crate::ui::clock::now())));
}

/// The notice at the top of the screen over everything else.
pub fn draw_notice() {
    use crate::ui::widgets::{measure, text_centered, ACCENT, PANEL};
    use macroquad::prelude::*;
    let Some((m, at)) = NOTICE.with(|n| n.borrow().clone()) else { return };
    let left = NOTICE_SECONDS - (crate::ui::clock::now() - at);
    if left <= 0.0 {
        NOTICE.with(|n| *n.borrow_mut() = None);
        return;
    }
    let a = (left / 0.5).clamp(0.0, 1.0) as f32;
    let k = crate::ui::chrome::k();
    let (size, h) = ((18.0 * k).max(14.0), (30.0 * k).max(24.0));
    let w = measure(&m, size).width + 40.0 * k;
    let (cx, y) = (screen_width() / 2.0, 16.0 * k);
    draw_rectangle(cx - w / 2.0, y, w, h, Color { a: PANEL.a * a, ..PANEL });
    draw_rectangle_lines(cx - w / 2.0, y, w, h, 1.0, Color { a: 0.5 * a, ..crate::ui::chrome::SILVER });
    text_centered(&m, cx, y + h * 0.5 + size * 0.36, size, Color { a, ..ACCENT });
}

/// The settings' display mode, interface scale, text size and large windows, taken in at the
/// start of every frame.
pub fn follow_settings(settings: &crate::ui::audio::Settings) {
    crate::ui::dt_font::set_mode(settings.font);
    if crate::ui::snapshot::size().is_some() {
        // Offscreen snapshots keep their fixed size and the automatic scale, or the one in
        // `RAZDOR_UI_SCALE` (checking a screen at several interface scales). The text size
        // (`RAZDOR_TEXT_SIZE`, one of `TEXT_SIZES`) and the large windows (`RAZDOR_LARGE_WINDOWS=1`)
        // come from their variables too: the original's size and off otherwise.
        let scale = std::env::var("RAZDOR_UI_SCALE").ok().and_then(|s| s.trim().parse::<f32>().ok());
        set_scale(scale.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(0.0));
        let text = std::env::var("RAZDOR_TEXT_SIZE").ok().and_then(|s| s.trim().parse::<f32>().ok());
        set_text_size(text.and_then(|t| TEXT_SIZES.iter().copied().find(|&v| (v - t).abs() < 1e-3)).unwrap_or(1.0));
        set_large_windows(std::env::var("RAZDOR_LARGE_WINDOWS").is_ok_and(|v| v.trim() == "1"));
        // `RAZDOR_DP=<f32>`: a phone's density (screen px per dp), the phone layout on the desktop.
        set_dp_px(std::env::var("RAZDOR_DP").ok().and_then(|s| s.trim().parse::<f32>().ok()));
        return;
    }
    read_dp_once();
    set_scale(settings.ui_scale);
    set_text_size(settings.text_size);
    set_large_windows(settings.large_windows);
    crate::ui::world_view::set_zoom_prefs(settings.map_zoom, settings.zoom_locked);
    follow(settings.display);
}

/// Puts the window in `mode` when it is not yet (called every frame; cheap when nothing
/// changes). The first call leaves a windowed start alone.
fn follow(mode: DisplayMode) {
    let now = crate::ui::clock::now();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let started = *s.started.get_or_insert(now);
        if let Some(step) = nudge_due(now - started, s.nudges) {
            s.nudges += 1;
            if s.applied == Some(DisplayMode::Windowed) && mode == DisplayMode::Windowed {
                s.sys.nudge(step);
            }
        }
        if s.applied == Some(DisplayMode::Fullscreen) {
            s.sys.keep_layer(DisplayMode::Fullscreen);
        }
        let from = s.applied.unwrap_or(DisplayMode::Windowed);
        if s.applied == Some(mode) {
            if s.settled || now - s.since < SETTLE_SECONDS {
                return;
            }
            s.settled = true;
        } else if s.applied.is_none() && mode == DisplayMode::Windowed {
            s.applied = Some(mode);
            return;
        }
        if s.target != Some(mode) {
            razdor::diag::step(&format!("display mode {from:?} -> {mode:?}"));
            s.target = Some(mode);
        }
        // A system still leaving the last mode says so; the next frame tries again.
        if s.sys.apply(from, mode) && s.applied != Some(mode) {
            s.applied = Some(mode);
            s.settled = false;
            s.since = now;
        }
    });
}

/// Whether the system pointer is outside the game's window (the window manager's frame or
/// another window): macroquad keeps the last position it saw, so a pointer that left the
/// window through an edge would stay "at the edge" and scroll the map (an owner's report).
/// False when the system cannot tell.
pub fn pointer_outside() -> bool {
    if crate::ui::snapshot::size().is_some() {
        return false;
    }
    STATE.with(|s| s.borrow_mut().sys.pointer_inside()) == Some(false)
}

#[cfg(target_os = "linux")]
pub use sys::WM_CLASS;

#[cfg(all(target_os = "linux", not(target_os = "android")))]
mod sys {
    //! X11: `_NET_WM_STATE_FULLSCREEN` for full screen; for borderless the Motif hints drop the
    //! frame and the window is moved over its monitor (XRandR's monitors when there).
    use super::DisplayMode;
    use std::ffi::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_void, CStr};

    type Display = c_void;
    type XWindow = c_ulong;
    type Atom = c_ulong;

    #[repr(C)]
    struct XClassHint {
        res_name: *mut c_char,
        res_class: *mut c_char,
    }

    #[repr(C)]
    struct XClientMessageEvent {
        type_: c_int,
        serial: c_ulong,
        send_event: c_int,
        display: *mut Display,
        window: XWindow,
        message_type: Atom,
        format: c_int,
        data: [c_long; 5],
    }

    /// `XEvent` is a union of 24 longs; `XSendEvent` reads all of it.
    #[repr(C)]
    struct XEvent {
        client: XClientMessageEvent,
        _rest: [c_long; 24],
    }

    #[repr(C)]
    struct XWindowAttributes {
        x: c_int,
        y: c_int,
        width: c_int,
        height: c_int,
        _rest: [c_long; 32],
    }

    #[repr(C)]
    struct XRRMonitorInfo {
        name: Atom,
        primary: c_int,
        automatic: c_int,
        noutput: c_int,
        x: c_int,
        y: c_int,
        width: c_int,
        height: c_int,
        mwidth: c_int,
        mheight: c_int,
        outputs: *mut c_ulong,
    }

    struct Xlib {
        open_display: unsafe extern "C" fn(*const c_char) -> *mut Display,
        default_root: unsafe extern "C" fn(*mut Display) -> XWindow,
        default_screen: unsafe extern "C" fn(*mut Display) -> c_int,
        display_width: unsafe extern "C" fn(*mut Display, c_int) -> c_int,
        display_height: unsafe extern "C" fn(*mut Display, c_int) -> c_int,
        intern_atom: unsafe extern "C" fn(*mut Display, *const c_char, c_int) -> Atom,
        query_tree: unsafe extern "C" fn(*mut Display, XWindow, *mut XWindow, *mut XWindow, *mut *mut XWindow, *mut c_uint) -> c_int,
        get_class_hint: unsafe extern "C" fn(*mut Display, XWindow, *mut XClassHint) -> c_int,
        free: unsafe extern "C" fn(*mut c_void) -> c_int,
        change_property: unsafe extern "C" fn(*mut Display, XWindow, Atom, Atom, c_int, c_int, *const c_uchar, c_int) -> c_int,
        send_event: unsafe extern "C" fn(*mut Display, XWindow, c_int, c_long, *mut XEvent) -> c_int,
        move_resize: unsafe extern "C" fn(*mut Display, XWindow, c_int, c_int, c_uint, c_uint) -> c_int,
        get_attributes: unsafe extern "C" fn(*mut Display, XWindow, *mut XWindowAttributes) -> c_int,
        translate: unsafe extern "C" fn(*mut Display, XWindow, XWindow, c_int, c_int, *mut c_int, *mut c_int, *mut XWindow) -> c_int,
        raise: unsafe extern "C" fn(*mut Display, XWindow) -> c_int,
        sync: unsafe extern "C" fn(*mut Display, c_int) -> c_int,
        query_pointer: unsafe extern "C" fn(*mut Display, XWindow, *mut XWindow, *mut XWindow, *mut c_int, *mut c_int, *mut c_int, *mut c_int, *mut c_uint) -> c_int,
        /// libXrandr's, when it loads.
        get_monitors: Option<unsafe extern "C" fn(*mut Display, XWindow, c_int, *mut c_int) -> *mut XRRMonitorInfo>,
        free_monitors: Option<unsafe extern "C" fn(*mut XRRMonitorInfo)>,
    }

    unsafe fn open(name: &CStr) -> *mut c_void {
        libc::dlopen(name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL)
    }

    unsafe fn sym<T: Copy>(lib: *mut c_void, name: &CStr) -> Option<T> {
        let p = libc::dlsym(lib, name.as_ptr());
        (!p.is_null()).then(|| std::mem::transmute_copy(&p))
    }

    impl Xlib {
        unsafe fn load() -> Option<Xlib> {
            let x = open(c"libX11.so.6");
            if x.is_null() {
                return None;
            }
            let r = open(c"libXrandr.so.2");
            Some(Xlib {
                open_display: sym(x, c"XOpenDisplay")?,
                default_root: sym(x, c"XDefaultRootWindow")?,
                default_screen: sym(x, c"XDefaultScreen")?,
                display_width: sym(x, c"XDisplayWidth")?,
                display_height: sym(x, c"XDisplayHeight")?,
                intern_atom: sym(x, c"XInternAtom")?,
                query_tree: sym(x, c"XQueryTree")?,
                get_class_hint: sym(x, c"XGetClassHint")?,
                free: sym(x, c"XFree")?,
                change_property: sym(x, c"XChangeProperty")?,
                send_event: sym(x, c"XSendEvent")?,
                move_resize: sym(x, c"XMoveResizeWindow")?,
                get_attributes: sym(x, c"XGetWindowAttributes")?,
                translate: sym(x, c"XTranslateCoordinates")?,
                raise: sym(x, c"XRaiseWindow")?,
                sync: sym(x, c"XSync")?,
                query_pointer: sym(x, c"XQueryPointer")?,
                get_monitors: if r.is_null() { None } else { sym(r, c"XRRGetMonitors") },
                free_monitors: if r.is_null() { None } else { sym(r, c"XRRFreeMonitors") },
            })
        }
    }

    /// The window's WM_CLASS (`main::conf`), how the game's own window is found.
    pub const WM_CLASS: &str = "razdor";

    struct Conn {
        x: Xlib,
        dpy: *mut Display,
        root: XWindow,
        win: XWindow,
    }

    #[derive(Default)]
    pub struct Window {
        conn: Option<Conn>,
        tried: bool,
        /// Where the frame-less client sat while windowed: x, y, width, height.
        windowed: Option<(i32, i32, u32, u32)>,
    }

    impl Conn {
        unsafe fn open() -> Option<Conn> {
            let x = Xlib::load()?;
            let dpy = (x.open_display)(std::ptr::null());
            if dpy.is_null() {
                return None;
            }
            let root = (x.default_root)(dpy);
            let mut c = Conn { x, dpy, root, win: 0 };
            c.win = c.find(root, 0)?;
            Some(c)
        }

        /// Our window under `w`: the one whose WM_CLASS is ours (one Razdor runs at a time).
        unsafe fn find(&self, w: XWindow, depth: u32) -> Option<XWindow> {
            let mut hint = XClassHint { res_name: std::ptr::null_mut(), res_class: std::ptr::null_mut() };
            if (self.x.get_class_hint)(self.dpy, w, &mut hint) != 0 {
                let ours = !hint.res_class.is_null() && CStr::from_ptr(hint.res_class).to_bytes() == WM_CLASS.as_bytes();
                for p in [hint.res_name, hint.res_class] {
                    if !p.is_null() {
                        (self.x.free)(p.cast());
                    }
                }
                if ours {
                    return Some(w);
                }
            }
            if depth > 3 {
                return None;
            }
            let (mut root, mut parent, mut kids, mut n) = (0, 0, std::ptr::null_mut(), 0);
            if (self.x.query_tree)(self.dpy, w, &mut root, &mut parent, &mut kids, &mut n) == 0 || kids.is_null() {
                return None;
            }
            let found = std::slice::from_raw_parts(kids, n as usize).iter().rev().find_map(|&k| self.find(k, depth + 1));
            (self.x.free)(kids.cast());
            found
        }

        unsafe fn atom(&self, name: &CStr) -> Atom {
            (self.x.intern_atom)(self.dpy, name.as_ptr(), 0)
        }

        /// Asks the window manager to add or remove full screen (EWMH `_NET_WM_STATE`).
        unsafe fn set_fullscreen(&self, on: bool) {
            let mut ev: XEvent = std::mem::zeroed();
            ev.client = XClientMessageEvent {
                type_: 33, // ClientMessage
                serial: 0,
                send_event: 1,
                display: self.dpy,
                window: self.win,
                message_type: self.atom(c"_NET_WM_STATE"),
                format: 32,
                data: [on as c_long, self.atom(c"_NET_WM_STATE_FULLSCREEN") as c_long, 0, 1, 0],
            };
            // SubstructureRedirectMask | SubstructureNotifyMask
            (self.x.send_event)(self.dpy, self.root, 0, (1 << 20) | (1 << 19), &mut ev);
        }

        /// The Motif hints: the window manager's frame on or off.
        unsafe fn set_frame(&self, on: bool) {
            let hints: [c_long; 5] = [2, 0, on as c_long, 0, 0]; // flags: decorations
            let atom = self.atom(c"_MOTIF_WM_HINTS");
            (self.x.change_property)(self.dpy, self.win, atom, atom, 32, 0, hints.as_ptr().cast(), 5);
        }

        /// The client's place on the root window and its size.
        unsafe fn geometry(&self) -> Option<(i32, i32, u32, u32)> {
            let mut a: XWindowAttributes = std::mem::zeroed();
            if (self.x.get_attributes)(self.dpy, self.win, &mut a) == 0 {
                return None;
            }
            let (mut x, mut y, mut child) = (0, 0, 0);
            (self.x.translate)(self.dpy, self.win, self.root, 0, 0, &mut x, &mut y, &mut child);
            Some((x, y, a.width.max(1) as u32, a.height.max(1) as u32))
        }

        /// The monitor under the window's centre (the whole screen without XRandR).
        unsafe fn monitor(&self) -> (i32, i32, u32, u32) {
            let s = (self.x.default_screen)(self.dpy);
            let whole = (0, 0, (self.x.display_width)(self.dpy, s) as u32, (self.x.display_height)(self.dpy, s) as u32);
            let (Some(get), Some(free)) = (self.x.get_monitors, self.x.free_monitors) else { return whole };
            let (cx, cy) = self.geometry().map_or((0, 0), |(x, y, w, h)| (x + w as i32 / 2, y + h as i32 / 2));
            let mut n = 0;
            let list = get(self.dpy, self.root, 1, &mut n);
            if list.is_null() {
                return whole;
            }
            let mons = std::slice::from_raw_parts(list, n.max(0) as usize);
            let inside = |m: &&XRRMonitorInfo| cx >= m.x && cx < m.x + m.width && cy >= m.y && cy < m.y + m.height;
            let pick = mons.iter().find(inside).or_else(|| mons.iter().find(|m| m.primary != 0)).or(mons.first());
            let r = pick.map_or(whole, |m| (m.x, m.y, m.width as u32, m.height as u32));
            free(list);
            r
        }
    }

    impl Window {
        /// Nothing to do: an X11 window shows its first frames.
        pub fn nudge(&mut self, _step: super::Nudge) {}

        /// Nothing to do: only Windows' full screen changes its place among the windows.
        pub fn keep_layer(&mut self, _mode: DisplayMode) {}

        fn conn(&mut self) -> Option<&Conn> {
            if !self.tried {
                self.tried = true;
                // SAFETY: plain Xlib calls on a connection of our own.
                self.conn = unsafe { Conn::open() };
                if self.conn.is_none() {
                    razdor::diag!("display mode: the game's X11 window was not found");
                }
            }
            self.conn.as_ref()
        }

        /// Whether the system pointer is over the window's client area; None when unknown.
        pub fn pointer_inside(&mut self) -> Option<bool> {
            let c = self.conn()?;
            // SAFETY: Xlib queries on our own window and connection.
            unsafe {
                let mut a: XWindowAttributes = std::mem::zeroed();
                if (c.x.get_attributes)(c.dpy, c.win, &mut a) == 0 {
                    return None;
                }
                let (mut root, mut child, mut rx, mut ry, mut wx, mut wy, mut mask) = (0, 0, 0, 0, 0, 0, 0);
                if (c.x.query_pointer)(c.dpy, c.win, &mut root, &mut child, &mut rx, &mut ry, &mut wx, &mut wy, &mut mask) == 0 {
                    return Some(false);
                }
                Some(wx >= 0 && wy >= 0 && wx < a.width && wy < a.height)
            }
        }

        pub fn apply(&mut self, from: DisplayMode, to: DisplayMode) -> bool {
            if self.conn().is_none() {
                return true;
            }
            let Some(c) = self.conn.as_ref() else { return true };
            // SAFETY: as above; every call names our own window on our own connection.
            unsafe {
                if from == DisplayMode::Windowed && to != DisplayMode::Windowed {
                    self.windowed = c.geometry().or(self.windowed);
                }
                c.set_fullscreen(to == DisplayMode::Fullscreen);
                c.set_frame(to == DisplayMode::Windowed);
                match to {
                    DisplayMode::Borderless => {
                        let (x, y, w, h) = c.monitor();
                        (c.x.move_resize)(c.dpy, c.win, x, y, w, h);
                        (c.x.raise)(c.dpy, c.win);
                    }
                    DisplayMode::Windowed if from != DisplayMode::Windowed => {
                        if let Some((x, y, w, h)) = self.windowed {
                            (c.x.move_resize)(c.dpy, c.win, x, y, w, h);
                        }
                    }
                    _ => {}
                }
                (c.x.sync)(c.dpy, 0);
            }
            true
        }
    }
}

#[cfg(windows)]
mod sys {
    //! Windows: borderless and full screen are both a popup over the monitor (borderless one
    //! pixel taller, [`super::popup_rect`]); full screen also stays above the other windows
    //! while it is the active one ([`super::layer`]). Every step goes to `razdor.log`, which is
    //! all there is to see what happened on a player's machine.
    use super::{DisplayMode, Layer, Nudge};
    use std::ffi::c_void;

    type Hwnd = *mut c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    impl Rect {
        fn size(self) -> (i32, i32) {
            (self.right - self.left, self.bottom - self.top)
        }
    }

    #[repr(C)]
    struct MonitorInfo {
        size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn FindWindowExW(parent: Hwnd, after: Hwnd, class: *const u16, title: *const u16) -> Hwnd;
        fn GetWindowThreadProcessId(w: Hwnd, pid: *mut u32) -> u32;
        fn GetWindowLongPtrW(w: Hwnd, index: i32) -> isize;
        fn SetWindowLongPtrW(w: Hwnd, index: i32, value: isize) -> isize;
        fn SetWindowPos(w: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        fn GetWindowRect(w: Hwnd, r: *mut Rect) -> i32;
        fn MonitorFromWindow(w: Hwnd, flags: u32) -> *mut c_void;
        fn GetMonitorInfoW(m: *mut c_void, info: *mut MonitorInfo) -> i32;
        fn GetCursorPos(p: *mut Point) -> i32;
        fn ScreenToClient(w: Hwnd, p: *mut Point) -> i32;
        fn GetClientRect(w: Hwnd, r: *mut Rect) -> i32;
        fn GetForegroundWindow() -> Hwnd;
        fn IsWindowVisible(w: Hwnd) -> i32;
        fn IsIconic(w: Hwnd) -> i32;
        fn IsZoomed(w: Hwnd) -> i32;
        fn RedrawWindow(w: Hwnd, update: *const Rect, region: *mut c_void, flags: u32) -> i32;
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcessId() -> u32;
    }

    const GWL_STYLE: i32 = -16;
    const GWL_EXSTYLE: i32 = -20;
    const WS_POPUP: isize = 0x8000_0000;
    const WS_VISIBLE: isize = 0x1000_0000;
    const WS_CLIPSIBLINGS: isize = 0x0400_0000;
    const WS_CLIPCHILDREN: isize = 0x0200_0000;
    const WS_CAPTION: isize = 0x00C0_0000;
    const WS_SYSMENU: isize = 0x0008_0000;
    const WS_SIZEBOX: isize = 0x0004_0000;
    const WS_MINIMIZEBOX: isize = 0x0002_0000;
    const WS_MAXIMIZEBOX: isize = 0x0001_0000;
    /// On the taskbar and in Alt+Tab / Win+Tab; a tool window is in neither.
    const WS_EX_APPWINDOW: isize = 0x0004_0000;
    const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_FRAMECHANGED: u32 = 0x0020;
    const SWP_SHOWWINDOW: u32 = 0x0040;
    const HWND_BOTTOM: Hwnd = 1isize as Hwnd;
    const HWND_TOPMOST: Hwnd = -1isize as Hwnd;
    const HWND_NOTOPMOST: Hwnd = -2isize as Hwnd;
    const MONITOR_DEFAULTTONEAREST: u32 = 2;
    const RDW_INVALIDATE: u32 = 0x0001;
    const RDW_UPDATENOW: u32 = 0x0100;
    const RDW_FRAME: u32 = 0x0400;

    #[derive(Default)]
    pub struct Window {
        hwnd: Option<usize>,
        /// The window was looked for once without success (logged once).
        missed: bool,
        windowed: Option<Rect>,
        /// The window's rectangle after the start nudge grew it, until it is put back.
        grown: Option<Rect>,
        /// Whether full screen was last placed as the active window; None: not placed yet.
        focused: Option<bool>,
    }

    /// miniquad's window class (`MINIQUADAPP`) in this process.
    unsafe fn find() -> Option<Hwnd> {
        let class: Vec<u16> = "MINIQUADAPP\0".encode_utf16().collect();
        let me = GetCurrentProcessId();
        let mut w = std::ptr::null_mut();
        loop {
            w = FindWindowExW(std::ptr::null_mut(), w, class.as_ptr(), std::ptr::null());
            if w.is_null() {
                return None;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(w, &mut pid);
            if pid == me {
                return Some(w);
            }
        }
    }

    /// The window's state for the log: rectangles, styles, whether it is shown and active.
    unsafe fn describe(w: Hwnd) -> String {
        let (mut r, mut c) = (Rect::default(), Rect::default());
        GetWindowRect(w, &mut r);
        GetClientRect(w, &mut c);
        format!(
            "window {r:?} client {:?} style {:#x} ex {:#x} visible {} minimized {} maximized {} active {}",
            c.size(),
            GetWindowLongPtrW(w, GWL_STYLE),
            GetWindowLongPtrW(w, GWL_EXSTYLE),
            IsWindowVisible(w) != 0,
            IsIconic(w) != 0,
            IsZoomed(w) != 0,
            GetForegroundWindow() == w
        )
    }

    impl Window {
        /// The game's window, looked up the first time it is needed.
        unsafe fn window(&mut self) -> Option<Hwnd> {
            if self.hwnd.is_none() {
                self.hwnd = find().map(|w| w as usize);
                match self.hwnd {
                    Some(w) => razdor::diag!("display: window found ({w:#x}): {}", describe(w as Hwnd)),
                    None if !self.missed => {
                        self.missed = true;
                        razdor::diag!("display: the game's window (MINIQUADAPP) was not found");
                    }
                    None => {}
                }
            }
            self.hwnd.map(|w| w as Hwnd)
        }

        /// Whether the system pointer is over the window's client area; None when unknown.
        pub fn pointer_inside(&mut self) -> Option<bool> {
            // SAFETY: Win32 queries on our own window.
            unsafe {
                let w = self.window()?;
                let (mut p, mut r) = (Point::default(), Rect::default());
                if GetCursorPos(&mut p) == 0 || ScreenToClient(w, &mut p) == 0 || GetClientRect(w, &mut r) == 0 {
                    return None;
                }
                Some(p.x >= r.left && p.y >= r.top && p.x < r.right && p.y < r.bottom)
            }
        }

        /// A step of the start nudge ([`super::NUDGE`]): one pixel wider, the frame redrawn,
        /// and some frames later back, unless the player has resized, maximized or minimized
        /// the window meanwhile.
        pub fn nudge(&mut self, step: Nudge) {
            // SAFETY: Win32 calls on our own window.
            unsafe {
                let Some(w) = self.window() else { return };
                razdor::diag!("display: start nudge {step:?}: {}", describe(w));
                let mut r = Rect::default();
                if GetWindowRect(w, &mut r) == 0 || IsIconic(w) != 0 || IsZoomed(w) != 0 {
                    return;
                }
                let (cw, ch) = r.size();
                let flags = SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;
                let ok = match step {
                    Nudge::Grow => {
                        let ok = SetWindowPos(w, std::ptr::null_mut(), 0, 0, cw + 1, ch, flags);
                        let mut g = Rect::default();
                        GetWindowRect(w, &mut g);
                        self.grown = Some(g);
                        ok
                    }
                    Nudge::Restore => match self.grown.take() {
                        Some(g) if g == r => SetWindowPos(w, std::ptr::null_mut(), 0, 0, cw - 1, ch, flags),
                        _ => {
                            razdor::diag!("display: start nudge: the window was changed meanwhile, left as it is");
                            return;
                        }
                    },
                };
                RedrawWindow(w, std::ptr::null(), std::ptr::null_mut(), RDW_INVALIDATE | RDW_FRAME | RDW_UPDATENOW);
                razdor::diag!("display: start nudge {step:?} done (SetWindowPos {ok}): {}", describe(w));
            }
        }

        /// Full screen: topmost while active, below the others once another window is
        /// ([`super::layer`]); called every frame, acts when the active window changes.
        pub fn keep_layer(&mut self, mode: DisplayMode) {
            // SAFETY: Win32 calls on our own window.
            unsafe {
                let Some(w) = self.window() else { return };
                let focused = GetForegroundWindow() == w;
                if self.focused == Some(focused) {
                    return;
                }
                self.focused = Some(focused);
                let layer = super::layer(mode, focused);
                let after = match layer {
                    Layer::Topmost => HWND_TOPMOST,
                    Layer::Normal => HWND_NOTOPMOST,
                    Layer::Bottom => HWND_BOTTOM,
                };
                let ok = SetWindowPos(w, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                razdor::diag!("display: {mode:?} {} -> {layer:?} (SetWindowPos {ok})", if focused { "active" } else { "not active" });
            }
        }

        pub fn apply(&mut self, from: DisplayMode, to: DisplayMode) -> bool {
            // SAFETY: Win32 calls on our own window.
            unsafe {
                let Some(w) = self.window() else { return true };
                self.grown = None;
                self.focused = None;
                if from == DisplayMode::Windowed && to != DisplayMode::Windowed {
                    let mut r = Rect::default();
                    if GetWindowRect(w, &mut r) != 0 {
                        self.windowed = Some(r);
                    }
                }
                // In Alt+Tab, Win+Tab and on the taskbar in every mode.
                let ex = (GetWindowLongPtrW(w, GWL_EXSTYLE) & !WS_EX_TOOLWINDOW) | WS_EX_APPWINDOW;
                SetWindowLongPtrW(w, GWL_EXSTYLE, ex);
                let flags = SWP_FRAMECHANGED | SWP_SHOWWINDOW;
                let ok = if to == DisplayMode::Windowed {
                    let style = WS_CLIPSIBLINGS | WS_CLIPCHILDREN | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SIZEBOX | WS_VISIBLE;
                    SetWindowLongPtrW(w, GWL_STYLE, style);
                    let r = self.windowed.unwrap_or(Rect { left: 64, top: 64, right: 64 + 1280, bottom: 64 + 800 });
                    SetWindowPos(w, HWND_NOTOPMOST, r.left, r.top, r.right - r.left, r.bottom - r.top, flags)
                } else {
                    let style = (GetWindowLongPtrW(w, GWL_STYLE) & !(WS_CAPTION | WS_SIZEBOX | WS_MAXIMIZEBOX)) | WS_POPUP | WS_VISIBLE;
                    SetWindowLongPtrW(w, GWL_STYLE, style);
                    let mut info = MonitorInfo { size: std::mem::size_of::<MonitorInfo>() as u32, monitor: Rect::default(), work: Rect::default(), flags: 0 };
                    GetMonitorInfoW(MonitorFromWindow(w, MONITOR_DEFAULTTONEAREST), &mut info);
                    let m = info.monitor;
                    let (l, t, r, b) = super::popup_rect(to, (m.left, m.top, m.right, m.bottom));
                    let after = if super::layer(to, true) == Layer::Topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
                    SetWindowPos(w, after, l, t, r - l, b - t, flags)
                };
                razdor::diag!("display: {from:?} -> {to:?} (SetWindowPos {ok}): {}", describe(w));
            }
            true
        }
    }
}

#[cfg(target_os = "macos")]
mod sys {
    //! macOS: full screen is the system's own (a Space of its own); borderless hides the title
    //! bar and covers the screen, the menu bar and Dock hiding while the game is in front.
    use super::DisplayMode;
    use macroquad::miniquad::native::apple::frameworks::{class, msg_send, nil, sel, sel_impl, NSRect, ObjcId, NO, YES};

    const FULL_SCREEN: u64 = 1 << 14;
    const FULL_SIZE_CONTENT_VIEW: u64 = 1 << 15;
    /// NSApplicationPresentationAutoHideDock | NSApplicationPresentationAutoHideMenuBar
    const AUTO_HIDE: u64 = (1 << 0) | (1 << 2);

    #[derive(Default)]
    pub struct Window {
        windowed: Option<NSRect>,
        /// Full screen as last asked: AppKit's own flag lags behind its animation.
        full: bool,
    }

    /// The title bar and the menu bar and Dock: hidden for borderless, shown otherwise.
    unsafe fn chrome(window: ObjcId, borderless: bool) {
        let app: ObjcId = msg_send![class!(NSApplication), sharedApplication];
        let () = msg_send![app, setPresentationOptions: if borderless { AUTO_HIDE } else { 0u64 }];
        let mask: u64 = msg_send![window, styleMask];
        let mask = if borderless { mask | FULL_SIZE_CONTENT_VIEW } else { mask & !FULL_SIZE_CONTENT_VIEW };
        let () = msg_send![window, setStyleMask: mask];
        let () = msg_send![window, setTitlebarAppearsTransparent: if borderless { YES } else { NO }];
        let () = msg_send![window, setTitleVisibility: if borderless { 1i64 } else { 0i64 }];
        for button in 0u64..3 {
            let b: ObjcId = msg_send![window, standardWindowButton: button];
            if b != nil {
                let () = msg_send![b, setHidden: if borderless { YES } else { NO }];
            }
        }
    }

    impl Window {
        /// Nothing to do: a macOS window shows its first frames.
        pub fn nudge(&mut self, _step: super::Nudge) {}

        /// Nothing to do: only Windows' full screen changes its place among the windows.
        pub fn keep_layer(&mut self, _mode: DisplayMode) {}

        /// Not known here: the pointer is taken as over the window.
        pub fn pointer_inside(&mut self) -> Option<bool> {
            None
        }

        pub fn apply(&mut self, from: DisplayMode, to: DisplayMode) -> bool {
            // SAFETY: AppKit messages to our own window on the main thread.
            unsafe {
                let view = macroquad::miniquad::window::apple_view();
                let window: ObjcId = msg_send![view, window];
                if window == nil {
                    return true;
                }
                let mask: u64 = msg_send![window, styleMask];
                let in_full = mask & FULL_SCREEN != 0;
                if from == DisplayMode::Windowed && !in_full && to != DisplayMode::Windowed {
                    self.windowed = Some(msg_send![window, frame]);
                }
                if to == DisplayMode::Fullscreen {
                    if !self.full {
                        chrome(window, false);
                        let () = msg_send![window, toggleFullScreen: nil];
                        self.full = true;
                    }
                    return true;
                }
                if self.full {
                    let () = msg_send![window, toggleFullScreen: nil];
                    self.full = false;
                }
                if in_full {
                    return false; // Leaving takes an animation; the next frames wait for it.
                }
                let borderless = to == DisplayMode::Borderless;
                chrome(window, borderless);
                if borderless {
                    let screen: ObjcId = msg_send![window, screen];
                    if screen != nil {
                        let frame: NSRect = msg_send![screen, frame];
                        let () = msg_send![window, setFrame: frame display: YES];
                    }
                } else if let Some(frame) = self.windowed {
                    let () = msg_send![window, setFrame: frame display: YES];
                }
            }
            true
        }
    }
}

#[cfg(not(any(all(target_os = "linux", not(target_os = "android")), windows, target_os = "macos")))]
mod sys {
    use super::DisplayMode;

    #[derive(Default)]
    pub struct Window;

    impl Window {
        pub fn nudge(&mut self, _step: super::Nudge) {}

        /// Nothing to do: only Windows' full screen changes its place among the windows.
        pub fn keep_layer(&mut self, _mode: DisplayMode) {}

        /// Not known here: the pointer is taken as over the window.
        pub fn pointer_inside(&mut self) -> Option<bool> {
            None
        }

        pub fn apply(&mut self, _from: DisplayMode, to: DisplayMode) -> bool {
            macroquad::miniquad::window::set_fullscreen(to != DisplayMode::Windowed);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windowed_start_grows_its_window_and_puts_it_back_later() {
        assert_eq!(nudge_due(0.0, 0), None, "not on the first frame");
        assert_eq!(nudge_due(0.31, 0), Some(Nudge::Grow));
        assert_eq!(nudge_due(0.5, 1), None, "the grown size is kept for some frames");
        assert_eq!(nudge_due(0.81, 1), Some(Nudge::Restore));
        assert_eq!(nudge_due(100.0, 2), None, "never again");
        assert_eq!(nudge_due(100.0, 0), Some(Nudge::Grow), "a long first frame: one step per frame");
        assert_eq!(nudge_due(100.0, 1), Some(Nudge::Restore));
    }

    #[test]
    fn full_screen_is_topmost_only_while_active() {
        assert_eq!(layer(DisplayMode::Fullscreen, true), Layer::Topmost);
        assert_eq!(layer(DisplayMode::Fullscreen, false), Layer::Bottom, "Alt+Tab / Win+Tab show the other window");
        for focused in [true, false] {
            assert_eq!(layer(DisplayMode::Borderless, focused), Layer::Normal);
            assert_eq!(layer(DisplayMode::Windowed, focused), Layer::Normal);
        }
    }

    #[test]
    fn borderless_is_one_pixel_taller_than_its_monitor() {
        let m = (1920, 0, 3840, 1080);
        assert_eq!(popup_rect(DisplayMode::Borderless, m), (1920, 0, 3840, 1081));
        assert_eq!(popup_rect(DisplayMode::Fullscreen, m), m);
    }

    #[test]
    fn modes_cycle_and_read_back() {
        assert_eq!(DisplayMode::Windowed.next().next().next(), DisplayMode::Windowed);
        let json = serde_json::to_string(&DisplayMode::Fullscreen).unwrap();
        assert_eq!(json, "\"fullscreen\"");
        assert_eq!(serde_json::from_str::<DisplayMode>("\"borderless\"").unwrap(), DisplayMode::Borderless);
    }

    #[test]
    fn scale_steps_stay_within_the_fit() {
        // Fit 1.6: the cycle is Auto, 1.0, 1.25, 1.5, then Auto again.
        assert_eq!(next_scale(0.0, 1.6), 1.0);
        assert_eq!(next_scale(1.0, 1.6), 1.25);
        assert_eq!(next_scale(1.25, 1.6), 1.5);
        assert_eq!(next_scale(1.5, 1.6), 0.0, "after the last that fits, Auto");
        // A scale saved above the fit (by 1.0.0) goes to Auto.
        assert_eq!(next_scale(4.0, 1.5), 0.0);
        assert_eq!(scale_label(0.0, 1.5), "Auto (1.5×)");
        assert_eq!(scale_label(1.25, 1.5), "1.25×");
        assert_eq!(scale_label(4.0, 1.5), "1.5×", "a scale above the fit shows the fit");
        assert_eq!(scale_label(2.0, 3.0), "2×");
    }

    #[test]
    fn the_choice_is_kept_to_the_fit_and_the_bar_keeps_it() {
        set_scale(4.0);
        assert_eq!(scale(1.5), 1.5, "a choice above the fit is kept to the fit");
        assert_eq!(fitted_scale(1.5), 1.5);
        set_scale(2.0);
        assert_eq!(scale(1.5), 1.5);
        assert_eq!(fitted_scale(1.5), 1.5, "the bar's scale stays at the fit");
        assert_eq!(fitted_scale(3.0), 2.0, "below the fit it is the choice");
        assert_eq!(scale(3.0), 2.0, "below the fit it is the choice");
        // The bar draws inside `with_fitted`: there the scale is the fit, then the choice again.
        assert_eq!(with_fitted(1.5, chosen_scale), 1.5);
        assert_eq!(chosen_scale(), 2.0);
        assert_eq!(with_fitted(3.0, chosen_scale), 2.0, "below the fit nothing changes");
        set_scale(0.0);
        assert_eq!(scale(1.5), 1.5, "Auto is the fit");
        assert_eq!(fitted_scale(1.5), 1.5);
    }

    #[test]
    fn desktop_large_message_windows_fill_the_screen_up_to_twice_the_base() {
        // The base is the interface scale's: the original's size (960×720) fitted to the screen.
        let screens: [(f32, f32); 10] = [
            (2400.0, 1080.0),
            (2160.0, 1080.0),
            (1600.0, 720.0),
            (1280.0, 720.0),
            (960.0, 540.0),
            (854.0, 480.0),
            (800.0, 480.0),
            (1080.0, 2400.0),
            (720.0, 1600.0),
            (480.0, 854.0),
        ];
        for (sw, sh) in screens {
            let fit_k = (sw / 960.0).min(sh / 720.0);
            let base = fit_k * 0.9375;
            let bar = (80.0 * fit_k).round();
            for (w, h) in [(634.0, 241.0), (634.0, 516.0)] {
                let o = popup_scale(base, true, None, (w, h), (sw, sh), bar);
                assert!(o >= base, "{sw}×{sh} {w}×{h}: {o} below the base {base}");
                assert!(o <= 2.0 * base + 1e-4, "{sw}×{sh} {w}×{h}: {o} above twice the base");
                if o > base {
                    assert!(w * o <= sw - 12.0 + 0.01, "{sw}×{sh} {w}×{h}: wider than the screen");
                    assert!(h * o <= sh - bar - 4.0 + 0.01, "{sw}×{sh} {w}×{h}: taller than the screen above the bar");
                }
            }
        }
    }

    #[test]
    fn a_phone_pop_up_is_one_dp_per_original_pixel_when_it_fits() {
        // Portrait 1080×2400 at 2.625 px per dp: the width is the limit (1068 / 480 = 2.2 px).
        let (sw, sh, bar) = (1080.0, 2400.0, 90.0);
        let base = 1.125 * 0.9375;
        let fit = ((sw - 12.0) / 480.0_f32).min((sh - bar - 4.0) / 211.0);
        let o = popup_scale(base, true, Some(2.625), (480.0, 211.0), (sw, sh), bar);
        assert!((o - fit).abs() < 1e-4, "{o} is not the fit {fit}");
        // A roomy screen: exactly the dp, no cap at twice the base.
        let o = popup_scale(1.0, true, Some(2.625), (480.0, 211.0), (2400.0, 1080.0), 120.0);
        assert!((o - 2.625).abs() < 1e-4, "{o} is not the dp");
        // Below the base it stays at the base.
        assert_eq!(popup_scale(base, true, Some(2.625), (480.0, 211.0), (400.0, 300.0), bar), base);
    }

    #[test]
    fn large_message_windows_off_keep_the_interface_scale() {
        assert_eq!(popup_scale(1.5, false, None, (634.0, 516.0), (2400.0, 1080.0), 80.0), 1.5);
        assert_eq!(popup_scale(1.5, false, Some(2.625), (634.0, 516.0), (1080.0, 2400.0), 80.0), 1.5);
    }

    #[test]
    fn a_large_text_steps_down_until_its_box_takes_it() {
        // Fits at 1.25 only: 1.5 steps down to it.
        assert_eq!(text_o_fitting_at(1.5, 2.0, |t| t <= 2.5 + 1e-3), 2.5);
        // Fits nowhere: the original's size.
        assert_eq!(text_o_fitting_at(1.5, 2.0, |_| false), 2.0);
        // Fits at the largest: taken.
        assert_eq!(text_o_fitting_at(1.5, 2.0, |_| true), 3.0);
        // The chosen size is the largest tried: at 1.0 only the original's.
        assert_eq!(text_o_fitting_at(1.0, 2.0, |_| true), 2.0, "the chosen size is the original's");
    }

    #[test]
    fn text_size_does_not_follow_large_windows() {
        // The text is measured at the window's scale with the large windows off (`base`); the
        // large window grows past it, and the text stays where it was at each size.
        let base = 1.406;
        let grown = popup_scale(base, true, Some(2.625), (480.0, 211.0), (2400.0, 1080.0), 90.0);
        assert!(grown > base, "the large window is larger");
        for t in TEXT_SIZES {
            assert_eq!(text_o_at(t, base), text_o_at(t, base), "the text base does not change");
            assert!(text_o_at(t, base) <= text_o_at(TEXT_SIZES[2], base) + 1e-4);
        }
        assert_eq!(text_o_at(1.0, base), base, "the original's size is the base");
    }

    #[test]
    fn a_phone_s_default_text_is_the_1_0_2_default() {
        // 2400×1080 at 2.625 px per dp: 1.0.2's large window was the dp scale, its text ×1.25.
        let (sw, sh, bar) = (2400.0, 1080.0, 120.0);
        let base = 1.5 * 0.9375;
        let (min_w, min_h) = crate::ui::dialog::TEXT_REF;
        let old_o = crate::ui::dt_font::whole_scale(popup_scale(base, true, Some(2.625), (min_w, min_h), (sw, sh), bar)).min((sw - 12.0) / min_w);
        let old_text = text_o_at(1.25, old_o);
        let new_text = text_o_at(1.25, phone_text_base(Some(2.625), base, (min_w, min_h), (sw, sh), bar));
        assert!((new_text - old_text).abs() <= old_text * 0.01, "{new_text} is not the 1.0.2 default {old_text}");
        assert!((new_text - 3.0).abs() < 0.01, "about 3 px per original px: {new_text}");
    }

    #[test]
    fn a_phone_tooltip_reads_the_message_s_text_base() {
        let (sw, sh, bar) = (2400.0, 1080.0, 120.0);
        let reference = crate::ui::dialog::TEXT_REF;
        let k = 1.5;
        let message = phone_text_base(Some(2.625), k * 0.9375, reference, (sw, sh), bar);
        let tooltip = phone_text_base(Some(2.625), k, reference, (sw, sh), bar);
        assert_eq!(message, tooltip, "one text scale on the phone");
        assert_eq!(phone_text_base(None, k, reference, (sw, sh), bar), k, "off the phone the window's own scale");
    }

    #[test]
    fn a_phone_s_box_text_follows_the_text_size() {
        // The hero window's diary, quest list and item description read `text_for`: on a phone the
        // phone's base times «Размер текста», so the text grows with the setting; the desktop keeps
        // the window's own scale.
        let (sw, sh, bar) = (2400.0, 1080.0, 120.0);
        let reference = crate::ui::dialog::TEXT_REF;
        let k = 1.5;
        let at = |t: f32| text_for_at(t, Some(2.625), k, reference, (sw, sh), bar);
        assert!(at(1.0) >= 2.6, "the phone's base: one px per dp");
        assert!(at(1.25) > at(1.0), "the large size is larger");
        assert!(at(1.5) > at(1.25), "the largest size is larger still");
        assert_eq!(text_for_at(1.25, None, k, reference, (sw, sh), bar), text_o_at(1.25, k), "the desktop keeps its scale");
    }

    #[test]
    fn text_sizes_cycle_and_an_unknown_one_goes_to_a_valid_size() {
        assert_eq!(next_text_size(1.0), 1.25);
        assert_eq!(next_text_size(1.25), 1.5);
        assert_eq!(next_text_size(1.5), 1.0);
        assert!(TEXT_SIZES.contains(&next_text_size(0.7)));
    }
}
