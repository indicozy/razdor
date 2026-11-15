//! A debug aid, off by default: the desktop's mouse as one phone finger, to reproduce a phone's
//! touch bugs off-screen and at the desk (`RAZDOR_FAKE_TOUCH=gestures|touchpad`, see the
//! offscreen-snapshot skill). The switch runs the chosen touch mode as on a phone (gestures or
//! the touchpad), and the real mouse's left button becomes ONE finger: a press is a touch that
//! starts at the mouse, a held button a touch that moves or rests with it, a release its lift.
//! The touches go through the same `ui::input::touch_points` path as a phone's, so the mappers
//! (`ui::gestures`, `ui::touchpad`) and everything above them run unchanged; the mouse's own
//! buttons are switched off (`ui::input`), as on a phone, so only the mappers' clicks count.
//! Never set in a game: without the variable the mode is `None` and nothing here runs. A phone
//! ignores the variable (its touches are real).

use std::cell::RefCell;
use std::sync::OnceLock;

use razdor::local;

use super::touchpad::{Phase, TouchPoint};

/// The touch mode the switch runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Gestures,
    Touchpad,
}

/// The mode `RAZDOR_FAKE_TOUCH` asks for, read once. `None` when it is unset, names no mode, or
/// the build runs on a phone.
pub fn mode() -> Option<Mode> {
    static MODE: OnceLock<Option<Mode>> = OnceLock::new();
    *MODE.get_or_init(|| {
        if razdor::platform::MOBILE {
            return None;
        }
        match std::env::var("RAZDOR_FAKE_TOUCH").ok()?.trim() {
            "gestures" => Some(Mode::Gestures),
            "touchpad" => Some(Mode::Touchpad),
            _ => None,
        }
    })
}

/// The mouse's button as one finger: its touch id (new for each press), where it was at the
/// last frame, and whether it is down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Finger {
    id: u64,
    at: (f32, f32),
    down: bool,
}

impl Finger {
    /// This frame's touch for the mouse: `held` the button is down, `pressed` and `released` its
    /// edges this frame, `at` the pointer. A press and a release in one frame arrive as one
    /// `Ended`, as macroquad reports a touch that came and went within a frame.
    pub fn step(&mut self, held: bool, pressed: bool, released: bool, at: (f32, f32)) -> Option<TouchPoint> {
        if !self.down {
            if !(held || pressed) {
                return None;
            }
            self.id += 1;
            self.at = at;
            self.down = !released;
            let phase = if released { Phase::Ended } else { Phase::Started };
            return Some(self.touch(phase, at));
        }
        if released || !held {
            // The finger lifts where the pointer is now.
            self.down = false;
            self.at = at;
            return Some(self.touch(Phase::Ended, at));
        }
        let phase = if at == self.at { Phase::Stationary } else { Phase::Moved };
        self.at = at;
        Some(self.touch(phase, at))
    }

    fn touch(&self, phase: Phase, at: (f32, f32)) -> TouchPoint {
        TouchPoint { id: self.id, x: at.0, y: at.1, phase }
    }
}

local! {
    static FINGER: RefCell<Finger> = RefCell::new(Finger::default());
}

/// This frame's touches from the mouse (`ui::input::touch_points` under the switch).
pub fn frame(held: bool, pressed: bool, released: bool, at: (f32, f32)) -> Vec<TouchPoint> {
    FINGER.with(|f| f.borrow_mut().step(held, pressed, released, at).into_iter().collect())
}

/// Where the finger is: its last place, the pointer while no finger is down ((0, 0) before the
/// first touch). Under the switch the mouse's own place is not the pointer, as on a phone.
pub fn at() -> (f32, f32) {
    FINGER.with(|f| f.borrow().at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase(t: Option<TouchPoint>) -> Option<Phase> {
        t.map(|t| t.phase)
    }

    /// A press is a Started at the mouse, a held still button Stationary, a moved one Moved, a
    /// release Ended at the release place: a tap as a phone sends it.
    #[test]
    fn a_press_hold_and_release_are_a_finger_down_and_up() {
        let mut f = Finger::default();
        assert_eq!(phase(f.step(false, false, false, (10.0, 10.0))), None);
        let down = f.step(true, true, false, (100.0, 200.0)).unwrap();
        assert_eq!((down.phase, down.x, down.y), (Phase::Started, 100.0, 200.0));
        assert_eq!(phase(f.step(true, false, false, (100.0, 200.0))), Some(Phase::Stationary));
        assert_eq!(phase(f.step(true, false, false, (110.0, 200.0))), Some(Phase::Moved));
        let up = f.step(false, false, true, (110.0, 200.0)).unwrap();
        assert_eq!((up.phase, up.x, up.y), (Phase::Ended, 110.0, 200.0));
        assert_eq!(phase(f.step(false, false, false, (110.0, 200.0))), None);
    }

    /// Each press is a new touch: its id is kept until its lift and differs from the last press's.
    #[test]
    fn each_press_is_a_new_touch_with_its_own_id() {
        let mut f = Finger::default();
        let a = f.step(true, true, false, (1.0, 1.0)).unwrap().id;
        assert_eq!(f.step(true, false, false, (2.0, 1.0)).unwrap().id, a);
        f.step(false, false, true, (2.0, 1.0));
        let b = f.step(true, true, false, (3.0, 3.0)).unwrap().id;
        assert_ne!(a, b);
    }

    /// A press and a release in one frame arrive as one Ended, as a phone's quick tap does, and
    /// a button released with no press seen (held or not) is no touch.
    #[test]
    fn a_tap_inside_one_frame_is_one_ended() {
        let mut f = Finger::default();
        assert_eq!(phase(f.step(false, true, true, (5.0, 5.0))), Some(Phase::Ended));
        assert_eq!(phase(f.step(false, false, false, (5.0, 5.0))), None);
    }

    /// A button held while the switch starts (no press edge seen) still begins a touch, and a
    /// frame that finds the button up with no touch down says nothing.
    #[test]
    fn a_button_already_held_begins_a_touch() {
        let mut f = Finger::default();
        assert_eq!(phase(f.step(true, false, false, (7.0, 7.0))), Some(Phase::Started));
        assert_eq!(phase(f.step(false, false, false, (7.0, 7.0))), Some(Phase::Ended));
    }

    /// The finger's place stays where it was lifted, and the mapper's touch ids keep to one press.
    #[test]
    fn the_last_place_is_where_the_finger_lifted() {
        let mut f = Finger::default();
        f.step(true, true, false, (40.0, 50.0));
        f.step(false, false, true, (41.0, 52.0));
        assert_eq!(f.at, (41.0, 52.0));
    }
}
