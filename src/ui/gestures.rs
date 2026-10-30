//! Gesture mode (a Razdor extra; the original has no touch input): the default touch mode on a
//! phone or tablet, where the finger is the pointer. A tap clicks where the finger lifts; a
//! double tap clicks at the first tap's place (the second tap does not shift the hero's route);
//! a finger dragged holds the left button; a long press holds the right one (army and building
//! information, tooltips, and every hint and item description in gesture mode); two fingers pinch the wheel (the map's zoom) or slide together (the
//! map's scroll, or the wheel of a list). A one-finger swipe that starts on a registered area
//! (`ui::input::swipe_area`: the map pans, a list or text box scrolls) is no click at all: it
//! moves the map or turns the list's wheel; only a finger held still before it moves drags as
//! above. [`GestureMapper`] turns the frame's touches into that mouse, purely; `ui::input` feeds
//! it macroquad's touches and merges its [`Frame`] with the pad's. The touchpad mode
//! (`ui::touchpad`) is the option instead. Distances are pixels of a 1080 px high screen, scaled
//! to the screen's height, so a gesture feels the same on any phone (a swipe's row height is
//! given in screen pixels).

use macroquad::prelude::Rect;

use super::gamepad::Frame;
use super::touchpad::{self, Phase, TouchPoint};

/// Gestures run on phones and tablets (`touchpad::MOBILE`), unless the touchpad mode is on.
pub fn enabled() -> bool {
    touchpad::MOBILE && !touchpad::enabled()
}

/// A tap is down and up within this many seconds and stays within [`TAP_SLOP`] pixels. Shorter
/// than [`LONG_TIME`]: a press between the two is nothing.
const TAP_TIME: f32 = 0.25;
const TAP_SLOP: f32 = 12.0;
/// A finger still within the slop for this long holds the right button (a long press): the one
/// hint delay, [`crate::ui::input::HINT_DELAY_SECS`] (0.5 s until 2026-10-30).
const LONG_TIME: f32 = super::input::HINT_DELAY_SECS;
/// A tap within this many seconds of the last one's click, within [`DOUBLE_SLOP`] pixels of it,
/// is a double tap: it clicks at the last click's place.
const DOUBLE_TIME: f32 = 0.35;
const DOUBLE_SLOP: f32 = 24.0;
/// Two fingers pinching turn the wheel a step per this many pixels of distance change.
const PINCH_STEP: f32 = 60.0;
/// Two fingers sliding turn the wheel a step per this many pixels.
const WHEEL_STEP: f32 = 40.0;
/// A one-finger slide past the slop within this many seconds of the touch is a swipe over its
/// area (shorter than [`LONG_TIME`]); a finger held still longer first is a held drag as before.
const SWIPE_TIME: f32 = 0.25;

/// What a one-finger swipe does over a registered rectangle (`ui::input::swipe_area`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SwipeKind {
    /// The map: the finger's slide pans it (`Frame::pan`), 1:1 with the finger.
    Pan,
    /// A list or text box whose rows are `row_px` screen pixels high: the slide turns the
    /// wheel a step per row, finger up = content up (natural scrolling).
    ScrollRows { row_px: f32 },
    /// Not a swipe (the minimap, drawn over the map): a drag there stays the drag it is.
    Off,
}

/// A registered rectangle and its swipe. Later ones are drawn on top: the last over a point
/// is the one that acts.
pub type SwipeArea = (Rect, SwipeKind);

/// The swipe of the last area registered over `at`, if any.
fn swipe_at(areas: &[SwipeArea], at: (f32, f32)) -> Option<SwipeKind> {
    areas.iter().rev().find(|(r, _)| at.0 >= r.x && at.0 < r.x + r.w && at.1 >= r.y && at.1 < r.y + r.h).map(|(_, k)| *k)
}

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
    /// The swipe of the area it touched down on (`Off` and no area: none).
    area: Option<SwipeKind>,
    /// Its first move past the slop has been judged (`decided`): a swipe over `area` or not.
    decided: bool,
    swiping: bool,
}

impl Finger {
    fn new(id: u64, pos: (f32, f32), anchor: (f32, f32), now: f32) -> Self {
        Finger { id, last: pos, down: pos, anchor, since: now, moved: false, delta: (0.0, 0.0), area: None, decided: false, swiping: false }
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
    /// The swipe's rows not yet a whole wheel step.
    rows: f32,
}

impl GestureMapper {
    /// The frame for this frame's `touches` after `dt` seconds on a `screen` (w, h) window.
    /// `map_scrolls`: two fingers sliding scroll the map (as the stick does), else they turn the
    /// wheel as in a list. A pinch always turns the wheel. `areas`: the rectangles a one-finger
    /// swipe acts on, as registered in the last frame.
    pub fn step(&mut self, touches: &[TouchPoint], dt: f32, screen: (f32, f32), map_scrolls: bool, areas: &[SwipeArea]) -> Frame {
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
                // macroquad keeps one entry per touch with its latest phase, so a finger that
                // came down and up within one frame arrives only as `Ended`: it is a tap, as if
                // it had been seen going down first. A cancelled one is no tap.
                (None, true) if matches!(t.phase, Phase::Ended) => {
                    self.down(t.id, pos, &mut f, screen, areas);
                    if let Some(i) = self.fingers.iter().position(|g| g.id == t.id) {
                        let mut g = self.fingers.remove(i);
                        g.slide(pos, screen);
                        self.lift(g, &mut f, screen);
                    }
                }
                (None, true) => {}
                (None, false) => self.down(t.id, pos, &mut f, screen, areas),
            }
        }

        match self.fingers.len() {
            1 if !self.multi => {
                let mut g = self.fingers[0];
                let mut movement = g.delta;
                if g.moved && !g.decided {
                    // The first move past the slop judges it: a swipe over its area if it came
                    // before the hold time, else the held drag or long press as before.
                    g.decided = true;
                    g.swiping = self.now - g.since < SWIPE_TIME && matches!(g.area, Some(SwipeKind::Pan | SwipeKind::ScrollRows { .. }));
                    // The slide from the touch counts too: the map follows the finger from its place.
                    movement = (g.last.0 - g.down.0, g.last.1 - g.down.1);
                }
                self.fingers[0] = g;
                if g.swiping {
                    // No click: the pointer stays where the swipe began, over the area it acts on.
                    f.pointer = Some(g.anchor);
                    match g.area {
                        Some(SwipeKind::Pan) => f.pan = (f.pan.0 + movement.0, f.pan.1 + movement.1),
                        Some(SwipeKind::ScrollRows { row_px }) => {
                            self.rows += movement.1;
                            let steps = (self.rows / row_px).trunc();
                            self.rows -= steps * row_px;
                            f.wheel += steps;
                        }
                        _ => {}
                    }
                } else if g.moved && !self.left_held && !self.right_held {
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

    /// Whether a long press holds the right button this frame (the hints show: `ui::input::hint_hover`).
    pub fn long_pressed(&self) -> bool {
        self.right_held
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
    fn down(&mut self, id: u64, pos: (f32, f32), f: &mut Frame, screen: (f32, f32), areas: &[SwipeArea]) {
        let now = self.now;
        if self.fingers.is_empty() {
            self.multi = false;
            self.pair = None;
            self.pinch = 0.0;
            self.wheel = 0.0;
            self.rows = 0.0;
            let anchor = match self.last_tap.take() {
                Some((clicked, at)) if now - clicked <= DOUBLE_TIME && dist(at, pos) <= px(screen, DOUBLE_SLOP) => at,
                _ => pos,
            };
            let mut g = Finger::new(id, pos, anchor, now);
            g.area = swipe_at(areas, pos);
            self.fingers.push(g);
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
        if !self.fingers.is_empty() || self.multi || g.swiping {
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
        m.step(touches, DT, SCREEN, map_scrolls, &[])
    }

    /// A frame with `areas` registered over the touches.
    fn step_in(m: &mut GestureMapper, touches: &[TouchPoint], areas: &[SwipeArea]) -> Frame {
        m.step(touches, DT, SCREEN, false, areas)
    }

    const PAN: SwipeKind = SwipeKind::Pan;
    const ROWS: SwipeKind = SwipeKind::ScrollRows { row_px: 40.0 };

    /// The whole screen, as one area of kind `kind`.
    fn all(kind: SwipeKind) -> Vec<SwipeArea> {
        vec![(Rect::new(0.0, 0.0, 720.0, 1080.0), kind)]
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
    fn a_tap_seen_only_as_ended_in_one_frame_is_the_same_tap() {
        let mut slow = GestureMapper::default();
        step(&mut slow, &[touch(1, 50.0, 60.0, Phase::Started)], false);
        step(&mut slow, &[touch(1, 50.0, 60.0, Phase::Stationary)], false);
        let lift = step(&mut slow, &[touch(1, 50.0, 60.0, Phase::Ended)], false);
        let after = step(&mut slow, &[], false);
        let mut quick = GestureMapper::default();
        assert_eq!(step(&mut quick, &[touch(1, 50.0, 60.0, Phase::Ended)], false), lift);
        assert_eq!(step(&mut quick, &[], false), after);
        assert_eq!(step(&mut quick, &[], false), Frame::default());
    }

    #[test]
    fn a_finger_cancelled_in_one_frame_is_no_tap() {
        let mut m = GestureMapper::default();
        assert_eq!(step(&mut m, &[touch(1, 50.0, 60.0, Phase::Cancelled)], false), Frame::default());
        assert_eq!(step(&mut m, &[], false), Frame::default());
    }

    #[test]
    fn a_press_too_long_for_a_tap_and_too_short_for_a_long_press_is_nothing() {
        // 18 frames (0.3 s) down: past TAP_TIME (0.25 s), short of LONG_TIME (0.35 s).
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 50.0, 50.0, Phase::Started)], false);
        for _ in 0..17 {
            step(&mut m, &[touch(1, 50.0, 50.0, Phase::Stationary)], false);
        }
        let f = step(&mut m, &[touch(1, 50.0, 50.0, Phase::Ended)], false);
        assert_eq!((f.left, f.right), (Edge::default(), Edge::default()));
    }

    #[test]
    fn a_finger_held_still_for_0_35_seconds_holds_the_right_button() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 50.0, 60.0, Phase::Started)], false);
        let mut pressed = None;
        for i in 1..=30 {
            if step(&mut m, &[touch(1, 50.0, 60.0, Phase::Stationary)], false).right.pressed {
                pressed = Some(i);
                break;
            }
        }
        // Frame 21 is 0.35 s after the touch (the float clock may land a frame later).
        assert!(pressed.is_some_and(|i| (20..=22).contains(&i)), "{pressed:?}");
        assert!(m.long_pressed());
        assert!(step(&mut m, &[touch(1, 50.0, 60.0, Phase::Ended)], false).right.released);
        assert!(!m.long_pressed());
    }

    #[test]
    fn a_touch_of_0_15_seconds_is_a_tap_and_no_long_press() {
        let mut m = GestureMapper::default();
        step(&mut m, &[touch(1, 50.0, 60.0, Phase::Started)], false);
        for _ in 0..8 {
            step(&mut m, &[touch(1, 50.0, 60.0, Phase::Stationary)], false);
        }
        // Lifted 9 frames (0.15 s) after the touch: a tap, no right button.
        let lift = step(&mut m, &[touch(1, 50.0, 60.0, Phase::Ended)], false);
        assert_eq!(lift.left, Edge { pressed: true, down: true, released: false });
        assert_eq!((lift.right, m.long_pressed()), (Edge::default(), false));
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
        assert!(pressed.is_some_and(|i| (19..24).contains(&i)), "{pressed:?}");
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

    #[test]
    fn a_swipe_over_the_map_pans_by_the_finger_and_clicks_nothing() {
        let mut m = GestureMapper::default();
        let areas = all(PAN);
        let down = step_in(&mut m, &[touch(1, 300.0, 500.0, Phase::Started)], &areas);
        assert_eq!(down.pointer, Some((300.0, 500.0)));
        // Past the slop in the first frame: the whole slide from the touch counts, no press.
        let first = step_in(&mut m, &[touch(1, 300.0, 530.0, Phase::Moved)], &areas);
        assert_eq!(first.pan, (0.0, 30.0));
        assert_eq!((first.left, first.right), (Edge::default(), Edge::default()));
        assert_eq!(first.pointer, Some((300.0, 500.0)));
        let slide = step_in(&mut m, &[touch(1, 290.0, 540.0, Phase::Moved)], &areas);
        assert_eq!(slide.pan, (-10.0, 10.0));
        assert_eq!(slide.left, Edge::default());
        let lift = step_in(&mut m, &[touch(1, 290.0, 540.0, Phase::Ended)], &areas);
        assert_eq!((lift.left, lift.right, lift.pan), (Edge::default(), Edge::default(), (0.0, 0.0)));
        assert_eq!(step_in(&mut m, &[], &areas), Frame::default());
    }

    #[test]
    fn a_swipe_over_a_list_turns_the_wheel_a_row_a_step_finger_up_is_down_the_list() {
        let mut m = GestureMapper::default();
        let areas = all(ROWS);
        step_in(&mut m, &[touch(1, 300.0, 500.0, Phase::Started)], &areas);
        let mut wheel = 0.0;
        for i in 1..=4 {
            // Up 20 px a frame: two rows in all, the list's content moves up (the wheel down).
            let f = step_in(&mut m, &[touch(1, 300.0, 500.0 - 20.0 * i as f32, Phase::Moved)], &areas);
            assert_eq!((f.left, f.right, f.pan), (Edge::default(), Edge::default(), (0.0, 0.0)));
            wheel += f.wheel;
        }
        assert_eq!(wheel, -2.0);
        // Down 40 px: one row back.
        let f = step_in(&mut m, &[touch(1, 300.0, 460.0, Phase::Moved)], &areas);
        assert_eq!(f.wheel, 1.0);
        assert_eq!(f.left, Edge::default());
    }

    #[test]
    fn a_finger_held_still_before_it_drags_inside_an_area_is_the_old_left_drag() {
        let mut m = GestureMapper::default();
        let areas = all(PAN);
        step_in(&mut m, &[touch(1, 100.0, 500.0, Phase::Started)], &areas);
        // Held past the swipe time (0.25 s = 15 frames) and short of the long press (0.35 s): then a drag.
        for _ in 0..16 {
            step_in(&mut m, &[touch(1, 100.0, 500.0, Phase::Stationary)], &areas);
        }
        let start = step_in(&mut m, &[touch(1, 100.0, 520.0, Phase::Moved)], &areas);
        assert_eq!(start.left, Edge { pressed: true, down: true, released: false });
        assert_eq!((start.pan, start.pointer), ((0.0, 0.0), Some((100.0, 500.0))));
        let lift = step_in(&mut m, &[touch(1, 100.0, 600.0, Phase::Ended)], &areas);
        assert_eq!(lift.left, Edge { pressed: false, down: false, released: true });
    }

    #[test]
    fn a_drag_outside_every_area_is_the_old_left_drag() {
        let mut m = GestureMapper::default();
        let areas = vec![(Rect::new(0.0, 0.0, 100.0, 100.0), PAN)];
        step_in(&mut m, &[touch(1, 300.0, 500.0, Phase::Started)], &areas);
        let start = step_in(&mut m, &[touch(1, 300.0, 520.0, Phase::Moved)], &areas);
        assert_eq!((start.left.pressed, start.pan), (true, (0.0, 0.0)));
    }

    #[test]
    fn an_off_area_keeps_the_drag_a_drag() {
        let mut m = GestureMapper::default();
        let areas = vec![(Rect::new(0.0, 0.0, 720.0, 1080.0), PAN), (Rect::new(0.0, 400.0, 200.0, 200.0), SwipeKind::Off)];
        step_in(&mut m, &[touch(1, 100.0, 500.0, Phase::Started)], &areas);
        let start = step_in(&mut m, &[touch(1, 100.0, 520.0, Phase::Moved)], &areas);
        assert_eq!((start.left.pressed, start.pan), (true, (0.0, 0.0)));
    }

    #[test]
    fn a_quick_drag_from_a_drag_source_on_a_window_pan_is_a_drag_and_elsewhere_it_pans() {
        // A window that pans (the whole screen) with a unit card's cell over it as an Off area
        // (`pan::drag_source`): a quick slide from the card drags the unit; one elsewhere pans.
        let mut m = GestureMapper::default();
        let areas = vec![(Rect::new(0.0, 0.0, 720.0, 1080.0), PAN), (Rect::new(100.0, 400.0, 200.0, 200.0), SwipeKind::Off)];
        step_in(&mut m, &[touch(1, 150.0, 450.0, Phase::Started)], &areas);
        let start = step_in(&mut m, &[touch(1, 150.0, 470.0, Phase::Moved)], &areas);
        assert_eq!((start.left.pressed, start.pan), (true, (0.0, 0.0)), "a quick drag from the card is the left drag");
        step_in(&mut m, &[touch(1, 150.0, 470.0, Phase::Ended)], &areas);
        // The same quick slide on the window, off the card: a pan, no drag.
        step_in(&mut m, &[touch(2, 600.0, 900.0, Phase::Started)], &areas);
        let pan = step_in(&mut m, &[touch(2, 600.0, 920.0, Phase::Moved)], &areas);
        assert_eq!(pan.pan, (0.0, 20.0), "the window pans by the finger");
        assert!(!pan.left.pressed, "and no drag starts");
    }

    #[test]
    fn the_last_area_registered_over_a_touch_acts() {
        let mut m = GestureMapper::default();
        // A list drawn over the map: its rows, not the map, take the slide.
        let areas = vec![(Rect::new(0.0, 0.0, 720.0, 1080.0), PAN), (Rect::new(0.0, 300.0, 720.0, 300.0), ROWS)];
        step_in(&mut m, &[touch(1, 100.0, 400.0, Phase::Started)], &areas);
        let f = step_in(&mut m, &[touch(1, 100.0, 380.0, Phase::Moved)], &areas);
        assert_eq!((f.pan, f.wheel), ((0.0, 0.0), 0.0));
        let g = step_in(&mut m, &[touch(1, 100.0, 360.0, Phase::Moved)], &areas);
        assert_eq!((g.pan, g.wheel), ((0.0, 0.0), -1.0));
    }

    #[test]
    fn a_tap_inside_an_area_is_still_a_tap() {
        let mut m = GestureMapper::default();
        let areas = all(PAN);
        step_in(&mut m, &[touch(1, 50.0, 60.0, Phase::Started)], &areas);
        let lift = step_in(&mut m, &[touch(1, 50.0, 60.0, Phase::Ended)], &areas);
        assert_eq!(lift.left, Edge { pressed: true, down: true, released: false });
        assert_eq!(lift.pointer, Some((50.0, 60.0)));
        assert_eq!(step_in(&mut m, &[], &areas).left, Edge { pressed: false, down: false, released: true });
    }

    #[test]
    fn a_double_tap_inside_an_area_is_still_a_double_tap() {
        let mut m = GestureMapper::default();
        let areas = all(ROWS);
        step_in(&mut m, &[touch(1, 50.0, 60.0, Phase::Started)], &areas);
        step_in(&mut m, &[touch(1, 50.0, 60.0, Phase::Ended)], &areas);
        step_in(&mut m, &[], &areas);
        let down = step_in(&mut m, &[touch(2, 55.0, 62.0, Phase::Started)], &areas);
        assert_eq!(down.pointer, Some((50.0, 60.0)));
        let lift = step_in(&mut m, &[touch(2, 55.0, 62.0, Phase::Ended)], &areas);
        assert_eq!((lift.left.pressed, lift.pointer), (true, Some((50.0, 60.0))));
    }

    #[test]
    fn a_long_press_inside_an_area_is_still_the_right_button() {
        let mut m = GestureMapper::default();
        let areas = all(PAN);
        step_in(&mut m, &[touch(1, 50.0, 60.0, Phase::Started)], &areas);
        let mut pressed = false;
        for _ in 0..40 {
            let f = step_in(&mut m, &[touch(1, 50.0, 60.0, Phase::Stationary)], &areas);
            if f.right.pressed {
                assert_eq!((f.right.down, f.left), (true, Edge::default()));
                pressed = true;
                break;
            }
        }
        assert!(pressed);
        // The slide off then keeps the right button; no pan.
        let slide = step_in(&mut m, &[touch(1, 50.0, 400.0, Phase::Moved)], &areas);
        assert_eq!((slide.pan, slide.pointer, slide.right.down, slide.left), ((0.0, 0.0), Some((50.0, 60.0)), true, Edge::default()));
        let lift = step_in(&mut m, &[touch(1, 50.0, 400.0, Phase::Ended)], &areas);
        assert_eq!(lift.right, Edge { pressed: false, down: false, released: true });
    }
}
