//! Touchpad mode (a Razdor extra; the original has no touch input), an option: off by default,
//! the standard gestures are the touch there (`ui::gestures`). On a phone or tablet with it on
//! the screen is a laptop trackpad: one finger sliding moves a cursor that Razdor draws
//! (`ui::cursor`); a tap is a left click where the cursor is; a second tap that is held and
//! slid drags with the left button (units, scroll bars, windows); two fingers tapped are a
//! right click, two held still hold the right button (the map's tooltips); two sliding
//! together scroll the map or turn the wheel (the lists and the shop). [`TouchMapper`] turns
//! the touches of a frame into that mouse, purely; `ui::input` feeds it macroquad's touches
//! and merges its [`Frame`] with the mouse's and the pad's. Distances are pixels of a 1080 px
//! high screen, scaled to the screen's height, so a gesture feels the same on any phone.

use razdor::local;
use std::cell::Cell;

use super::gamepad::Frame;

/// Phones and tablets (`platform::MOBILE`): where the touch modes run and the setting shows.
pub use razdor::platform::MOBILE;

local! {
    /// The setting (`Settings::touchpad`), pushed in by `main_menu::apply_options`.
    static SETTING: Cell<Option<bool>> = const { Cell::new(None) };
}

/// The setting as the player chose it (`None`: not chosen yet, which is off).
pub fn set_setting(on: Option<bool>) {
    SETTING.with(|s| s.set(on));
}

/// The touchpad mode is on: on mobile, when the player turned it on (off by default).
pub fn enabled() -> bool {
    MOBILE && SETTING.with(|s| s.get()) == Some(true)
}

/// A touch's phase, as macroquad reports it: down, then still or moved, then up once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Started,
    Stationary,
    Moved,
    Ended,
    Cancelled,
}

/// One finger this frame: its id (kept from its touch to its lift), where it is, its phase.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchPoint {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub phase: Phase,
}

/// A tap is down and up within this many seconds and slides less than [`TAP_SLOP`] pixels.
const TAP_TIME: f32 = 0.25;
const TAP_SLOP: f32 = 12.0;
/// The pointer moves this many pixels for each pixel the finger slides.
const SPEED: f32 = 1.6;
/// A second finger down within this many seconds of a tap's lift, within [`DOUBLE_SLOP`] pixels
/// of where the tap went down, is a double tap: its left button is held while it lasts.
const DOUBLE_TIME: f32 = 0.3;
const DOUBLE_SLOP: f32 = 40.0;
/// Two fingers lifted within this many seconds of the second one's touch are a right click;
/// still for longer, they hold the right button until one lifts.
const TWO_TIME: f32 = 0.3;
/// Two fingers sliding turn the wheel a step per this many pixels.
const WHEEL_STEP: f32 = 40.0;

/// A finger from its touch to its lift.
#[derive(Clone, Copy, Debug)]
struct Finger {
    id: u64,
    last: (f32, f32),
    /// Where it touched down, and when (the mapper's clock, seconds).
    down: (f32, f32),
    since: f32,
    /// The path it has slid, pixels.
    travel: f32,
}

/// The touches from the first finger down until none is.
#[derive(Clone, Copy, Debug, Default)]
struct Session {
    /// The most fingers down at once.
    most: usize,
    /// It began as the second tap of a double tap: the left button is held from its touch.
    double: bool,
    /// Three fingers or more came: nothing happens until they are all up.
    unclear: bool,
    /// Two fingers: when the second came down, where they were together, and whether they
    /// have slid on from still (then they scroll: no right click, no hold).
    two_at: f32,
    two_mid: (f32, f32),
    sliding: bool,
}

/// Turns the touches into the touchpad's mouse, frame by frame.
#[derive(Clone, Debug, Default)]
pub struct TouchMapper {
    /// The mapper's clock: the frames' dt added up (a long frame counts as 0.1 s).
    now: f32,
    /// The fingers down now.
    fingers: Vec<Finger>,
    session: Session,
    /// The last single-finger tap: when it lifted, and where it went down (for a double tap).
    last_tap: Option<(f32, (f32, f32))>,
    /// The left button is held by a double tap, until its finger lifts.
    left_held: bool,
    /// The right button is held by two fingers still, until one lifts.
    right_held: bool,
    /// A click made by the last frame's lift: let go this frame.
    release_left: bool,
    release_right: bool,
    /// The wheel turned by the two fingers' slide, not yet a whole step (pixels).
    wheel: f32,
}

impl TouchMapper {
    /// The frame for this frame's `touches` after `dt` seconds, the pointer at `at` on a
    /// `screen` (w, h) window. `map_scrolls`: two fingers sliding scroll the map (as the stick
    /// does), else they turn the wheel.
    pub fn step(&mut self, touches: &[TouchPoint], dt: f32, at: (f32, f32), screen: (f32, f32), map_scrolls: bool) -> Frame {
        let dt = dt.clamp(0.0, 0.1);
        self.now += dt;
        let scale = |v: f32| v * screen.1 / 1080.0;
        let mut f = Frame::default();
        if std::mem::take(&mut self.release_left) {
            f.left.released = true;
        }
        if std::mem::take(&mut self.release_right) {
            f.right.released = true;
        }

        // Downs and lifts change the touches; the slides of the fingers that stayed down add up.
        let mut slide = (0.0f32, 0.0f32);
        for t in touches {
            let pos = (t.x, t.y);
            let known = self.fingers.iter().position(|g| g.id == t.id);
            match (known, t.phase) {
                (Some(i), Phase::Ended | Phase::Cancelled) => {
                    let mut g = self.fingers.remove(i);
                    g.travel += dist(g.last, pos);
                    self.lift(g, &mut f, scale);
                }
                (Some(i), _) => {
                    let g = &mut self.fingers[i];
                    let d = (pos.0 - g.last.0, pos.1 - g.last.1);
                    g.travel += d.0.hypot(d.1);
                    g.last = pos;
                    slide = (slide.0 + d.0, slide.1 + d.1);
                }
                (None, Phase::Ended | Phase::Cancelled) => {}
                (None, _) => {
                    let g = Finger { id: t.id, last: pos, down: pos, since: self.now, travel: 0.0 };
                    self.down(g, &mut f, scale);
                }
            }
        }
        if self.session.unclear {
            return f;
        }

        match self.fingers.len() {
            1 if slide != (0.0, 0.0) => {
                // One finger moves the pointer, wherever on the screen it is.
                let k = SPEED * screen.1 / 1080.0;
                let (w, h) = ((screen.0 - 1.0).max(0.0), (screen.1 - 1.0).max(0.0));
                f.pointer = Some(((at.0 + slide.0 * k).clamp(0.0, w), (at.1 + slide.1 * k).clamp(0.0, h)));
            }
            2 => {
                let mean = (slide.0 / 2.0, slide.1 / 2.0);
                if !self.session.sliding && dist(mid(&self.fingers), self.session.two_mid) > scale(TAP_SLOP) {
                    self.session.sliding = true;
                    if std::mem::take(&mut self.right_held) {
                        f.right.released = true;
                    }
                }
                if self.session.sliding {
                    if map_scrolls {
                        // The stick: a screen's height a second is full push.
                        self.wheel = 0.0;
                        if dt > 0.0 {
                            let stick = |v: f32| (-v / (dt * screen.1)).clamp(-1.0, 1.0);
                            f.scroll = (stick(mean.0), stick(mean.1));
                        }
                    } else {
                        // Fingers moving up scroll the content down, so they turn the wheel down.
                        let step = scale(WHEEL_STEP);
                        self.wheel += mean.1;
                        let steps = (self.wheel / step).trunc();
                        self.wheel -= steps * step;
                        f.wheel += steps;
                    }
                } else {
                    self.wheel = 0.0;
                    if !self.right_held && self.now - self.session.two_at >= TWO_TIME {
                        self.right_held = true;
                        f.right.pressed = true;
                        f.right.down = true;
                    }
                }
            }
            _ => {}
        }
        // A held button stays down for every frame it is held.
        f.left.down |= self.left_held;
        f.right.down |= self.right_held;
        f
    }

    /// A finger came down: a session starts with the first; a second one makes two fingers;
    /// a third ends the session's gestures (`unclear`), letting a held button go.
    fn down(&mut self, g: Finger, f: &mut Frame, scale: impl Fn(f32) -> f32) {
        self.fingers.push(g);
        let n = self.fingers.len();
        if n == 1 {
            let last = self.last_tap.take();
            let double = last.is_some_and(|(lift, at)| self.now - lift <= DOUBLE_TIME && dist(at, g.down) <= scale(DOUBLE_SLOP));
            self.session = Session { most: 1, double, ..Session::default() };
            if double {
                self.left_held = true;
                f.left.pressed = true;
                f.left.down = true;
            }
            return;
        }
        self.session.most = self.session.most.max(n);
        if self.session.unclear {
            return;
        }
        if n == 2 {
            // Two fingers are no double tap: a held left button lets go.
            self.session.double = false;
            if std::mem::take(&mut self.left_held) {
                f.left.released = true;
            }
            self.session.two_at = self.now;
            self.session.two_mid = mid(&self.fingers);
            self.session.sliding = false;
        } else {
            self.session.unclear = true;
            if std::mem::take(&mut self.left_held) {
                f.left.released = true;
            }
            if std::mem::take(&mut self.right_held) {
                f.right.released = true;
            }
        }
    }

    /// A finger lifted (`g` is it, already out of `fingers`): a single finger's tap clicks, the
    /// first of two lifting makes a right click (or lets a held one go); the last one ends the
    /// session and lets go of whatever is still held.
    fn lift(&mut self, g: Finger, f: &mut Frame, scale: impl Fn(f32) -> f32) {
        let rest = self.fingers.len();
        let session = self.session;
        if !session.unclear {
            match (session.most, rest) {
                (1, 0) if !self.left_held => {
                    if g.travel <= scale(TAP_SLOP) && self.now - g.since <= TAP_TIME {
                        f.left.pressed = true;
                        f.left.down = true;
                        self.release_left = true;
                        self.last_tap = Some((self.now, g.down));
                    }
                }
                (2, 1) => {
                    if std::mem::take(&mut self.right_held) {
                        f.right.released = true;
                    } else if !session.sliding && self.now - session.two_at <= TWO_TIME {
                        f.right.pressed = true;
                        f.right.down = true;
                        self.release_right = true;
                    }
                }
                _ => {}
            }
        }
        if rest == 0 {
            if std::mem::take(&mut self.left_held) {
                f.left.released = true;
            }
            if std::mem::take(&mut self.right_held) {
                f.right.released = true;
            }
            self.session = Session::default();
            self.wheel = 0.0;
        }
    }
}

/// The middle of the fingers' last positions.
fn mid(fingers: &[Finger]) -> (f32, f32) {
    let n = fingers.len().max(1) as f32;
    let (x, y) = fingers.iter().fold((0.0, 0.0), |(x, y), g| (x + g.last.0, y + g.last.1));
    (x / n, y / n)
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::gamepad::Edge;

    /// A 1080 px high screen: the pixel distances are the constants as they are.
    const SCREEN: (f32, f32) = (720.0, 1080.0);
    const DT: f32 = 1.0 / 60.0;

    fn touch(id: u64, x: f32, y: f32, phase: Phase) -> TouchPoint {
        TouchPoint { id, x, y, phase }
    }

    fn step(m: &mut TouchMapper, touches: &[TouchPoint], at: (f32, f32), map_scrolls: bool) -> Frame {
        m.step(touches, DT, at, SCREEN, map_scrolls)
    }

    fn held(f: &Frame) -> bool {
        f.left.down || f.right.down
    }

    #[test]
    fn a_resting_finger_does_nothing() {
        let mut m = TouchMapper::default();
        assert_eq!(step(&mut m, &[touch(1, 100.0, 100.0, Phase::Started)], (360.0, 540.0), false), Frame::default());
        for _ in 0..5 {
            assert_eq!(step(&mut m, &[touch(1, 100.0, 100.0, Phase::Stationary)], (360.0, 540.0), false), Frame::default());
        }
    }

    #[test]
    fn a_slide_moves_the_pointer_and_does_not_click() {
        let mut m = TouchMapper::default();
        let at = (360.0, 540.0);
        step(&mut m, &[touch(1, 100.0, 500.0, Phase::Started)], at, false);
        // The finger's place on the screen does not matter: 30 px of slide is 48 px of pointer.
        let f = step(&mut m, &[touch(1, 130.0, 500.0, Phase::Moved)], at, false);
        assert_eq!(f.pointer, Some((408.0, 540.0)));
        assert!(!held(&f) && !f.left.pressed && !f.right.pressed);
        let f = step(&mut m, &[touch(1, 130.0, 500.0, Phase::Ended)], (408.0, 540.0), false);
        assert_eq!((f.left, f.right), (Edge::default(), Edge::default()));
    }

    #[test]
    fn the_pointer_stays_on_the_screen() {
        let mut m = TouchMapper::default();
        step(&mut m, &[touch(1, 400.0, 500.0, Phase::Started)], (2.0, 2.0), false);
        let f = step(&mut m, &[touch(1, 370.0, 500.0, Phase::Moved)], (2.0, 2.0), false);
        assert_eq!(f.pointer, Some((0.0, 2.0)));
        // Down the screen: 500 px of slide is 800 px of pointer, clamped to the bottom.
        let f = step(&mut m, &[touch(1, 370.0, 1000.0, Phase::Moved)], (700.0, 1070.0), false);
        assert_eq!(f.pointer, Some((700.0, 1079.0)));
    }

    #[test]
    fn a_tap_clicks_over_two_frames() {
        let mut m = TouchMapper::default();
        let at = (200.0, 300.0);
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Started)], at, false);
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Stationary)], at, false);
        let lift = step(&mut m, &[touch(1, 50.0, 50.0, Phase::Ended)], at, false);
        assert_eq!(lift.left, Edge { pressed: true, down: true, released: false });
        assert_eq!(lift.pointer, None);
        let after = step(&mut m, &[], at, false);
        assert_eq!(after.left, Edge { pressed: false, down: false, released: true });
        assert_eq!(step(&mut m, &[], at, false), Frame::default());
    }

    #[test]
    fn a_long_press_is_not_a_tap() {
        let mut m = TouchMapper::default();
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Started)], (0.0, 0.0), false);
        for _ in 0..20 {
            step(&mut m, &[touch(1, 50.0, 50.0, Phase::Stationary)], (0.0, 0.0), false);
        }
        let f = step(&mut m, &[touch(1, 50.0, 50.0, Phase::Ended)], (0.0, 0.0), false);
        assert_eq!((f.left, f.right), (Edge::default(), Edge::default()));
    }

    #[test]
    fn a_double_tap_drags_with_the_left_button() {
        let mut m = TouchMapper::default();
        let at = (200.0, 300.0);
        // The first tap clicks.
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Started)], at, false);
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Ended)], at, false);
        step(&mut m, &[], at, false);
        // The second finger, close to the first, holds the left button and slides.
        let down = step(&mut m, &[touch(2, 55.0, 52.0, Phase::Started)], at, false);
        assert_eq!(down.left, Edge { pressed: true, down: true, released: false });
        let slide = step(&mut m, &[touch(2, 65.0, 52.0, Phase::Moved)], at, false);
        assert!(slide.left.down && !slide.left.pressed && !slide.left.released);
        assert_eq!(slide.pointer, Some((216.0, 300.0)));
        let lift = step(&mut m, &[touch(2, 65.0, 52.0, Phase::Ended)], (216.0, 300.0), false);
        assert_eq!(lift.left, Edge { pressed: false, down: false, released: true });
        assert_eq!(step(&mut m, &[], at, false), Frame::default());
    }

    #[test]
    fn a_double_tap_without_a_slide_is_two_clicks() {
        let mut m = TouchMapper::default();
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Started)], (0.0, 0.0), false);
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Ended)], (0.0, 0.0), false);
        step(&mut m, &[], (0.0, 0.0), false);
        let second = step(&mut m, &[touch(2, 50.0, 50.0, Phase::Started)], (0.0, 0.0), false);
        assert!(second.left.pressed);
        let lift = step(&mut m, &[touch(2, 50.0, 50.0, Phase::Ended)], (0.0, 0.0), false);
        assert_eq!(lift.left, Edge { pressed: false, down: false, released: true });
    }

    #[test]
    fn a_tap_far_from_the_last_one_is_a_single_click() {
        let mut m = TouchMapper::default();
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Started)], (0.0, 0.0), false);
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Ended)], (0.0, 0.0), false);
        step(&mut m, &[], (0.0, 0.0), false);
        // Far away (over 40 px): a new tap, which clicks on its lift, not on its touch.
        let down = step(&mut m, &[touch(2, 300.0, 300.0, Phase::Started)], (0.0, 0.0), false);
        assert_eq!(down.left, Edge::default());
        let lift = step(&mut m, &[touch(2, 300.0, 300.0, Phase::Ended)], (0.0, 0.0), false);
        assert_eq!(lift.left, Edge { pressed: true, down: true, released: false });
    }

    #[test]
    fn two_fingers_tapped_are_a_right_click() {
        let mut m = TouchMapper::default();
        let at = (100.0, 100.0);
        step(&mut m, &[touch(1, 100.0, 100.0, Phase::Started)], at, false);
        step(&mut m, &[touch(1, 100.0, 100.0, Phase::Stationary), touch(2, 300.0, 100.0, Phase::Started)], at, false);
        let lift = step(&mut m, &[touch(1, 100.0, 100.0, Phase::Ended), touch(2, 300.0, 100.0, Phase::Stationary)], at, false);
        assert_eq!(lift.right, Edge { pressed: true, down: true, released: false });
        assert_eq!(lift.left, Edge::default());
        let after = step(&mut m, &[touch(2, 300.0, 100.0, Phase::Ended)], at, false);
        assert_eq!(after.right, Edge { pressed: false, down: false, released: true });
        assert_eq!((after.left, after.pointer), (Edge::default(), None));
    }

    #[test]
    fn two_fingers_held_still_hold_the_right_button_until_one_lifts() {
        let mut m = TouchMapper::default();
        let at = (100.0, 100.0);
        step(&mut m, &[touch(1, 100.0, 100.0, Phase::Started), touch(2, 300.0, 100.0, Phase::Started)], at, false);
        let mut pressed = None;
        for i in 0..40 {
            let f = step(&mut m, &[touch(1, 100.0, 100.0, Phase::Stationary), touch(2, 300.0, 100.0, Phase::Stationary)], at, false);
            if f.right.pressed {
                pressed = Some(i);
                assert!(f.right.down && !f.left.down);
                break;
            }
        }
        assert!(pressed.is_some_and(|i| i >= 15), "{pressed:?}");
        let f = step(&mut m, &[touch(1, 100.0, 100.0, Phase::Stationary), touch(2, 300.0, 100.0, Phase::Stationary)], at, false);
        assert!(f.right.down && !f.right.pressed);
        let lift = step(&mut m, &[touch(1, 100.0, 100.0, Phase::Ended), touch(2, 300.0, 100.0, Phase::Stationary)], at, false);
        assert_eq!(lift.right, Edge { pressed: false, down: false, released: true });
        assert_eq!(step(&mut m, &[touch(2, 300.0, 100.0, Phase::Ended)], at, false), Frame::default());
    }

    #[test]
    fn two_fingers_sliding_scroll_the_map_and_are_no_click() {
        let mut m = TouchMapper::default();
        let at = (100.0, 100.0);
        step(&mut m, &[touch(1, 300.0, 500.0, Phase::Started), touch(2, 400.0, 500.0, Phase::Started)], at, true);
        let mut last = Frame::default();
        for i in 0..5 {
            // Both fingers slide left (10 px a frame): the view scrolls right.
            let x = 300.0 - 10.0 * (i + 1) as f32;
            last = step(&mut m, &[touch(1, x, 500.0, Phase::Moved), touch(2, x + 100.0, 500.0, Phase::Moved)], at, true);
        }
        assert!(last.scroll.0 > 0.5 && last.scroll.1 == 0.0, "{last:?}");
        assert_eq!((last.wheel, last.left, last.right), (0.0, Edge::default(), Edge::default()));
    }

    #[test]
    fn two_fingers_sliding_off_the_map_turn_the_wheel() {
        let mut m = TouchMapper::default();
        let at = (100.0, 100.0);
        step(&mut m, &[touch(1, 300.0, 500.0, Phase::Started), touch(2, 400.0, 500.0, Phase::Started)], at, false);
        let mut wheel = 0.0;
        for i in 0..3 {
            // Both fingers move up a step's worth (40 px) a frame: the content scrolls down.
            let y = 500.0 - 40.0 * (i + 1) as f32;
            let f = step(&mut m, &[touch(1, 300.0, y, Phase::Moved), touch(2, 400.0, y, Phase::Moved)], at, false);
            assert_eq!(f.scroll, (0.0, 0.0));
            wheel += f.wheel;
        }
        assert_eq!(wheel, -3.0);
        // Down again turns it back up.
        let f = step(&mut m, &[touch(1, 300.0, 420.0, Phase::Moved), touch(2, 400.0, 420.0, Phase::Moved)], at, false);
        assert_eq!(f.wheel, 1.0);
    }

    #[test]
    fn a_third_finger_lets_a_held_button_go_and_is_ignored() {
        let mut m = TouchMapper::default();
        let at = (100.0, 100.0);
        step(&mut m, &[touch(1, 100.0, 100.0, Phase::Started), touch(2, 300.0, 100.0, Phase::Started)], at, false);
        for _ in 0..20 {
            step(&mut m, &[touch(1, 100.0, 100.0, Phase::Stationary), touch(2, 300.0, 100.0, Phase::Stationary)], at, false);
        }
        let third = step(&mut m, &[touch(1, 100.0, 100.0, Phase::Stationary), touch(2, 300.0, 100.0, Phase::Stationary), touch(3, 500.0, 100.0, Phase::Started)], at, false);
        assert!(third.right.released && !held(&third));
        let slide = step(&mut m, &[touch(1, 150.0, 100.0, Phase::Moved), touch(2, 300.0, 100.0, Phase::Stationary), touch(3, 500.0, 100.0, Phase::Stationary)], at, false);
        assert_eq!((slide.pointer, slide.wheel, slide.scroll), (None, 0.0, (0.0, 0.0)));
        for id in 1..=3 {
            step(&mut m, &[touch(id, 150.0, 100.0, Phase::Ended)], at, false);
        }
        assert_eq!(step(&mut m, &[], at, false), Frame::default());
    }

    #[test]
    fn lifting_everything_releases_what_is_held() {
        let mut m = TouchMapper::default();
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Started)], (0.0, 0.0), false);
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Ended)], (0.0, 0.0), false);
        step(&mut m, &[], (0.0, 0.0), false);
        step(&mut m, &[touch(2, 50.0, 50.0, Phase::Started)], (0.0, 0.0), false);
        assert!(step(&mut m, &[touch(2, 50.0, 50.0, Phase::Stationary)], (0.0, 0.0), false).left.down);
        let lift = step(&mut m, &[touch(2, 50.0, 50.0, Phase::Cancelled)], (0.0, 0.0), false);
        assert_eq!(lift.left, Edge { pressed: false, down: false, released: true });
        assert_eq!(step(&mut m, &[], (0.0, 0.0), false), Frame::default());
    }

    #[test]
    fn the_touchpad_is_off_unless_the_player_turns_it_on_on_mobile() {
        set_setting(None);
        assert!(!enabled());
        set_setting(Some(false));
        assert!(!enabled());
        set_setting(Some(true));
        assert_eq!(enabled(), MOBILE);
        set_setting(None);
    }
}
