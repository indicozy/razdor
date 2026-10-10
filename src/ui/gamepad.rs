//! Gamepads (a Razdor extra the players asked for; the original has none). The game is played
//! with the mouse, so a gamepad drives a mouse of its own: the left stick or the d-pad moves
//! the pointer, A is the left button and B the right one (held, so drags and the map's
//! right-button tooltip work), Start is Esc, X Enter, Y Space, the shoulders turn the wheel
//! and the right stick scrolls the map (elsewhere it turns the wheel too). [`Mapper`] turns
//! what the pad holds into that mouse, purely; `ui::input` merges it with the real mouse and
//! keyboard. The pads are read with gilrs (feature `gamepad`); without it [`poll`] finds none.

use macroquad::prelude::KeyCode;

/// A gamepad button, by its place on an Xbox pad (A below, B right, X left, Y above; on a
/// PlayStation pad cross, circle, square, triangle).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    South,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    Start,
    Up,
    Down,
    Left,
    Right,
}

impl Button {
    fn bit(self) -> u16 {
        1 << self as u16
    }
}

/// What a pad holds at one moment: its sticks (x right, y down, each -1..1) and buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadState {
    pub left: (f32, f32),
    pub right: (f32, f32),
    buttons: u16,
}

impl PadState {
    pub fn down(&self, b: Button) -> bool {
        self.buttons & b.bit() != 0
    }

    #[cfg_attr(not(feature = "gamepad"), allow(dead_code))]
    pub fn set(&mut self, b: Button, down: bool) {
        if down {
            self.buttons |= b.bit();
        } else {
            self.buttons &= !b.bit();
        }
    }

    #[cfg(test)]
    fn with(mut self, b: Button) -> PadState {
        self.set(b, true);
        self
    }
}

/// A button or key of the pad's mouse this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edge {
    pub pressed: bool,
    pub down: bool,
    pub released: bool,
}

/// The pad's mouse and keys for one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    /// Where the pointer went, when the pad moved it.
    pub pointer: Option<(f32, f32)>,
    pub left: Edge,
    pub right: Edge,
    /// Wheel steps (up is positive), as macroquad's.
    pub wheel: f32,
    /// The keys the buttons stand for ([`KEYS`]), those not up.
    pub keys: Vec<(KeyCode, Edge)>,
    /// The right stick, when the map scrolls with it (x right, y down, -1..1).
    pub scroll: (f32, f32),
}

/// The keys of the buttons: Start is Esc (menus, back), X Enter (OK, Yes), Y Space (the
/// map's stop, the battle's own card).
pub const KEYS: [(Button, KeyCode); 3] = [(Button::Start, KeyCode::Escape), (Button::West, KeyCode::Enter), (Button::North, KeyCode::Space)];

/// A stick nearer its centre than this counts as let go.
const DEADZONE: f32 = 0.2;
/// The pointer's speed (pixels a second on a 768 px high window): it starts slow, for aiming
/// at a cell, and reaches its top speed after [`RAMP`] seconds of moving.
const SLOW: f32 = 260.0;
const FAST: f32 = 1300.0;
const RAMP: f32 = 0.7;
/// Wheel steps a second of the right stick pushed all the way.
const WHEEL_RATE: f32 = 10.0;
/// A held shoulder turns the wheel again after this, then at this pace.
const REPEAT_DELAY: f32 = 0.4;
const REPEAT_EVERY: f32 = 0.1;

/// A stick with its dead centre cut: the length rescaled from the dead zone's edge to 1.
fn deadzone((x, y): (f32, f32)) -> (f32, f32) {
    let len = x.hypot(y);
    if len < DEADZONE {
        return (0.0, 0.0);
    }
    let k = ((len - DEADZONE) / (1.0 - DEADZONE)).min(1.0) / len;
    (x * k, y * k)
}

/// Turns pad states into the pad's mouse, frame by frame.
#[derive(Clone, Debug, Default)]
pub struct Mapper {
    /// Seconds the pointer has been moving.
    moving: f32,
    /// Wheel turned by the right stick, not yet a whole step.
    wheel: f32,
    /// Seconds a shoulder has been held.
    shoulder: f32,
    prev: PadState,
}

impl Mapper {
    /// The frame for `pad` after `dt` seconds, the pointer at `at` on a `screen` (w, h) window.
    /// `map_scrolls`: the right stick scrolls the map, and does not turn the wheel.
    pub fn step(&mut self, pad: &PadState, dt: f32, at: (f32, f32), screen: (f32, f32), map_scrolls: bool) -> Frame {
        // A long frame (loading) does not throw the pointer across the screen.
        let dt = dt.clamp(0.0, 0.1);
        let prev = std::mem::replace(&mut self.prev, *pad);
        let edge = |b: Button| Edge { pressed: pad.down(b) && !prev.down(b), down: pad.down(b), released: !pad.down(b) && prev.down(b) };
        let mut f = Frame { left: edge(Button::South), right: edge(Button::East), ..Default::default() };
        f.keys = KEYS.iter().map(|&(b, k)| (k, edge(b))).filter(|(_, e)| e.down || e.released).collect();

        // The pointer: the d-pad at full speed in its eight directions, else the stick, which
        // moves slowly near its centre (the square of its push).
        let axis = |minus: Button, plus: Button| pad.down(plus) as i8 as f32 - pad.down(minus) as i8 as f32;
        let dpad = (axis(Button::Left, Button::Right), axis(Button::Up, Button::Down));
        let (dir, push) = if dpad != (0.0, 0.0) {
            let len = dpad.0.hypot(dpad.1);
            ((dpad.0 / len, dpad.1 / len), 1.0)
        } else {
            let (x, y) = deadzone(pad.left);
            let len = x.hypot(y);
            if len > 0.0 { ((x / len, y / len), len * len) } else { ((0.0, 0.0), 0.0) }
        };
        if push > 0.0 {
            self.moving += dt;
            let speed = (SLOW + (FAST - SLOW) * (self.moving / RAMP).min(1.0)) * push * screen.1 / 768.0;
            let (w, h) = ((screen.0 - 1.0).max(0.0), (screen.1 - 1.0).max(0.0));
            let to = ((at.0 + dir.0 * speed * dt).clamp(0.0, w), (at.1 + dir.1 * speed * dt).clamp(0.0, h));
            f.pointer = Some(to);
        } else {
            self.moving = 0.0;
        }

        // The right stick: the map's scroll, or the wheel (up turns it up).
        let right = deadzone(pad.right);
        if map_scrolls {
            f.scroll = right;
            self.wheel = 0.0;
        } else if right.1 == 0.0 {
            self.wheel = 0.0;
        } else {
            self.wheel -= right.1 * WHEEL_RATE * dt;
            let steps = self.wheel.trunc();
            self.wheel -= steps;
            f.wheel += steps;
        }

        // The shoulders: one wheel step at the press, then more while held.
        let turn = pad.down(Button::LeftBumper) as i8 as f32 - pad.down(Button::RightBumper) as i8 as f32;
        let pressed = [Button::LeftBumper, Button::RightBumper].iter().any(|&b| pad.down(b) && !prev.down(b));
        if turn == 0.0 {
            self.shoulder = 0.0;
        } else if pressed {
            self.shoulder = 0.0;
            f.wheel += turn;
        } else {
            let before = self.shoulder;
            self.shoulder += dt;
            let repeats = |t: f32| if t < REPEAT_DELAY { 0 } else { ((t - REPEAT_DELAY) / REPEAT_EVERY) as i32 + 1 };
            f.wheel += turn * (repeats(self.shoulder) - repeats(before)) as f32;
        }
        f
    }
}

/// The pad played with now (the last one used, else the first connected), if any.
#[cfg(feature = "gamepad")]
pub fn poll() -> Option<PadState> {
    use std::cell::RefCell;

    use gilrs::{Axis, Button as B, EventType, GamepadId, Gilrs};

    struct Pads {
        gilrs: Option<Gilrs>,
        active: Option<GamepadId>,
    }
    thread_local! {
        static PADS: RefCell<Option<Pads>> = const { RefCell::new(None) };
    }
    PADS.with(|p| {
        let mut p = p.borrow_mut();
        let pads = p.get_or_insert_with(|| Pads {
            gilrs: Gilrs::new().map_err(|e| razdor::diag!("gamepads: {e}")).ok(),
            active: None,
        });
        let gilrs = pads.gilrs.as_mut()?;
        while let Some(ev) = gilrs.next_event() {
            match ev.event {
                EventType::Disconnected if pads.active == Some(ev.id) => pads.active = None,
                EventType::Disconnected => {}
                _ => pads.active = Some(ev.id),
            }
        }
        let id = pads.active.or_else(|| gilrs.gamepads().next().map(|(id, _)| id))?;
        let pad = gilrs.connected_gamepad(id)?;
        let mut s = PadState {
            left: (pad.value(Axis::LeftStickX), -pad.value(Axis::LeftStickY)),
            right: (pad.value(Axis::RightStickX), -pad.value(Axis::RightStickY)),
            buttons: 0,
        };
        for (b, g) in [
            (Button::South, B::South),
            (Button::East, B::East),
            (Button::West, B::West),
            (Button::North, B::North),
            (Button::LeftBumper, B::LeftTrigger),
            (Button::RightBumper, B::RightTrigger),
            (Button::Start, B::Start),
            (Button::Up, B::DPadUp),
            (Button::Down, B::DPadDown),
            (Button::Left, B::DPadLeft),
            (Button::Right, B::DPadRight),
        ] {
            s.set(b, pad.is_pressed(g));
        }
        Some(s)
    })
}

/// Built without gamepads: there is never one.
#[cfg(not(feature = "gamepad"))]
pub fn poll() -> Option<PadState> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (f32, f32) = (1024.0, 768.0);

    fn run(m: &mut Mapper, pad: PadState) -> Frame {
        m.step(&pad, 1.0 / 60.0, (500.0, 400.0), SCREEN, false)
    }

    #[test]
    fn a_pad_at_rest_does_nothing() {
        let mut m = Mapper::default();
        assert_eq!(run(&mut m, PadState::default()), Frame::default());
        // A stick resting a little off its centre is still at rest.
        assert_eq!(run(&mut m, PadState { left: (0.1, -0.1), right: (0.0, 0.15), ..Default::default() }), Frame::default());
    }

    #[test]
    fn the_stick_moves_the_pointer_slowly_then_faster() {
        let mut m = Mapper::default();
        let pad = PadState { left: (1.0, 0.0), ..Default::default() };
        let first = m.step(&pad, 0.1, (500.0, 400.0), SCREEN, false).pointer.unwrap();
        assert!(first.0 > 500.0 && first.0 < 560.0 && first.1 == 400.0, "{first:?}");
        for _ in 0..10 {
            m.step(&pad, 0.1, (500.0, 400.0), SCREEN, false);
        }
        let later = m.step(&pad, 0.1, (500.0, 400.0), SCREEN, false).pointer.unwrap();
        assert!((later.0 - 630.0).abs() < 1.0, "{later:?}");
        // Let go, it starts slow again.
        run(&mut m, PadState::default());
        assert_eq!(m.step(&pad, 0.1, (500.0, 400.0), SCREEN, false).pointer, Some(first));
    }

    #[test]
    fn a_small_push_moves_slower_than_a_full_one() {
        let half = Mapper::default().step(&PadState { left: (0.0, 0.6), ..Default::default() }, 0.1, (0.0, 0.0), SCREEN, false);
        let full = Mapper::default().step(&PadState { left: (0.0, 1.0), ..Default::default() }, 0.1, (0.0, 0.0), SCREEN, false);
        assert!(half.pointer.unwrap().1 < full.pointer.unwrap().1 / 2.0);
    }

    #[test]
    fn the_dpad_moves_the_pointer_and_wins_over_the_stick() {
        let mut m = Mapper::default();
        let pad = PadState { left: (1.0, 0.0), ..Default::default() }.with(Button::Up).with(Button::Left);
        let (x, y) = run(&mut m, pad).pointer.unwrap();
        assert!(x < 500.0 && y < 400.0 && (500.0 - x - (400.0 - y)).abs() < 1e-3);
    }

    #[test]
    fn the_pointer_stays_on_the_screen() {
        let mut m = Mapper::default();
        let pad = PadState { left: (-1.0, 1.0), ..Default::default() };
        assert_eq!(m.step(&pad, 0.1, (2.0, 766.0), SCREEN, false).pointer, Some((0.0, 767.0)));
    }

    #[test]
    fn a_long_frame_does_not_throw_the_pointer() {
        let mut m = Mapper::default();
        let pad = PadState { left: (1.0, 0.0), ..Default::default() };
        let after_hitch = m.step(&pad, 5.0, (0.0, 0.0), SCREEN, false).pointer.unwrap();
        assert!(after_hitch.0 < 200.0);
    }

    #[test]
    fn a_and_b_are_the_mouse_buttons_pressed_held_and_let_go() {
        let mut m = Mapper::default();
        let f = run(&mut m, PadState::default().with(Button::South));
        assert_eq!(f.left, Edge { pressed: true, down: true, released: false });
        assert_eq!(f.right, Edge::default());
        let f = run(&mut m, PadState::default().with(Button::South).with(Button::East));
        assert_eq!(f.left, Edge { pressed: false, down: true, released: false });
        assert_eq!(f.right, Edge { pressed: true, down: true, released: false });
        let f = run(&mut m, PadState::default());
        assert_eq!(f.left, Edge { pressed: false, down: false, released: true });
        assert_eq!(f.right, Edge { pressed: false, down: false, released: true });
        assert_eq!(run(&mut m, PadState::default()), Frame::default());
    }

    #[test]
    fn a_drag_holds_the_button_while_the_pointer_moves() {
        let mut m = Mapper::default();
        run(&mut m, PadState::default().with(Button::South));
        let f = run(&mut m, PadState { left: (0.0, 1.0), ..Default::default() }.with(Button::South));
        assert!(f.left.down && !f.left.pressed && f.pointer.is_some());
    }

    #[test]
    fn start_x_and_y_are_esc_enter_and_space() {
        let mut m = Mapper::default();
        let f = run(&mut m, PadState::default().with(Button::Start));
        assert_eq!(f.keys, vec![(KeyCode::Escape, Edge { pressed: true, down: true, released: false })]);
        let f = run(&mut m, PadState::default().with(Button::West).with(Button::North));
        assert_eq!(
            f.keys,
            vec![
                (KeyCode::Escape, Edge { pressed: false, down: false, released: true }),
                (KeyCode::Enter, Edge { pressed: true, down: true, released: false }),
                (KeyCode::Space, Edge { pressed: true, down: true, released: false }),
            ]
        );
    }

    #[test]
    fn the_shoulders_turn_the_wheel_once_then_again_while_held() {
        let mut m = Mapper::default();
        let lb = PadState::default().with(Button::LeftBumper);
        assert_eq!(m.step(&lb, 0.1, (0.0, 0.0), SCREEN, false).wheel, 1.0);
        let held: f32 = (0..18).map(|_| m.step(&lb, 0.0625, (0.0, 0.0), SCREEN, false).wheel).sum();
        // 1.125 s held: repeats at 0.4, 0.5 … 1.1 s.
        assert_eq!(held, 8.0);
        m.step(&PadState::default(), 0.1, (0.0, 0.0), SCREEN, false);
        assert_eq!(m.step(&PadState::default().with(Button::RightBumper), 0.1, (0.0, 0.0), SCREEN, false).wheel, -1.0);
    }

    #[test]
    fn the_right_stick_scrolls_the_map_or_turns_the_wheel() {
        let up = PadState { right: (0.0, -1.0), ..Default::default() };
        let mut m = Mapper::default();
        let f = m.step(&up, 0.1, (0.0, 0.0), SCREEN, true);
        assert_eq!((f.scroll, f.wheel), ((0.0, -1.0), 0.0));
        let mut m = Mapper::default();
        let steps: f32 = (0..10).map(|_| m.step(&up, 0.1, (0.0, 0.0), SCREEN, false).wheel).sum();
        assert!((9.0..=10.0).contains(&steps), "{steps}");
        let down = PadState { right: (0.0, 1.0), ..Default::default() };
        assert!(m.step(&down, 0.2, (0.0, 0.0), SCREEN, false).wheel < 0.0);
    }

    #[test]
    fn a_disconnected_pad_lets_its_buttons_go() {
        let mut m = Mapper::default();
        run(&mut m, PadState::default().with(Button::South).with(Button::Start));
        let f = run(&mut m, PadState::default());
        assert!(f.left.released);
        assert_eq!(f.keys, vec![(KeyCode::Escape, Edge { pressed: false, down: false, released: true })]);
    }
}
