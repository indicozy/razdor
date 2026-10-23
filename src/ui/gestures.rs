//! Gesture mode (a Razdor extra; the original has no touch input): the default touch mode on a
//! phone or tablet, where the finger is the pointer. A tap clicks where the finger lifts; a
//! double tap clicks at the first tap's place (the second tap does not shift the hero's route);
//! a finger dragged holds the left button; a long press holds the right one (army and building
//! information, tooltips); two fingers pinch the wheel (the map's zoom) or slide together (the
//! map's scroll, or the wheel of a list). [`GestureMapper`] turns the frame's touches into that
//! mouse, purely; `ui::input` feeds it macroquad's touches and merges its [`Frame`] with the
//! pad's. The touchpad mode (`ui::touchpad`) is the option instead. Distances are pixels of a
//! 1080 px high screen, scaled to the screen's height, so a gesture feels the same on any phone.

use super::gamepad::Frame;
use super::touchpad::{self, Phase, TouchPoint};

/// Gestures run on phones and tablets (`touchpad::MOBILE`), unless the touchpad mode is on.
pub fn enabled() -> bool {
    touchpad::MOBILE && !touchpad::enabled()
}

/// A tap is down and up within this many seconds and stays within [`TAP_SLOP`] pixels.
const TAP_TIME: f32 = 0.25;
const TAP_SLOP: f32 = 12.0;
/// A finger still within the slop for this long holds the right button (a long press).
const LONG_TIME: f32 = 0.5;
/// A tap within this many seconds of the last one's click, within [`DOUBLE_SLOP`] pixels of it,
/// is a double tap: it clicks at the last click's place.
const DOUBLE_TIME: f32 = 0.35;
const DOUBLE_SLOP: f32 = 24.0;
/// Two fingers pinching turn the wheel a step per this many pixels of distance change.
const PINCH_STEP: f32 = 60.0;
/// Two fingers sliding turn the wheel a step per this many pixels.
const WHEEL_STEP: f32 = 40.0;

/// Pixels of a 1080 px high screen, scaled to this one.
fn px(screen: (f32, f32), v: f32) -> f32 {
    v * screen.1 / 1080.0
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// A finger from its touch to its lift.
#[derive(Clone, Copy, Debug)]
struct Finger {
    id: u64,
    /// Where it is now, and where it touched down.
    last: (f32, f32),
    down: (f32, f32),
    /// Where the pointer is when the finger is at `down`: the first tap's click for the second
    /// tap of a double tap, else `down`. The pointer moves with the finger from there.
    anchor: (f32, f32),
    since: f32,
    /// It has been more than the slop away from `down` (sticky): no tap, and a drag or nothing.
    moved: bool,
    /// Its slide this frame.
    delta: (f32, f32),
}

impl Finger {
    fn new(id: u64, pos: (f32, f32), anchor: (f32, f32), now: f32) -> Self {
        Finger { id, last: pos, down: pos, anchor, since: now, moved: false, delta: (0.0, 0.0) }
    }

    /// Where the pointer is for this finger, kept on the screen.
    fn at(&self, screen: (f32, f32)) -> (f32, f32) {
        let p = (self.anchor.0 + self.last.0 - self.down.0, self.anchor.1 + self.last.1 - self.down.1);
        (p.0.clamp(0.0, (screen.0 - 1.0).max(0.0)), p.1.clamp(0.0, (screen.1 - 1.0).max(0.0)))
    }

    /// The finger is at `pos` this frame.
    fn slide(&mut self, pos: (f32, f32), screen: (f32, f32)) {
        self.delta = (self.delta.0 + pos.0 - self.last.0, self.delta.1 + pos.1 - self.last.1);
        self.last = pos;
        if dist(self.last, self.down) > px(screen, TAP_SLOP) {
            self.moved = true;
        }
    }
}

/// Turns the touches into the gesture mouse, frame by frame.
#[derive(Clone, Debug, Default)]
pub struct GestureMapper {
    /// The mapper's clock: the frames' dt added up (a long frame counts as 0.1 s).
    now: f32,
    /// The fingers down, in the order they touched. The first two make the gestures; a third
    /// and the rest are ignored until they lift.
    fingers: Vec<Finger>,
    /// Two fingers came in this session: no tap, drag or long press until all are up.
    multi: bool,
    /// The last single-finger tap: when it clicked, and where.
    last_tap: Option<(f32, (f32, f32))>,
    /// The left button is held by a drag, the right one by a long press, until the finger lifts.
    left_held: bool,
    right_held: bool,
    /// Where the pointer was when the long press began: it stays there while the right button
    /// is held (the finger may slide off to uncover the view; the finger covers the army).
    press_at: (f32, f32),
    /// A click made by the last frame's lift: let go this frame.
    release_left: bool,
    release_right: bool,
    /// The two fingers' ids and the distance between them last frame.
    pair: Option<(u64, u64, f32)>,
    /// The pinch's distance change not yet a whole wheel step, and the same for the slide.
    pinch: f32,
    wheel: f32,
}

impl GestureMapper {
    /// The frame for this frame's `touches` after `dt` seconds on a `screen` (w, h) window.
    /// `map_scrolls`: two fingers sliding scroll the map (as the stick does), else they turn the
    /// wheel as in a list. A pinch always turns the wheel.
    pub fn step(&mut self, touches: &[TouchPoint], dt: f32, screen: (f32, f32), map_scrolls: bool) -> Frame {
        let dt = dt.clamp(0.0, 0.1);
        self.now += dt;
        let mut f = Frame::default();
        if std::mem::take(&mut self.release_left) {
            f.left.released = true;
        }
        if std::mem::take(&mut self.release_right) {
            f.right.released = true;
        }
        for g in &mut self.fingers {
            g.delta = (0.0, 0.0);
        }

        for t in touches {
            let pos = (t.x, t.y);
            let ending = matches!(t.phase, Phase::Ended | Phase::Cancelled);
            match (self.fingers.iter().position(|g| g.id == t.id), ending) {
                (Some(i), true) => {
                    let mut g = self.fingers.remove(i);
                    g.slide(pos, screen);
                    self.lift(g, &mut f, screen);
                }
                (Some(i), false) => self.fingers[i].slide(pos, screen),
                (None, true) => {}
                (None, false) => self.down(t.id, pos, &mut f, screen),
            }
        }

        match self.fingers.len() {
            1 if !self.multi => {
                let g = self.fingers[0];
                if g.moved && !self.left_held && !self.right_held {
                    // A drag: the left button goes down where the finger touched (the first tap's
                    // place for a double tap); the pointer follows the finger from the next frame.
                    self.left_held = true;
                    f.left.pressed = true;
                    f.left.down = true;
                    f.pointer = Some(g.anchor);
                } else {
                    if !g.moved && !self.left_held && !self.right_held && self.now - g.since >= LONG_TIME {
                        self.right_held = true;
                        f.right.pressed = true;
                        f.right.down = true;
                        self.press_at = g.at(screen);
                    }
                    // A long press keeps the pointer where it began (the info window stays put).
                    f.pointer = Some(if self.right_held { self.press_at } else { g.at(screen) });
                }
            }
            n if n >= 2 => {
                // The pointer stays on the first finger while two are down (Razdor's, a player's
                // report): the wheel of a list takes its turn where the fingers are, not where
                // the pointer was left before the second finger came down.
                f.pointer = Some(self.fingers[0].at(screen));
                self.pinch_or_slide(&mut f, dt, screen, map_scrolls);
            }
            _ => {}
        }
        // A held button stays down for every frame it is held.
        f.left.down |= self.left_held;
        f.right.down |= self.right_held;
        f
    }

    /// The first two fingers, frame by frame. The distance between them changing more than their
    /// slide is a pinch: the wheel, a step per [`PINCH_STEP`], fingers apart zoom in. Otherwise
    /// they slide together: the map's scroll (the stick, from the moving fingers' velocity) or
    /// the wheel of a list. Only the fingers that moved this frame count for the slide.
    fn pinch_or_slide(&mut self, f: &mut Frame, dt: f32, screen: (f32, f32), map_scrolls: bool) {
        let (a, b) = (self.fingers[0], self.fingers[1]);
        let d = dist(a.last, b.last);
        if let Some((_, _, prev)) = self.pair.filter(|p| (p.0, p.1) == (a.id, b.id)) {
            let change = d - prev;
            let moving: Vec<(f32, f32)> = [a.delta, b.delta].into_iter().filter(|v| *v != (0.0, 0.0)).collect();
            let n = moving.len().max(1) as f32;
            let mean = (moving.iter().map(|v| v.0).sum::<f32>() / n, moving.iter().map(|v| v.1).sum::<f32>() / n);
            if change.abs() > mean.0.hypot(mean.1) {
                // Fingers apart is the wheel up, which zooms the map in.
                let step = px(screen, PINCH_STEP);
                self.pinch += change;
                let steps = (self.pinch / step).trunc();
                self.pinch -= steps * step;
                f.wheel += steps;
            } else if map_scrolls {
                self.wheel = 0.0;
                if dt > 0.0 && !moving.is_empty() {
                    // The stick: a screen's height a second is full push. Fingers moving up push
                    // the map up, as a page under them.
                    let stick = |v: f32| (-v / (dt * screen.1)).clamp(-1.0, 1.0);
                    f.scroll = (stick(mean.0), stick(mean.1));
                }
            } else {
                // Fingers moving up scroll the content down, so they turn the wheel down.
                let step = px(screen, WHEEL_STEP);
                self.wheel += mean.1;
                let steps = (self.wheel / step).trunc();
                self.wheel -= steps * step;
                f.wheel += steps;
            }
        }
        self.pair = Some((a.id, b.id, d));
    }

    /// A finger came down at `pos`. The first one starts a session; a tap near the last one's
    /// click is a double tap and lands at that click. A second one makes two fingers, which
    /// lets a held button go; the rest are ignored.
    fn down(&mut self, id: u64, pos: (f32, f32), f: &mut Frame, screen: (f32, f32)) {
        let now = self.now;
        if self.fingers.is_empty() {
            self.multi = false;
            self.pair = None;
            self.pinch = 0.0;
            self.wheel = 0.0;
            let anchor = match self.last_tap.take() {
                Some((clicked, at)) if now - clicked <= DOUBLE_TIME && dist(at, pos) <= px(screen, DOUBLE_SLOP) => at,
                _ => pos,
            };
            self.fingers.push(Finger::new(id, pos, anchor, now));
            return;
        }
        if self.fingers.len() == 1 {
            self.multi = true;
            if std::mem::take(&mut self.left_held) {
                f.left.released = true;
            }
            if std::mem::take(&mut self.right_held) {
                f.right.released = true;
            }
        }
        self.fingers.push(Finger::new(id, pos, pos, now));
    }

    /// A finger lifted (`g` is it, already out of `fingers`, at its last place). A single
    /// finger's tap clicks where it lifts; whatever it held lets go, a long press's right button
    /// at the place the press began. Two fingers lifting click nothing, and nothing stays held
    /// once all are up.
    fn lift(&mut self, g: Finger, f: &mut Frame, screen: (f32, f32)) {
        if !self.fingers.is_empty() || self.multi {
            return;
        }
        let at = g.at(screen);
        f.pointer = Some(if self.right_held { self.press_at } else { at });
        if std::mem::take(&mut self.left_held) {
            f.left.released = true;
        }
        if std::mem::take(&mut self.right_held) {
            f.right.released = true;
        }
        if !g.moved && self.now - g.since <= TAP_TIME {
            f.left.pressed = true;
            f.left.down = true;
            self.release_left = true;
            self.last_tap = Some((self.now, at));
        }
    }
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

    fn step(m: &mut GestureMapper, touches: &[TouchPoint], map_scrolls: bool) -> Frame {
        m.step(touches, DT, SCREEN, map_scrolls)
    }

    #[test]
    fn a_tap_clicks_where_the_finger_lifts_over_two_frames() {
        let mut m = GestureMapper::default();
        let down = step(&mut m, &[touch(1, 50.0, 60.0, Phase::Started)], false);
        assert_eq!(down.pointer, Some((50.0, 60.0)));
        assert_eq!((down.left, down.right), (Edge::default(), Edge::default()));
        step(&mut m, &[touch(1, 50.0, 60.0, Phase::Stationary)], false);
        let lift = step(&mut m, &[touch(1, 50.0, 60.0, Phase::Ended)], false);
        assert_eq!(lift.left, Edge { pressed: true, down: true, released: false });
        assert_eq!(lift.pointer, Some((50.0, 60.0)));
        let after = step(&mut m, &[], false);
        assert_eq!(after.left, Edge { pressed: false, down: false, released: true });
        assert_eq!(step(&mut m, &[], false), Frame::default());
    }

    #[test]
    fn a_press_too_long_for_a_tap_and_too_short_for_a_long_press_is_nothing() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Started)], false);
        for _ in 0..20 {
            step(&mut m, &[touch(1, 50.0, 50.0, Phase::Stationary)], false);
        }
        let f = step(&mut m, &[touch(1, 50.0, 50.0, Phase::Ended)], false);
        assert_eq!((f.left, f.right), (Edge::default(), Edge::default()));
    }

    #[test]
    fn a_long_press_holds_the_right_button_at_the_finger_and_no_left() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 50.0, 60.0, Phase::Started)], false);
        let mut pressed = None;
        for i in 0..40 {
            let f = step(&mut m, &[touch(1, 50.0, 60.0, Phase::Stationary)], false);
            if f.right.pressed {
                assert_eq!((f.right.down, f.left), (true, Edge::default()));
                assert_eq!(f.pointer, Some((50.0, 60.0)));
                pressed = Some(i);
                break;
            }
        }
        assert!(pressed.is_some_and(|i| (25..35).contains(&i)), "{pressed:?}");
        let held = step(&mut m, &[touch(1, 50.0, 60.0, Phase::Stationary)], false);
        assert_eq!(held.right, Edge { pressed: false, down: true, released: false });
        let lift = step(&mut m, &[touch(1, 50.0, 60.0, Phase::Ended)], false);
        assert_eq!(lift.right, Edge { pressed: false, down: false, released: true });
        assert_eq!((lift.left, lift.pointer), (Edge::default(), Some((50.0, 60.0))));
        assert_eq!(step(&mut m, &[], false), Frame::default());
    }

    #[test]
    fn a_long_press_keeps_the_pointer_where_it_began_while_the_finger_moves() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 50.0, 60.0, Phase::Started)], false);
        for _ in 0..40 {
            if step(&mut m, &[touch(1, 50.0, 60.0, Phase::Stationary)], false).right.pressed {
                break;
            }
        }
        // The finger slides down off the cell and on: the right button stays down, the pointer
        // stays at the press, and no drag starts.
        let slide = step(&mut m, &[touch(1, 50.0, 300.0, Phase::Moved)], false);
        assert_eq!((slide.pointer, slide.right), (Some((50.0, 60.0)), Edge { pressed: false, down: true, released: false }));
        assert_eq!(slide.left, Edge::default());
        let far = step(&mut m, &[touch(1, 200.0, 500.0, Phase::Moved)], false);
        assert_eq!((far.pointer, far.right.down, far.left), (Some((50.0, 60.0)), true, Edge::default()));
        // The lift releases the right button where the press began, and clicks nothing.
        let lift = step(&mut m, &[touch(1, 200.0, 500.0, Phase::Ended)], false);
        assert_eq!(lift.right, Edge { pressed: false, down: false, released: true });
        assert_eq!((lift.pointer, lift.left), (Some((50.0, 60.0)), Edge::default()));
        assert_eq!(step(&mut m, &[], false), Frame::default());
    }

    #[test]
    fn a_long_press_after_a_small_drift_keeps_the_point_it_began_at() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 100.0, 100.0, Phase::Started)], false);
        // Still inside the slop: the press is taken where the finger is now, 5 px away.
        step(&mut m, &[touch(1, 105.0, 100.0, Phase::Stationary)], false);
        let mut pressed = None;
        for _ in 0..40 {
            let f = step(&mut m, &[touch(1, 105.0, 100.0, Phase::Stationary)], false);
            if f.right.pressed {
                pressed = Some(f.pointer);
                break;
            }
        }
        assert_eq!(pressed, Some(Some((105.0, 100.0))));
        let slide = step(&mut m, &[touch(1, 105.0, 400.0, Phase::Moved)], false);
        assert_eq!(slide.pointer, Some((105.0, 100.0)));
    }

    #[test]
    fn a_double_tap_clicks_at_the_first_tap() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 50.0, 60.0, Phase::Started)], false);
        step(&mut m, &[touch(1, 50.0, 60.0, Phase::Ended)], false);
        step(&mut m, &[], false);
        // The second tap, a few pixels away, lands at the first tap's place.
        let down = step(&mut m, &[touch(2, 55.0, 62.0, Phase::Started)], false);
        assert_eq!((down.pointer, down.left), (Some((50.0, 60.0)), Edge::default()));
        let lift = step(&mut m, &[touch(2, 55.0, 62.0, Phase::Ended)], false);
        assert_eq!(lift.left, Edge { pressed: true, down: true, released: false });
        assert_eq!(lift.pointer, Some((50.0, 60.0)));
        assert_eq!(step(&mut m, &[], false).left, Edge { pressed: false, down: false, released: true });
        assert_eq!(step(&mut m, &[], false), Frame::default());
    }

    #[test]
    fn a_tap_far_from_the_last_one_lands_where_it_is() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Started)], false);
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Ended)], false);
        step(&mut m, &[], false);
        let down = step(&mut m, &[touch(2, 300.0, 300.0, Phase::Started)], false);
        assert_eq!(down.pointer, Some((300.0, 300.0)));
        let lift = step(&mut m, &[touch(2, 300.0, 300.0, Phase::Ended)], false);
        assert_eq!((lift.left.pressed, lift.pointer), (true, Some((300.0, 300.0))));
    }

    #[test]
    fn a_dragged_finger_holds_the_left_button_from_its_start_and_follows() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 100.0, 500.0, Phase::Started)], false);
        // 20 px away: past the slop, so the drag starts where the finger touched down.
        let start = step(&mut m, &[touch(1, 100.0, 520.0, Phase::Moved)], false);
        assert_eq!(start.left, Edge { pressed: true, down: true, released: false });
        assert_eq!(start.pointer, Some((100.0, 500.0)));
        let slide = step(&mut m, &[touch(1, 100.0, 600.0, Phase::Moved)], false);
        assert_eq!(slide.left, Edge { pressed: false, down: true, released: false });
        assert_eq!(slide.pointer, Some((100.0, 600.0)));
        let lift = step(&mut m, &[touch(1, 100.0, 600.0, Phase::Ended)], false);
        assert_eq!(lift.left, Edge { pressed: false, down: false, released: true });
        assert_eq!(step(&mut m, &[], false), Frame::default());
    }

    #[test]
    fn a_second_finger_cancels_a_pending_tap() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Started)], false);
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Stationary), touch(2, 300.0, 300.0, Phase::Started)], false);
        let lift = step(&mut m, &[touch(1, 50.0, 50.0, Phase::Ended), touch(2, 300.0, 300.0, Phase::Stationary)], false);
        assert_eq!((lift.left, lift.right), (Edge::default(), Edge::default()));
        let end = step(&mut m, &[touch(2, 300.0, 300.0, Phase::Ended)], false);
        assert_eq!(end, Frame::default());
    }

    #[test]
    fn a_second_finger_lets_a_held_drag_go() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 100.0, 500.0, Phase::Started)], false);
        assert!(step(&mut m, &[touch(1, 100.0, 520.0, Phase::Moved)], false).left.pressed);
        let f = step(&mut m, &[touch(1, 100.0, 520.0, Phase::Stationary), touch(2, 300.0, 500.0, Phase::Started)], false);
        assert_eq!(f.left, Edge { pressed: false, down: false, released: true });
        let end = step(&mut m, &[touch(1, 100.0, 520.0, Phase::Ended), touch(2, 300.0, 500.0, Phase::Stationary)], false);
        assert_eq!(end.left, Edge::default());
    }

    #[test]
    fn pinching_apart_turns_the_wheel_up_and_together_down() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 300.0, 500.0, Phase::Started), touch(2, 400.0, 500.0, Phase::Started)], true);
        let mut wheel = 0.0;
        for i in 0..3 {
            // Apart: 20 px of distance a frame, a step per 60 px.
            let (a, b) = (300.0 - 10.0 * (i + 1) as f32, 400.0 + 10.0 * (i + 1) as f32);
            let f = step(&mut m, &[touch(1, a, 500.0, Phase::Moved), touch(2, b, 500.0, Phase::Moved)], true);
            assert_eq!(f.scroll, (0.0, 0.0));
            wheel += f.wheel;
        }
        assert_eq!(wheel, 1.0);
        for i in 0..3 {
            let (a, b) = (270.0 + 10.0 * (i + 1) as f32, 430.0 - 10.0 * (i + 1) as f32);
            wheel += step(&mut m, &[touch(1, a, 500.0, Phase::Moved), touch(2, b, 500.0, Phase::Moved)], true).wheel;
        }
        assert_eq!(wheel, 0.0);
        for i in 0..3 {
            let (a, b) = (300.0 + 10.0 * (i + 1) as f32, 400.0 - 10.0 * (i + 1) as f32);
            wheel += step(&mut m, &[touch(1, a, 500.0, Phase::Moved), touch(2, b, 500.0, Phase::Moved)], true).wheel;
        }
        assert_eq!(wheel, -1.0);
    }

    #[test]
    fn two_fingers_sliding_together_scroll_the_map_and_are_no_click() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 300.0, 500.0, Phase::Started), touch(2, 400.0, 500.0, Phase::Started)], true);
        let mut last = Frame::default();
        for i in 0..5 {
            // Both fingers slide left (10 px a frame): the view scrolls right.
            let x = 300.0 - 10.0 * (i + 1) as f32;
            last = step(&mut m, &[touch(1, x, 500.0, Phase::Moved), touch(2, x + 100.0, 500.0, Phase::Moved)], true);
        }
        assert!(last.scroll.0 > 0.5 && last.scroll.1 == 0.0, "{last:?}");
        assert_eq!((last.wheel, last.left, last.right), (0.0, Edge::default(), Edge::default()));
    }

    #[test]
    fn two_fingers_sliding_together_off_the_map_turn_the_wheel() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 300.0, 500.0, Phase::Started), touch(2, 400.0, 500.0, Phase::Started)], false);
        let mut wheel = 0.0;
        for i in 0..3 {
            // Both move up a step's worth (40 px) a frame: the content scrolls down.
            let y = 500.0 - 40.0 * (i + 1) as f32;
            let f = step(&mut m, &[touch(1, 300.0, y, Phase::Moved), touch(2, 400.0, y, Phase::Moved)], false);
            assert_eq!(f.scroll, (0.0, 0.0));
            wheel += f.wheel;
        }
        assert_eq!(wheel, -3.0);
    }

    #[test]
    fn two_fingers_sliding_keep_the_pointer_on_the_first_finger() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 300.0, 500.0, Phase::Started)], false);
        // The second finger lands: the pointer stays where the first one is, not left behind.
        let down = step(&mut m, &[touch(1, 300.0, 500.0, Phase::Stationary), touch(2, 400.0, 500.0, Phase::Started)], false);
        assert_eq!(down.pointer, Some((300.0, 500.0)));
        let mut wheel = 0.0;
        for i in 0..3 {
            let y = 500.0 - 40.0 * (i + 1) as f32;
            let f = step(&mut m, &[touch(1, 300.0, y, Phase::Moved), touch(2, 400.0, y, Phase::Moved)], false);
            assert_eq!(f.pointer, Some((300.0, y)));
            wheel += f.wheel;
        }
        assert_eq!(wheel, -3.0);
    }

    #[test]
    fn one_finger_held_and_the_other_sliding_scrolls_the_map() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 300.0, 500.0, Phase::Started)], true);
        step(&mut m, &[touch(1, 300.0, 500.0, Phase::Stationary), touch(2, 400.0, 500.0, Phase::Started)], true);
        let mut last = Frame::default();
        for i in 0..3 {
            // The held finger stays; the other moves down 10 px a frame: the map scrolls up.
            let y = 500.0 + 10.0 * (i + 1) as f32;
            last = step(&mut m, &[touch(1, 300.0, 500.0, Phase::Stationary), touch(2, 400.0, y, Phase::Moved)], true);
        }
        assert!(last.scroll.1 < -0.5, "{last:?}");
        assert_eq!((last.wheel, last.left, last.right), (0.0, Edge::default(), Edge::default()));
    }

    #[test]
    fn a_third_finger_is_ignored() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 100.0, 100.0, Phase::Started), touch(2, 300.0, 100.0, Phase::Started)], false);
        let third = step(&mut m, &[touch(1, 100.0, 100.0, Phase::Stationary), touch(2, 300.0, 100.0, Phase::Stationary), touch(3, 500.0, 100.0, Phase::Started)], false);
        // Nothing but the pointer, which stays on the first finger.
        assert_eq!(Frame { pointer: None, ..third }, Frame::default());
        let slide = step(&mut m, &[touch(1, 100.0, 100.0, Phase::Stationary), touch(2, 300.0, 100.0, Phase::Stationary), touch(3, 500.0, 300.0, Phase::Moved)], false);
        assert_eq!((slide.wheel, slide.scroll, slide.left, slide.right), (0.0, (0.0, 0.0), Edge::default(), Edge::default()));
        for id in 1..=3 {
            step(&mut m, &[touch(id, 100.0, 100.0, Phase::Ended)], false);
        }
        assert_eq!(step(&mut m, &[], false), Frame::default());
    }

    #[test]
    fn a_cancelled_finger_lets_go_of_what_it_held() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 100.0, 500.0, Phase::Started)], false);
        assert!(step(&mut m, &[touch(1, 100.0, 520.0, Phase::Moved)], false).left.down);
        let lift = step(&mut m, &[touch(1, 100.0, 520.0, Phase::Cancelled)], false);
        assert_eq!(lift.left, Edge { pressed: false, down: false, released: true });
        assert_eq!(step(&mut m, &[], false), Frame::default());
    }
}
