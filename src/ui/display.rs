//! The window's display mode (windowed, borderless, full screen) and the interface scale,
//! both chosen in the settings.
//!
//! miniquad only turns full screen on (on X11 it cannot even turn it off again) and has no
//! borderless window, so the modes are set here on each system's own window: X11 through a
//! second connection to the X server, Windows through the window's styles, macOS through the
//! `NSWindow`. Wayland is not used (miniquad runs on X11 alone by default).

use std::sync::atomic::{AtomicU32, Ordering};

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
/// reference (`chrome::k`); `0` is "Auto", the largest that fits the window.
pub const SCALES: [f32; 9] = [0.0, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0];

/// The chosen scale as `f32` bits (`0.0`: auto), read by `chrome::k` every call.
static SCALE: AtomicU32 = AtomicU32::new(0);

pub fn set_scale(scale: f32) {
    SCALE.store(scale.to_bits(), Ordering::Relaxed);
}

/// The interface scale for a window whose own fit is `fit`: the chosen one, as in Minecraft
/// never larger than what fits.
pub fn scale(fit: f32) -> f32 {
    let chosen = f32::from_bits(SCALE.load(Ordering::Relaxed));
    if chosen > 0.0 { chosen.min(fit) } else { fit }
}

/// The scale after `chosen` in the settings' cycle, skipping those the window cannot fit
/// (they would look the same as Auto).
pub fn next_scale(chosen: f32, fit: f32) -> f32 {
    let at = SCALES.iter().position(|&s| s == chosen).unwrap_or(0);
    (1..=SCALES.len()).map(|i| SCALES[(at + i) % SCALES.len()]).find(|&s| s == 0.0 || s < fit - 0.01).unwrap_or(0.0)
}

/// The settings' label of a scale, against the window's own fit.
pub fn scale_label(chosen: f32, fit: f32) -> String {
    use razdor::i18n::tr;
    if chosen > 0.0 && chosen < fit - 0.01 {
        format!("{chosen}×")
    } else {
        format!("{} ({:.1}×)", tr("Auto"), fit)
    }
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
    sys: sys::Window,
}

thread_local! {
    static STATE: std::cell::RefCell<State> =
        std::cell::RefCell::new(State { applied: None, target: None, since: 0.0, settled: true, sys: sys::Window::default() });
}

/// Seconds after a change when it is set once more.
const SETTLE_SECONDS: f64 = 0.5;

// ------------------------------------------------------------------------------------------
// Alt+Enter
// ------------------------------------------------------------------------------------------

/// Seconds the new mode's name stays on screen after Alt+Enter; it fades over the last 0.5.
const NOTICE_SECONDS: f64 = 2.0;

thread_local! {
    /// The mode Alt+Enter last chose and when.
    static NOTICE: std::cell::Cell<Option<(DisplayMode, f64)>> = const { std::cell::Cell::new(None) };
}

/// Alt+Enter went down this frame: the next display mode, on any screen (Razdor's, the
/// players asked for it: some overlays need a window, and a battle cannot be left for the
/// settings).
pub fn hotkey_pressed() -> bool {
    use crate::ui::input::{is_key_down, is_key_pressed};
    use macroquad::input::KeyCode;
    (is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt)) && (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter))
}

/// Shows `mode`'s name for a moment ([`draw_notice`]).
pub fn notice(mode: DisplayMode) {
    NOTICE.with(|n| n.set(Some((mode, macroquad::prelude::get_time()))));
}

/// The name of the mode Alt+Enter chose, at the top of the screen over everything else.
pub fn draw_notice() {
    use crate::ui::widgets::{measure, text_centered, ACCENT, PANEL};
    use macroquad::prelude::*;
    let Some((mode, at)) = NOTICE.with(|n| n.get()) else { return };
    let left = NOTICE_SECONDS - (get_time() - at);
    if left <= 0.0 {
        NOTICE.with(|n| n.set(None));
        return;
    }
    let a = (left / 0.5).clamp(0.0, 1.0) as f32;
    let k = crate::ui::chrome::k();
    let (size, h) = ((18.0 * k).max(14.0), (30.0 * k).max(24.0));
    let m = format!("{}: {}", razdor::i18n::tr("Screen"), mode.label());
    let w = measure(&m, size).width + 40.0 * k;
    let (cx, y) = (screen_width() / 2.0, 16.0 * k);
    draw_rectangle(cx - w / 2.0, y, w, h, Color { a: PANEL.a * a, ..PANEL });
    draw_rectangle_lines(cx - w / 2.0, y, w, h, 1.0, Color { a: 0.5 * a, ..crate::ui::chrome::SILVER });
    text_centered(&m, cx, y + h * 0.5 + size * 0.36, size, Color { a, ..ACCENT });
}

/// The settings' display mode and interface scale, taken in at the start of every frame.
pub fn follow_settings(settings: &crate::ui::audio::Settings) {
    if crate::ui::snapshot::size().is_some() {
        return; // Offscreen snapshots keep their fixed size and the automatic scale.
    }
    set_scale(settings.ui_scale);
    crate::ui::world_view::set_zoom_prefs(settings.map_zoom, settings.zoom_locked);
    follow(settings.display);
}

/// Puts the window in `mode` when it is not yet (called every frame; cheap when nothing
/// changes). The first call leaves a windowed start alone.
fn follow(mode: DisplayMode) {
    let now = macroquad::prelude::get_time();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
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
        pub fn apply(&mut self, from: DisplayMode, to: DisplayMode) -> bool {
            if !self.tried {
                self.tried = true;
                // SAFETY: plain Xlib calls on a connection of our own.
                self.conn = unsafe { Conn::open() };
                if self.conn.is_none() {
                    razdor::diag!("display mode: the game's X11 window was not found");
                }
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
    //! Windows: borderless and full screen are both a popup over the monitor; full screen
    //! also stays above the other windows (as a game's exclusive screen does).
    use super::DisplayMode;
    use std::ffi::c_void;

    type Hwnd = *mut c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
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
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcessId() -> u32;
    }

    const GWL_STYLE: i32 = -16;
    const WS_POPUP: isize = 0x8000_0000;
    const WS_VISIBLE: isize = 0x1000_0000;
    const WS_CLIPSIBLINGS: isize = 0x0400_0000;
    const WS_CLIPCHILDREN: isize = 0x0200_0000;
    const WS_CAPTION: isize = 0x00C0_0000;
    const WS_SYSMENU: isize = 0x0008_0000;
    const WS_SIZEBOX: isize = 0x0004_0000;
    const WS_MINIMIZEBOX: isize = 0x0002_0000;
    const WS_MAXIMIZEBOX: isize = 0x0001_0000;
    const SWP_FRAMECHANGED: u32 = 0x0020;
    const SWP_SHOWWINDOW: u32 = 0x0040;
    const HWND_TOPMOST: Hwnd = -1isize as Hwnd;
    const HWND_NOTOPMOST: Hwnd = -2isize as Hwnd;
    const MONITOR_DEFAULTTONEAREST: u32 = 2;

    #[derive(Default)]
    pub struct Window {
        hwnd: Option<usize>,
        windowed: Option<Rect>,
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

    impl Window {
        pub fn apply(&mut self, from: DisplayMode, to: DisplayMode) -> bool {
            // SAFETY: Win32 calls on our own window.
            unsafe {
                if self.hwnd.is_none() {
                    self.hwnd = find().map(|w| w as usize);
                }
                let Some(w) = self.hwnd.map(|w| w as Hwnd) else { return true };
                if from == DisplayMode::Windowed && to != DisplayMode::Windowed {
                    let mut r = Rect::default();
                    if GetWindowRect(w, &mut r) != 0 {
                        self.windowed = Some(r);
                    }
                }
                let flags = SWP_FRAMECHANGED | SWP_SHOWWINDOW;
                if to == DisplayMode::Windowed {
                    let style = WS_CLIPSIBLINGS | WS_CLIPCHILDREN | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SIZEBOX | WS_VISIBLE;
                    SetWindowLongPtrW(w, GWL_STYLE, style);
                    let r = self.windowed.unwrap_or(Rect { left: 64, top: 64, right: 64 + 1280, bottom: 64 + 800 });
                    SetWindowPos(w, HWND_NOTOPMOST, r.left, r.top, r.right - r.left, r.bottom - r.top, flags);
                } else {
                    let style = (GetWindowLongPtrW(w, GWL_STYLE) & !(WS_CAPTION | WS_SIZEBOX | WS_MAXIMIZEBOX)) | WS_POPUP | WS_VISIBLE;
                    SetWindowLongPtrW(w, GWL_STYLE, style);
                    let mut info = MonitorInfo { size: std::mem::size_of::<MonitorInfo>() as u32, monitor: Rect::default(), work: Rect::default(), flags: 0 };
                    GetMonitorInfoW(MonitorFromWindow(w, MONITOR_DEFAULTTONEAREST), &mut info);
                    let m = info.monitor;
                    let after = if to == DisplayMode::Fullscreen { HWND_TOPMOST } else { HWND_NOTOPMOST };
                    SetWindowPos(w, after, m.left, m.top, m.right - m.left, m.bottom - m.top, flags);
                }
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
    fn modes_cycle_and_read_back() {
        assert_eq!(DisplayMode::Windowed.next().next().next(), DisplayMode::Windowed);
        let json = serde_json::to_string(&DisplayMode::Fullscreen).unwrap();
        assert_eq!(json, "\"fullscreen\"");
        assert_eq!(serde_json::from_str::<DisplayMode>("\"borderless\"").unwrap(), DisplayMode::Borderless);
    }

    #[test]
    fn scale_never_exceeds_the_fit_and_skips_what_does_not_fit() {
        assert_eq!(next_scale(0.0, 1.5), 1.0);
        assert_eq!(next_scale(1.0, 1.5), 1.25, "a full HD screen's step between 1× and its fit");
        assert_eq!(next_scale(1.25, 1.5), 0.0, "1.5 is the fit itself: Auto");
        assert_eq!(scale_label(1.25, 1.5), "1.25×");
        assert_eq!(next_scale(0.0, 3.2), 1.0);
        assert_eq!(next_scale(3.0, 3.2), 0.0);
        assert_eq!(next_scale(2.0, 1.5), 0.0, "a scale that no longer fits moves on to Auto");
        assert_eq!(scale_label(2.0, 3.0), "2×");
    }
}
