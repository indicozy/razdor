//! The interface's pan: at an interface scale larger than the screen, the windows and the
//! screens are drawn shifted by an offset, so a big window can be moved over the screen (the
//! owner's request, with the scale up to ×4).
//!
//! The offset is a camera translation (`set_camera`): everything drawn in the frame shifts by
//! it, and the pointer follows (`input::mouse_position` takes it off). It is clamped so the
//! canvas edges can come to the screen edges and no further (`offset_range`). It resets when a
//! screen or a dialog opens, so the window opened is centred again. What stays where it is:
//! the bottom bar, the FPS counter and the keyboard strip (`unpanned`, and `end_frame` for what
//! the frame draws last), the map under a window (`window_backdrop`) and the title background.
//! Input: a middle-button drag pans (desktop); on phones a one-finger drag does (`gestures`).

use std::cell::{Cell, RefCell};
use std::mem::Discriminant;

use macroquad::camera::{set_camera, set_default_camera, Camera2D};
use macroquad::math::Rect;
use macroquad::window::{screen_height, screen_width};
use razdor::local;

use super::{chrome, Screen};

/// The interface's canvas at scale `k`: the 960×720 reference.
pub fn canvas(k: f32) -> (f32, f32) {
    (960.0 * k, 720.0 * k)
}

/// Where the canvas sits with no offset at scale `k` on a `screen`: centred across, and
/// vertically as the title screens (`chrome::centred_top`).
pub fn origin(screen: (f32, f32), k: f32) -> (f32, f32) {
    let (cw, ch) = canvas(k);
    ((screen.0 - cw) / 2.0, chrome::centred_top(screen.0, screen.1, ch))
}

/// The offset's range on one axis: a canvas of `size` whose edge sits at `origin` with no
/// offset may move until its far edge reaches the screen's (`screen`) and the near one meets
/// the screen's edge; no range where it fits.
pub fn offset_range(screen: f32, size: f32, origin: f32) -> (f32, f32) {
    if size <= screen { (0.0, 0.0) } else { (screen - size - origin, -origin) }
}

/// The pan's state, as the frame sees it (pure: the macroquad calls are in [`begin_frame`]).
#[derive(Default, Debug)]
pub struct Pan {
    /// Screen pixels the content is drawn shifted by (whole pixels).
    pub offset: (f32, f32),
    /// Where the middle-button drag was last seen.
    drag: Option<(f32, f32)>,
    /// The screen and the dialogs at the last frame: a change of either resets the offset.
    screen: Option<Discriminant<Screen>>,
    dialogs: usize,
}

/// What one frame gives the pan (see [`Pan::frame`]).
#[derive(Clone, Copy)]
pub struct Input {
    /// The screen can be panned now.
    pub allowed: bool,
    pub screen: Discriminant<Screen>,
    pub dialogs: usize,
    /// The pointer on the screen, and whether the middle button is down.
    pub pointer: (f32, f32),
    pub middle: bool,
    /// The one-finger pan of this frame (`input::map_pan`), in screen pixels.
    pub touch: (f32, f32),
    /// The screen's size, and the interface scale.
    pub size: (f32, f32),
    pub k: f32,
}

impl Pan {
    /// One frame: resets on a new screen or dialog, moves by a middle-button drag or a finger's
    /// pan while `allowed`, and clamps to the canvas at the interface scale.
    pub fn frame(&mut self, input: &Input) {
        let Input { allowed, screen, dialogs, pointer, middle, touch, size, k } = *input;
        let opened = self.screen != Some(screen) || dialogs > self.dialogs;
        self.screen = Some(screen);
        self.dialogs = dialogs;
        if opened || !allowed {
            self.offset = (0.0, 0.0);
        }
        if allowed {
            self.offset.0 += touch.0;
            self.offset.1 += touch.1;
        }
        self.drag = match self.drag {
            Some(last) if allowed && middle => {
                self.offset.0 += pointer.0 - last.0;
                self.offset.1 += pointer.1 - last.1;
                Some(pointer)
            }
            None if allowed && middle => Some(pointer),
            _ => None,
        };
        let (cw, ch) = canvas(k);
        let org = origin(size, k);
        let (x0, x1) = offset_range(size.0, cw, org.0);
        let (y0, y1) = offset_range(size.1, ch, org.1);
        self.offset = (self.offset.0.round().clamp(x0, x1), self.offset.1.round().clamp(y0, y1));
    }
}

local! {
    static PAN: RefCell<Pan> = RefCell::new(Pan::default());
    /// The offset applies now: false inside [`unpanned`] and after [`end_frame`].
    static ON: Cell<bool> = Cell::new(false);
    /// The camera of the offset is set (so the default one is put back only when needed).
    static CAMERA: Cell<bool> = Cell::new(false);
}

/// The offset the pointer and the drawing use now (zero where nothing is shifted).
pub fn shift() -> (f32, f32) {
    if ON.with(|c| c.get()) { PAN.with(|p| p.borrow().offset) } else { (0.0, 0.0) }
}

/// Puts the camera the state asks for (the offset's translation, or the default one).
fn sync() {
    let (on, (ox, oy)) = (ON.with(|c| c.get()), PAN.with(|p| p.borrow().offset));
    let want = on && (ox != 0.0 || oy != 0.0);
    if want {
        let (sw, sh) = (screen_width(), screen_height());
        set_camera(&Camera2D::from_display_rect(Rect::new(-ox, -oy, sw, sh)));
    } else if CAMERA.with(|c| c.get()) {
        set_default_camera();
    }
    CAMERA.with(|c| c.set(want));
}

/// Once a frame, before the input: `allowed` says whether the screen can be panned now (the
/// title and the windows; not the map alone, nor the editor).
/// Must run after `input::begin_frame` (it takes that frame's finger pan).
pub fn begin_frame(allowed: bool, screen: Discriminant<Screen>, dialogs: usize) {
    let input = Input {
        allowed,
        screen,
        dialogs,
        pointer: super::input::screen_pointer(),
        middle: super::input::is_mouse_button_down(macroquad::input::MouseButton::Middle),
        touch: super::input::map_pan(),
        size: (screen_width(), screen_height()),
        k: chrome::k(),
    };
    PAN.with(|p| p.borrow_mut().frame(&input));
    ON.with(|c| c.set(true));
    sync();
}

/// Whether the canvas is larger than the screen at the current scale (the windows can pan).
pub fn overflows() -> bool {
    let (sw, sh) = (screen_width(), screen_height());
    let (cw, ch) = canvas(chrome::k());
    cw > sw || ch > sh
}

/// A window's background and frame (`r`, in the canvas) is a one-finger pan area while the
/// canvas overflows: a slide over it pans the window; controls drawn over it keep their own
/// presses (a list's rows scroll, a tap clicks). Registered while the window is drawn.
pub fn window_area(r: Rect) {
    if !overflows() {
        return;
    }
    let (ox, oy) = shift();
    super::input::swipe_area(Rect::new(r.x + ox, r.y + oy, r.w, r.h), super::gestures::SwipeKind::Pan);
}

/// The frame's end: what is drawn after it (the FPS counter, the keyboard strip, the pointer)
/// stays where the screen is.
pub fn end_frame() {
    ON.with(|c| c.set(false));
    sync();
}

/// Runs `f` with the offset off (the bar, the map under a window): its pointer is the screen's too.
pub fn unpanned<R>(f: impl FnOnce() -> R) -> R {
    let was = ON.with(|c| c.replace(false));
    sync();
    let r = f();
    ON.with(|c| c.set(was));
    sync();
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame of the pan with no finger's pan (the drags and the reset tests).
    #[allow(clippy::too_many_arguments)]
    fn step(p: &mut Pan, allowed: bool, screen: Discriminant<Screen>, dialogs: usize, pointer: (f32, f32), middle: bool, size: (f32, f32), k: f32) {
        p.frame(&Input { allowed, screen, dialogs, pointer, middle, touch: (0.0, 0.0), size, k });
    }

    fn screen_a() -> Discriminant<Screen> {
        std::mem::discriminant(&Screen::MainMenu)
    }

    fn screen_b() -> Discriminant<Screen> {
        std::mem::discriminant(&Screen::Settings)
    }

    #[test]
    fn the_offset_runs_until_the_canvas_edges_meet_the_screen_edges() {
        // A 1280×800 screen at ×2: the canvas is 1920×1440, centred across (-320) and from the top.
        let (sw, sh) = (1280.0, 800.0);
        assert_eq!(origin((sw, sh), 2.0), (-320.0, 0.0));
        assert_eq!(offset_range(sw, 1920.0, -320.0), (-320.0, 320.0), "across: half the excess each way");
        assert_eq!(offset_range(sh, 1440.0, 0.0), (-640.0, 0.0), "down: the canvas bottom to the screen's bottom, no further");
        assert_eq!(offset_range(sw, 960.0, 160.0), (0.0, 0.0), "a canvas that fits does not pan");
    }

    #[test]
    fn a_drag_clamps_at_the_edges() {
        let mut p = Pan::default();
        let size = (1280.0, 800.0);
        step(&mut p, true, screen_a(), 0, (100.0, 100.0), true, size, 2.0);
        step(&mut p, true, screen_a(), 0, (150.0, 60.0), true, size, 2.0);
        assert_eq!(p.offset, (50.0, -40.0), "the drag follows the pointer");
        step(&mut p, true, screen_a(), 0, (2000.0, 60.0), true, size, 2.0);
        assert_eq!(p.offset, (320.0, -40.0), "across stops at the canvas's edge");
        step(&mut p, true, screen_a(), 0, (2000.0, 0.0), true, size, 2.0);
        assert_eq!(p.offset, (320.0, -100.0), "a drag up moves the canvas");
        step(&mut p, true, screen_a(), 0, (2000.0, 0.0), false, size, 2.0);
        assert_eq!(p.offset, (320.0, -100.0), "a release keeps the offset");
        step(&mut p, true, screen_a(), 0, (0.0, 0.0), true, size, 2.0);
        step(&mut p, true, screen_a(), 0, (0.0, -2000.0), true, size, 2.0);
        assert_eq!(p.offset.1, -640.0, "down stops at the bottom edge");
    }

    #[test]
    fn a_new_screen_or_dialog_resets_to_centred() {
        let mut p = Pan::default();
        let size = (1280.0, 800.0);
        step(&mut p, true, screen_a(), 0, (100.0, 100.0), true, size, 2.0);
        step(&mut p, true, screen_a(), 0, (160.0, 100.0), true, size, 2.0);
        assert_eq!(p.offset.0, 60.0);
        step(&mut p, true, screen_b(), 0, (160.0, 100.0), false, size, 2.0);
        assert_eq!(p.offset, (0.0, 0.0), "a window opened is centred");
        step(&mut p, true, screen_b(), 0, (160.0, 100.0), true, size, 2.0);
        step(&mut p, true, screen_b(), 0, (200.0, 100.0), true, size, 2.0);
        assert_eq!(p.offset.0, 40.0);
        step(&mut p, true, screen_b(), 1, (200.0, 100.0), true, size, 2.0);
        assert_eq!(p.offset, (0.0, 0.0), "so does a dialog on top");
    }

    #[test]
    fn a_screen_that_is_not_allowed_stays_centred() {
        let mut p = Pan::default();
        let size = (1280.0, 800.0);
        step(&mut p, true, screen_a(), 0, (100.0, 100.0), true, size, 2.0);
        step(&mut p, true, screen_a(), 0, (160.0, 100.0), true, size, 2.0);
        step(&mut p, false, screen_a(), 0, (200.0, 100.0), true, size, 2.0);
        assert_eq!(p.offset, (0.0, 0.0), "the map alone, the editor: no pan");
    }

    #[test]
    fn a_fitting_scale_never_pans() {
        let mut p = Pan::default();
        step(&mut p, true, screen_a(), 0, (100.0, 100.0), true, (1024.0, 768.0), 1.0);
        step(&mut p, true, screen_a(), 0, (300.0, 300.0), true, (1024.0, 768.0), 1.0);
        assert_eq!(p.offset, (0.0, 0.0));
    }

    #[test]
    fn a_finger_pan_moves_the_window_and_clamps_like_the_drag() {
        let mut p = Pan::default();
        let size = (1280.0, 800.0);
        let touch = |dx: f32, dy: f32| Input { allowed: true, screen: screen_a(), dialogs: 0, pointer: (0.0, 0.0), middle: false, touch: (dx, dy), size, k: 2.0 };
        p.frame(&touch(0.0, 0.0));
        p.frame(&touch(-30.0, -20.0));
        assert_eq!(p.offset, (-30.0, -20.0), "the pan follows the finger, 1:1");
        p.frame(&touch(-500.0, 0.0));
        assert_eq!(p.offset, (-320.0, -20.0), "across stops at the canvas edge");
        p.frame(&Input { allowed: false, ..touch(10.0, 10.0) });
        assert_eq!(p.offset, (0.0, 0.0), "a screen that cannot pan stays centred");
    }
}
