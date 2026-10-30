//! Razdor's mouse and keys: macroquad's, merged with a gamepad's (`ui::gamepad`). The screens
//! read input through these functions, named and called as macroquad's (each file imports
//! them by name, which wins over `macroquad::prelude::*`), so a pad's A is a left click, its
//! pointer is the mouse's, its Start an Esc, wherever the game looks.
//!
//! The pad's pointer: it starts where the mouse is and, once the pad has moved it, is the
//! pointer until the real mouse moves or clicks again; then the mouse's is. While the pad's
//! pointer is in use the system pointer is hidden and Razdor draws one there (`ui::cursor`),
//! as the window's pointer cannot be moved. With no pad connected all of it is the mouse
//! and keyboard unchanged.
//!
//! On a phone the touches are the touch modes': the gestures (`ui::gestures`, the default), where
//! the finger is the pointer, or the touchpad mode (`ui::touchpad`, an option), where the touches
//! are a trackpad's and its pointer is the pointer from the start. Either way the clicks, wheel
//! and scroll merge with the pad's, and miniquad's simulated mouse is off, so a touch is never a
//! click of its own.

use razdor::local;
use std::cell::RefCell;
use std::collections::HashSet;

use macroquad::input as mq;
use macroquad::prelude::{screen_height, screen_width, KeyCode, MouseButton, Rect};

use super::gamepad::{self, Edge, Frame, Mapper};
use super::gestures::{self, GestureMapper, SwipeArea, SwipeKind};
use super::touchpad::{self, Phase, TouchMapper, TouchPoint};

#[derive(Default)]
struct State {
    mapper: Mapper,
    frame: Frame,
    /// The pad's pointer is the pointer, at `at`.
    active: bool,
    at: (f32, f32),
    /// The mouse at the last frame.
    mouse: Option<(f32, f32)>,
    /// The map screen asked for the right stick this frame and the last ([`map_scroll`]).
    map_claimed: (bool, bool),
    touch: TouchMapper,
    gestures: GestureMapper,
    /// The swipe areas registered this frame (`swipe_area`), and last frame's, which this
    /// frame's gestures use.
    swipe_now: Vec<SwipeArea>,
    swipe_prev: Vec<SwipeArea>,
    /// The touchpad mode was on at the last frame.
    touch_on: bool,
    /// A finger lifted and none has come down since (`hover_after`), and a finger is down now:
    /// the pointer is where the finger lifted, so no hover shows a hint (`touch_hover`).
    lifted: bool,
    down_now: bool,
    /// miniquad's simulated mouse has been turned off (phones, once).
    sim_off: bool,
    /// Virtual key taps (`tap_key`): queued in one frame, pressed in the next.
    taps: TapQueue,
    /// The screen's size at the last frame (zero before the first): a change of it on a phone
    /// is a turn of the device (`cursor_after_resize`).
    screen_seen: (f32, f32),
}

/// The pointer after the screen's size changed (`begin_frame`): on a phone a change of size is
/// a turn of the device, and the pointer goes to the middle of the new screen, as the player
/// asked (a pointer left in the new edge zone or off the new screen scrolled the map by
/// itself). Elsewhere, and while the size is unchanged, the pointer stays. Razdor's extra.
pub fn cursor_after_resize(old: (f32, f32), new: (f32, f32), phone: bool, cursor: (f32, f32)) -> (f32, f32) {
    let known = old.0 > 0.0 && old.1 > 0.0 && new.0 > 0.0 && new.1 > 0.0;
    if phone && known && old != new {
        (new.0 / 2.0, new.1 / 2.0)
    } else {
        cursor
    }
}

/// Virtual key presses for the screens: a key tapped in one frame is pressed in the next, once
/// (the F1 list's tiles run their keys this way, `ui::hotkeys`).
#[derive(Default, Debug)]
pub struct TapQueue {
    queued: Vec<KeyCode>,
    now: Vec<KeyCode>,
}

impl TapQueue {
    /// Queues `k` for the next frame.
    pub fn push(&mut self, k: KeyCode) {
        self.queued.push(k);
    }

    /// The start of a frame: what was queued before is pressed now.
    pub fn advance(&mut self) {
        self.now = std::mem::take(&mut self.queued);
    }

    /// Whether `k` is tapped this frame.
    pub fn pressed(&self, k: KeyCode) -> bool {
        self.now.contains(&k)
    }

    /// Every key tapped this frame.
    pub fn keys(&self) -> &[KeyCode] {
        &self.now
    }
}

/// Presses `k` on the next frame, as if the player had pressed it once: the screen under the
/// F1 list runs its action (quick save, the minimap, …) without a keyboard.
pub fn tap_key(k: KeyCode) {
    STATE.with(|s| s.borrow_mut().taps.push(k));
}

local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Once per frame, before anything reads input: reads the pad and works out its mouse.
pub fn begin_frame() {
    let pad = gamepad::poll();
    let mouse = mq::mouse_position();
    let mouse_used = [MouseButton::Left, MouseButton::Right, MouseButton::Middle].into_iter().any(mq::is_mouse_button_pressed);
    let screen = (screen_width(), screen_height());
    let phone = super::display::dp_px().is_some();
    // Out of focus the pad lets everything go: it is read even then, by its own device.
    let pad = pad.filter(|_| super::cursor::has_focus()).unwrap_or_default();
    let touchpad_on = touchpad::enabled();
    let gestures_on = gestures::enabled();
    let touches = if touchpad_on || gestures_on { touch_points() } else { Vec::new() };
    STATE.with(|s| {
        let s = &mut *s.borrow_mut();
        s.taps.advance();
        // Phones: miniquad's simulated mouse (a touch as the left button) is off in both touch modes.
        if touchpad::MOBILE && !s.sim_off {
            mq::simulate_mouse_with_touch(false);
            s.sim_off = true;
        }
        if s.mouse.is_some_and(|m| m != mouse) || mouse_used {
            s.active = false;
        }
        s.mouse = Some(mouse);
        // A turn of the phone: the pointer goes to the middle of the new screen (`cursor_after_resize`).
        // It is the pad's pointer then, so the stale mouse position (a touch's, before the turn)
        // is not the pointer, and the next touch or mouse move takes it back as usual.
        let resized = s.screen_seen.0 > 0.0 && s.screen_seen != screen;
        if phone && resized {
            s.active = true;
            s.at = cursor_after_resize(s.screen_seen, screen, phone, s.at);
        }
        s.screen_seen = screen;
        // The touchpad's pointer starts in the middle of the screen and stays the pointer.
        if touchpad_on && !s.touch_on {
            s.active = true;
            s.at = (screen.0 / 2.0, screen.1 / 2.0);
        } else if !touchpad_on && s.touch_on {
            s.active = false;
        }
        s.touch_on = touchpad_on;
        if touchpad_on || gestures_on {
            s.lifted = hover_after(s.lifted, &touches);
            s.down_now = touches.iter().any(|t| !matches!(t.phase, Phase::Ended | Phase::Cancelled));
        } else {
            s.lifted = false;
            s.down_now = false;
        }
        let from = if s.active { s.at } else { mouse };
        let map_scrolls = std::mem::take(&mut s.map_claimed.0);
        s.map_claimed.1 = map_scrolls;
        let dt = crate::ui::clock::frame_dt();
        // The swipe areas of the frame before: this frame's are registered while it is drawn.
        std::mem::swap(&mut s.swipe_prev, &mut s.swipe_now);
        s.swipe_now.clear();
        let pad_frame = s.mapper.step(&pad, dt, from, screen, map_scrolls);
        s.frame = if touchpad_on {
            merge(s.touch.step(&touches, dt, from, screen, map_scrolls), pad_frame)
        } else if gestures_on {
            merge(s.gestures.step(&touches, dt, screen, map_scrolls, &s.swipe_prev), pad_frame)
        } else {
            pad_frame
        };
        if let Some(at) = s.frame.pointer {
            s.active = true;
            s.at = at;
        }
        // The window grew smaller: the pointer stays on it.
        if s.active {
            s.at = (s.at.0.clamp(0.0, (screen.0 - 1.0).max(0.0)), s.at.1.clamp(0.0, (screen.1 - 1.0).max(0.0)));
        }
    });
}

/// This frame's touches, as the touchpad reads them.
fn touch_points() -> Vec<TouchPoint> {
    mq::touches()
        .into_iter()
        .map(|t| TouchPoint {
            id: t.id,
            x: t.position.x,
            y: t.position.y,
            phase: match t.phase {
                mq::TouchPhase::Started => Phase::Started,
                mq::TouchPhase::Stationary => Phase::Stationary,
                mq::TouchPhase::Moved => Phase::Moved,
                mq::TouchPhase::Ended => Phase::Ended,
                mq::TouchPhase::Cancelled => Phase::Cancelled,
            },
        })
        .collect()
}

/// The touch's frame and the pad's, as one: the pointer is the touch's when it moved, a button
/// is held when either holds it, the wheel turns with both, and the map's scroll is the touch's
/// while its fingers push.
fn merge(touch: Frame, pad: Frame) -> Frame {
    let either = |a: Edge, b: Edge| Edge { pressed: a.pressed || b.pressed, down: a.down || b.down, released: a.released || b.released };
    let mut keys = pad.keys;
    keys.extend(touch.keys);
    Frame {
        pointer: touch.pointer.or(pad.pointer),
        left: either(touch.left, pad.left),
        right: either(touch.right, pad.right),
        wheel: touch.wheel + pad.wheel,
        keys,
        scroll: if touch.scroll != (0.0, 0.0) { touch.scroll } else { pad.scroll },
        pan: (touch.pan.0 + pad.pan.0, touch.pan.1 + pad.pan.1),
    }
}

/// Whether a hover may show a hint (`widgets::tooltip`, `widgets::tooltip_later`, the bar's
/// hover hint). On a phone a finger's lift leaves the pointer where it lifted, so no hint shows
/// until a finger is down again; a long press is a finger down, so its info still shows. Always
/// true with no touch mode on.
pub fn touch_hover() -> bool {
    STATE.with(|s| {
        let s = s.borrow();
        hover_shown(s.lifted, s.down_now)
    })
}

/// Razdor's one delay for every hint shown on a hold or a rest (the owner's decision, 2026-10-30):
/// a phone's long press (`gestures`) lasts this long, and so does the hover of a hint.
pub const HINT_DELAY_SECS: f32 = 0.35;

/// Whether a hover may show a description or hint this frame. On a phone's gesture mode only
/// while a long press holds (the owner's choice, 2026-10-30: every hint on a long press, as the
/// map's). In the touchpad mode while the cursor has rested [`HINT_DELAY_SECS`] still
/// (`rest::rested`, the same delay; a touch alone does not count there). With no touch, as
/// [`touch_hover`].
pub fn hint_hover() -> bool {
    let long_pressed = STATE.with(|s| s.borrow().gestures.long_pressed());
    hint_shown(
        gestures::enabled(),
        touchpad::enabled(),
        long_pressed,
        super::rest::rested(),
        touch_hover(),
    )
}

/// The gesture mode's hints follow its long press; the touchpad mode's follow the cursor's rest;
/// the other modes' follow the touch hover.
fn hint_shown(gestures_on: bool, touchpad_on: bool, long_pressed: bool, rested: bool, touched: bool) -> bool {
    if gestures_on {
        long_pressed
    } else if touchpad_on {
        rested
    } else {
        touched
    }
}

/// The lift latch after this frame's `touches`: a finger coming down clears it, a lift sets it,
/// and a frame with neither keeps it. A touch that came down and up within one frame arrives
/// only as its lift, so it counts as one.
fn hover_after(lifted: bool, touches: &[TouchPoint]) -> bool {
    if touches.iter().any(|t| t.phase == Phase::Started) {
        false
    } else if touches.iter().any(|t| matches!(t.phase, Phase::Ended | Phase::Cancelled)) {
        true
    } else {
        lifted
    }
}

/// A hover shows a hint unless a finger has lifted and none is down now.
fn hover_shown(lifted: bool, down: bool) -> bool {
    !lifted || down
}

/// Registers a rectangle a one-finger swipe acts on (gestures only; a no-op elsewhere): the
/// map's `SwipeKind::Pan`, a list's or text box's `ScrollRows` (see `widgets::swipe_rows`),
/// `SwipeKind::Off` for a control a drag must stay a drag on. Called while the screen is
/// drawn: the next frame's gestures use it, the last one registered over a point acting.
pub fn swipe_area(rect: Rect, kind: SwipeKind) {
    if !gestures::enabled() {
        return;
    }
    STATE.with(|s| s.borrow_mut().swipe_now.push((rect, kind)));
}

/// The map's pan this frame: the finger's slide over the map, in screen pixels.
pub fn map_pan() -> (f32, f32) {
    with_frame(|f| f.pan)
}

/// Where the pad's pointer is, while it is the pointer.
pub fn pad_pointer() -> Option<(f32, f32)> {
    STATE.with(|s| {
        let s = s.borrow();
        s.active.then_some(s.at)
    })
}

/// The map screen scrolls with the right stick: called each frame it shows, it gets the
/// stick (x right, y down, -1..1) and from the next frame the stick no longer turns the
/// wheel (the map's zoom).
pub fn map_scroll() -> (f32, f32) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.map_claimed.0 = true;
        s.frame.scroll
    })
}

fn with_frame<R>(f: impl FnOnce(&Frame) -> R) -> R {
    STATE.with(|s| f(&s.borrow().frame))
}

fn button(b: MouseButton) -> Edge {
    with_frame(|f| match b {
        MouseButton::Left => f.left,
        MouseButton::Right => f.right,
        _ => Edge::default(),
    })
}

fn key_edge(k: KeyCode) -> Edge {
    with_frame(|f| f.keys.iter().find(|(c, _)| *c == k).map(|(_, e)| *e).unwrap_or_default())
}

fn keys_where(pick: impl Fn(&Edge) -> bool) -> Vec<KeyCode> {
    with_frame(|f| f.keys.iter().filter(|(_, e)| pick(e)).map(|(k, _)| *k).collect())
}

/// The pointer: the pad's while it is in use, else the mouse's; on the canvas, so the pan
/// (`ui::pan`) is taken off it.
pub fn mouse_position() -> (f32, f32) {
    super::pan::to_canvas(screen_pointer(), super::pan::shift())
}

/// The pointer on the screen: the pad's while it is in use, else the mouse's (not panned).
pub fn screen_pointer() -> (f32, f32) {
    pad_pointer().unwrap_or_else(mq::mouse_position)
}

/// The wheel this frame (x, y; up is positive): the mouse's and the pad's.
pub fn mouse_wheel() -> (f32, f32) {
    let (x, y) = mq::mouse_wheel();
    (x, y + with_frame(|f| f.wheel))
}

pub fn is_mouse_button_pressed(b: MouseButton) -> bool {
    mq::is_mouse_button_pressed(b) || button(b).pressed
}

pub fn is_mouse_button_down(b: MouseButton) -> bool {
    mq::is_mouse_button_down(b) || button(b).down || (b == MouseButton::Right && SNAPSHOT_RIGHT.with(|r| r.get()))
}

local! {
    /// Debug snapshots: the right button is held (`RAZDOR_MOUSE_RIGHT=1`, `ui::snapshot`).
    static SNAPSHOT_RIGHT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Holds the right button down for good (debug snapshots only: the map's tooltips).
pub fn hold_right_for_snapshot(on: bool) {
    SNAPSHOT_RIGHT.with(|r| r.set(on));
}

pub fn is_mouse_button_released(b: MouseButton) -> bool {
    mq::is_mouse_button_released(b) || button(b).released
}

pub fn is_key_pressed(k: KeyCode) -> bool {
    mq::is_key_pressed(k) || key_edge(k).pressed || with_taps(|t| t.pressed(k))
}

pub fn is_key_down(k: KeyCode) -> bool {
    mq::is_key_down(k) || key_edge(k).down
}

pub fn get_keys_pressed() -> HashSet<KeyCode> {
    let mut keys = mq::get_keys_pressed();
    keys.extend(keys_where(|e| e.pressed));
    keys.extend(with_taps(|t| t.keys().to_vec()));
    keys
}

fn with_taps<R>(f: impl FnOnce(&TapQueue) -> R) -> R {
    STATE.with(|s| f(&s.borrow().taps))
}

pub fn get_keys_released() -> HashSet<KeyCode> {
    let mut keys = mq::get_keys_released();
    keys.extend(keys_where(|e| e.released));
    keys
}

/// `k` went down this frame on the pad only (not on the keyboard).
pub fn pad_key_pressed(k: KeyCode) -> bool {
    key_edge(k).pressed && !mq::is_key_pressed(k)
}

#[cfg(test)]
mod tests {
    #[test]
    fn hints_follow_the_long_press_in_gestures_and_the_touch_hover_elsewhere() {
        use super::hint_shown;
        // Gestures: a hover with no long press shows nothing; a long press shows, lifted or not.
        assert!(!hint_shown(true, false, false, false, true));
        assert!(hint_shown(true, false, true, false, false));
        // Touchpad: as the cursor's rest says, whatever a finger does and whatever the long press.
        assert!(hint_shown(false, true, false, true, false));
        assert!(hint_shown(false, true, false, true, true));
        assert!(!hint_shown(false, true, false, false, true));
        assert!(!hint_shown(false, true, true, false, false));
        // Desktop: as the touch hover says, whatever the long press or the rest.
        assert!(hint_shown(false, false, false, false, true));
        assert!(!hint_shown(false, false, true, true, false));
        assert!(!hint_shown(false, false, false, false, false));
    }

    /// After a lift no hover shows a hint until a finger comes down; a finger held still (a long
    /// press) keeps them, and a tap that starts and ends in one frame counts as a lift.
    #[test]
    fn a_lift_hides_the_hover_hints_until_a_finger_comes_down() {
        use super::{hover_after, hover_shown};
        use crate::ui::touchpad::{Phase, TouchPoint};
        let t = |phase| TouchPoint { id: 1, x: 0.0, y: 0.0, phase };
        let mut lifted = false;
        lifted = hover_after(lifted, &[t(Phase::Started)]);
        assert!(hover_shown(lifted, true));
        lifted = hover_after(lifted, &[t(Phase::Stationary)]);
        assert!(hover_shown(lifted, true));
        lifted = hover_after(lifted, &[t(Phase::Ended)]);
        assert!(!hover_shown(lifted, false));
        lifted = hover_after(lifted, &[]);
        assert!(!hover_shown(lifted, false));
        lifted = hover_after(lifted, &[t(Phase::Started)]);
        assert!(hover_shown(lifted, true));
        let quick = hover_after(false, &[t(Phase::Ended)]);
        assert!(!hover_shown(quick, false));
        assert!(hover_shown(false, false));
    }

    /// Every file of the interface reads the mouse and keys through this module: one calling
    /// macroquad's functions of the same names (through its prelude) would not see the pad.
    #[test]
    fn the_screens_read_input_through_razdors_layer() {
        const NAMES: [&str; 11] = [
            "mouse_position",
            "mouse_wheel",
            "is_mouse_button_pressed",
            "is_mouse_button_down",
            "is_mouse_button_released",
            "is_key_pressed",
            "is_key_down",
            "is_key_released",
            "get_keys_pressed",
            "get_keys_released",
            "get_keys_down",
        ];
        fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    files(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let mut all = Vec::new();
        files(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui"), &mut all);
        let mut wrong = Vec::new();
        for path in all {
            if path.ends_with("input.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            // The names this file imports from `input` (`use …input::{a, b};`), Razdor's or
            // macroquad's.
            let imports = |ours: bool| -> Vec<&str> {
                src.match_indices("input::{")
                    .filter(|(i, _)| src[..*i].ends_with("macroquad::") != ours)
                    .filter_map(|(i, m)| src[i + m.len()..].split_once('}').map(|(list, _)| list))
                    .flat_map(|list| list.split(',').map(str::trim))
                    .collect()
            };
            let (imported, macroquads) = (imports(true), imports(false));
            let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).map(|l| format!("{l}\n")).collect();
            for name in NAMES {
                // Used: called or passed on (`unwrap_or_else(mouse_position)`), outside comments.
                let ident = |c: char| c.is_alphanumeric() || c == '_';
                let used = code.match_indices(name).any(|(i, _)| {
                    let before = code[..i].chars().next_back();
                    let after = code[i + name.len()..].chars().next();
                    !before.is_some_and(|c| ident(c) || c == ':') && !after.is_some_and(ident) && !code[..i].ends_with("fn ")
                });
                let imported = imported.contains(&name) || src.contains(&format!("use input::{name};")) || src.contains(&format!("use crate::ui::input::{name};"));
                let qualified = macroquads.contains(&name) || src.contains(&format!("macroquad::input::{name}")) || src.contains(&format!("macroquad::prelude::{name}"));
                if (used && !imported) || qualified {
                    wrong.push(format!("{}: {name}", path.display()));
                }
            }
        }
        assert!(wrong.is_empty(), "read macroquad's input directly instead of ui::input's:\n{}", wrong.join("\n"));
    }
}

#[cfg(test)]
mod resize_tests {
    use super::cursor_after_resize;

    /// A turn of a phone puts the pointer in the middle of the new screen, wherever it was.
    #[test]
    fn a_turn_of_the_phone_puts_the_pointer_in_the_middle_of_the_new_screen() {
        assert_eq!(cursor_after_resize((800.0, 400.0), (400.0, 800.0), true, (790.0, 395.0)), (200.0, 400.0));
        assert_eq!(cursor_after_resize((400.0, 800.0), (800.0, 400.0), true, (2.0, 2.0)), (400.0, 200.0));
    }

    /// The desktop's window resize does not warp the mouse, and a phone's unchanged size neither.
    #[test]
    fn a_resize_on_the_desktop_or_no_change_keeps_the_pointer() {
        assert_eq!(cursor_after_resize((800.0, 400.0), (400.0, 800.0), false, (790.0, 395.0)), (790.0, 395.0));
        assert_eq!(cursor_after_resize((800.0, 400.0), (800.0, 400.0), true, (790.0, 395.0)), (790.0, 395.0));
    }

    /// The first frame has no size before it, so nothing moves.
    #[test]
    fn the_first_frame_keeps_the_pointer() {
        assert_eq!(cursor_after_resize((0.0, 0.0), (400.0, 800.0), true, (10.0, 10.0)), (10.0, 10.0));
    }
}

#[cfg(test)]
mod tap_tests {
    use super::*;

    /// A tap is pressed on the frame after it is queued, once, and only that key.
    #[test]
    fn a_tap_is_pressed_once_on_the_next_frame() {
        let mut q = TapQueue::default();
        q.push(KeyCode::F5);
        assert!(!q.pressed(KeyCode::F5), "not in the frame it is queued");
        q.advance();
        assert!(q.pressed(KeyCode::F5));
        assert!(!q.pressed(KeyCode::F9));
        assert_eq!(q.keys(), &[KeyCode::F5]);
        q.advance();
        assert!(!q.pressed(KeyCode::F5), "gone the frame after");
        assert!(q.keys().is_empty());
    }

    /// Taps queued in one frame all come out together in the next.
    #[test]
    fn taps_queued_together_are_pressed_together() {
        let mut q = TapQueue::default();
        q.push(KeyCode::M);
        q.push(KeyCode::J);
        q.advance();
        assert!(q.pressed(KeyCode::M) && q.pressed(KeyCode::J));
    }
}
