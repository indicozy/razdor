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
//!
//! Inert since 1.0.1: the interface scale is kept to the screen's fit (`display::scale`), so the
//! canvas never exceeds the screen and nothing pans. The pan stays for a later, safer return of
//! larger scales.

use std::cell::{Cell, RefCell};
use std::mem::Discriminant;

use macroquad::camera::{set_camera, set_default_camera, Camera2D};
use macroquad::math::Rect;
use macroquad::window::{get_internal_gl, screen_dpi_scale, screen_height, screen_width};
use razdor::local;

use super::input;
use super::widgets::{pointer, swallow_input};
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

/// The canvas point under a screen point at `shift` (the pointer's offset, `input::mouse_position`).
pub fn to_canvas(screen: (f32, f32), shift: (f32, f32)) -> (f32, f32) {
    (screen.0 - shift.0, screen.1 - shift.1)
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
    /// The screen's size, the interface scale, and the bottom bar's height (its top is the
    /// lowest the canvas may come to).
    pub size: (f32, f32),
    pub k: f32,
    pub bar: f32,
}

impl Pan {
    /// One frame: resets on a new screen or dialog, moves by a middle-button drag or a finger's
    /// pan while `allowed`, and clamps to the canvas at the interface scale.
    pub fn frame(&mut self, input: &Input) {
        let Input { allowed, screen, dialogs, pointer, middle, touch, size, k, bar } = *input;
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
        self.settle(size, k, bar);
    }

    /// Clamps the offset to the canvas at scale `k` on a `size` screen whose bottom `bar` pixels
    /// are the bottom bar: the canvas's bottom edge comes up to the bar's top, not the screen's
    /// bottom, and nothing pans while the canvas fits the screen's height.
    fn settle(&mut self, size: (f32, f32), k: f32, bar: f32) {
        let (cw, ch) = canvas(k);
        let org = origin(size, k);
        let (x0, x1) = offset_range(size.0, cw, org.0);
        let (y0, y1) = if ch > size.1 { offset_range(size.1 - bar, ch, org.1) } else { (0.0, 0.0) };
        self.offset = (self.offset.0.round().clamp(x0, x1), self.offset.1.round().clamp(y0, y1));
    }

    /// Sets the offset to `to` (the snapshot's `RAZDOR_SCENE_PAN`), clamped as a drag is.
    fn force(&mut self, to: (f32, f32), size: (f32, f32), k: f32, bar: f32) {
        self.offset = to;
        self.settle(size, k, bar);
    }
}

local! {
    static PAN: RefCell<Pan> = RefCell::new(Pan::default());
    /// The offset applies now: false inside [`unpanned`] and after [`end_frame`].
    static ON: Cell<bool> = Cell::new(false);
    /// The camera of the offset is set (so the default one is put back only when needed).
    static CAMERA: Cell<bool> = Cell::new(false);
    /// The bottom bar was drawn over a window this frame ([`cover_bar`]): the drawing and the
    /// pointer stop above the bar until [`end_frame`].
    static COVER: Cell<bool> = Cell::new(false);
    /// The offset the snapshot holds (`RAZDOR_SCENE_PAN=dx,dy`, `ui::snapshot`).
    static FORCED: Cell<Option<(f32, f32)>> = Cell::new(None);
}

/// Holds the offset at `to` for good (debug snapshots only).
pub fn force_for_snapshot(to: Option<(f32, f32)>) {
    FORCED.with(|f| f.set(to));
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
        // `from_display_rect` sets a negative `zoom.y` for a render target; drawn to the screen,
        // macroquad's matrix negates it again, so it is put back here (else the frame is flipped).
        let mut camera = Camera2D::from_display_rect(Rect::new(-ox, -oy, sw, sh));
        camera.zoom.y = -camera.zoom.y;
        set_camera(&camera);
    } else if CAMERA.with(|c| c.get()) {
        set_default_camera();
    }
    CAMERA.with(|c| c.set(want));
}

/// Once a frame, before the input: `allowed` says whether the screen can be panned now (the
/// title and the windows; not the map alone, nor the editor).
/// Must run after `input::begin_frame` (it takes that frame's finger pan).
pub fn begin_frame(allowed: bool, screen: Discriminant<Screen>, dialogs: usize) {
    let size = (screen_width(), screen_height());
    let input = Input {
        allowed,
        screen,
        dialogs,
        pointer: super::input::screen_pointer(),
        middle: super::input::is_mouse_button_down(macroquad::input::MouseButton::Middle),
        touch: super::input::map_pan(),
        size,
        k: chrome::k(),
        bar: chrome::bar_height(),
    };
    PAN.with(|p| {
        let mut p = p.borrow_mut();
        p.frame(&input);
        if let (true, Some(to)) = (allowed, FORCED.with(|f| f.get())) {
            p.force(to, size, input.k, input.bar);
        }
    });
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
    super::input::swipe_area(clipped(on_screen(r)), super::gestures::SwipeKind::Pan);
}

/// A rectangle a unit drag starts on (a card, a grid of cards; `r` in the canvas, as
/// `window_area`): while the canvas overflows and the units can be dragged, a slide over it is
/// the drag, not a pan of the window (`SwipeKind::Off`, which keeps a drag a drag). Registered
/// after the window's own area, so it wins over it there. Clicks-only leaves the window's pan.
pub fn drag_source(r: Rect) {
    if !overflows() || !super::unit_drag::controls().drags() {
        return;
    }
    super::input::swipe_area(clipped(on_screen(r)), super::gestures::SwipeKind::Off);
}

/// An item's cell or worn slot (`item_pull`, Razdor's): a one-finger slide over it pulls the item
/// (a drag), or, a quick vertical one over a list of `row_px` rows, scrolls the list. Clipped as
/// `drag_source`; gestures only.
pub fn pull_source(r: Rect, row_px: Option<f32>) {
    super::input::swipe_area(clipped(on_screen(r)), super::gestures::SwipeKind::Pull { row_px });
}

/// [`drag_source`] over the bounding box of a grid's cell rectangles (a contiguous block of
/// cards); nothing when there are none.
pub fn drag_grid(cells: impl IntoIterator<Item = Rect>) {
    if let Some(r) = bounds(cells) {
        drag_source(r);
    }
}

/// The smallest rectangle holding every one of `rects` (`None` for none).
fn bounds(rects: impl IntoIterator<Item = Rect>) -> Option<Rect> {
    rects.into_iter().reduce(|a, b| {
        let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
        let (x1, y1) = ((a.x + a.w).max(b.x + b.w), (a.y + a.h).max(b.y + b.h));
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    })
}

/// The top of the bottom bar on the screen (the bar's own scale, `chrome::bar_height`).
pub fn bar_top() -> f32 {
    screen_height() - chrome::bar_height()
}

/// The screen above the bar: what a window drawn over the bar may show ([`cover_bar`]).
fn above_bar() -> Rect {
    Rect::new(0.0, 0.0, screen_width(), bar_top())
}

/// Cuts the drawing off outside `clip` (screen pixels), or lifts the cut (`None`).
fn scissor(clip: Option<Rect>) {
    let dpi = screen_dpi_scale();
    let gl = unsafe { get_internal_gl() }.quad_gl;
    gl.scissor(clip.map(|r| ((r.x * dpi) as i32, (r.y * dpi) as i32, (r.w * dpi).ceil() as i32, (r.h * dpi).ceil() as i32)));
}

/// Called once the bottom bar is drawn for a window (or a dialog over the map): when the canvas
/// overflows the screen, the rest of the frame is cut off above the bar, and a press on the bar
/// is the bar's alone (`covered`). The bar is drawn first, so its buttons keep their presses.
pub fn cover_bar() {
    if !overflows() {
        return;
    }
    COVER.with(|c| c.set(true));
    scissor(Some(above_bar()));
    if covered(pointer()) && input::is_mouse_button_pressed(macroquad::input::MouseButton::Left) {
        swallow_input();
    }
}

/// Whether the canvas point `p` lies on the bottom bar while a window covers it: a window's
/// controls take no hover or press there.
pub fn covered(p: (f32, f32)) -> bool {
    covers(COVER.with(|c| c.get()), p.1 + shift().1, bar_top())
}

/// Whether a screen height `y` is on the bar (at `top` and below) while a window covers it.
fn covers(cover: bool, y: f32, top: f32) -> bool {
    cover && y >= top
}

/// A canvas rectangle where it lands on the screen with the pan (a scissor works on the screen).
pub fn on_screen(r: Rect) -> Rect {
    let (ox, oy) = shift();
    Rect::new(r.x + ox, r.y + oy, r.w, r.h)
}

/// `r` cut to the screen above the bar while a window covers it (the `widgets::Clip` rects).
pub fn clipped(r: Rect) -> Rect {
    if !COVER.with(|c| c.get()) {
        return r;
    }
    let bottom = (r.y + r.h).min(bar_top());
    Rect::new(r.x, r.y, r.w, (bottom - r.y).max(0.0))
}

/// Puts back the cut the bar makes after a `widgets::Clip` ends (`None` when no bar covers).
pub fn restore_scissor() {
    scissor(COVER.with(|c| c.get()).then(above_bar));
}

/// The frame's end: what is drawn after it (the FPS counter, the keyboard strip, the pointer)
/// stays where the screen is, and no bar cut stays on.
pub fn end_frame() {
    ON.with(|c| c.set(false));
    sync();
    COVER.with(|c| c.set(false));
    scissor(None);
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
        p.frame(&Input { allowed, screen, dialogs, pointer, middle, touch: (0.0, 0.0), size, k, bar: 0.0 });
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
        let touch = |dx: f32, dy: f32| Input { allowed: true, screen: screen_a(), dialogs: 0, pointer: (0.0, 0.0), middle: false, touch: (dx, dy), size, k: 2.0, bar: 0.0 };
        p.frame(&touch(0.0, 0.0));
        p.frame(&touch(-30.0, -20.0));
        assert_eq!(p.offset, (-30.0, -20.0), "the pan follows the finger, 1:1");
        p.frame(&touch(-500.0, 0.0));
        assert_eq!(p.offset, (-320.0, -20.0), "across stops at the canvas edge");
        p.frame(&Input { allowed: false, ..touch(10.0, 10.0) });
        assert_eq!(p.offset, (0.0, 0.0), "a screen that cannot pan stays centred");
    }

    #[test]
    fn the_canvas_stops_at_the_bars_top_not_the_screens_bottom() {
        // A 1280×800 screen at ×2 with an 89 px bar: the canvas's bottom comes up to y = 711.
        let mut p = Pan::default();
        let size = (1280.0, 800.0);
        let touch = |dy: f32| Input { allowed: true, screen: screen_a(), dialogs: 0, pointer: (0.0, 0.0), middle: false, touch: (0.0, dy), size, k: 2.0, bar: 89.0 };
        p.frame(&touch(0.0));
        p.frame(&touch(-2000.0));
        assert_eq!(p.offset.1, -(1440.0 - (800.0 - 89.0)), "the bottom edge at the bar's top");
        assert_eq!(p.offset.1, -729.0);
    }

    #[test]
    fn a_canvas_that_fits_the_height_does_not_pan_for_the_bar() {
        // At the fit scale the canvas is the screen's height: the bar overlaps it, no pan.
        let mut p = Pan::default();
        let size = (1024.0, 768.0);
        let touch = |dy: f32| Input { allowed: true, screen: screen_a(), dialogs: 0, pointer: (0.0, 0.0), middle: false, touch: (0.0, dy), size, k: 1.0, bar: 80.0 };
        p.frame(&touch(0.0));
        p.frame(&touch(-200.0));
        assert_eq!(p.offset, (0.0, 0.0));
    }

    #[test]
    fn a_click_where_a_panned_button_is_drawn_hits_it() {
        // A button at (100, 50) 80×20 in the canvas, panned by (-30, -40): drawn at (70, 10).
        let button = Rect::new(100.0, 50.0, 80.0, 20.0);
        let shift = (-30.0, -40.0);
        let drawn = Rect::new(button.x + shift.0, button.y + shift.1, button.w, button.h);
        let hit = |p: (f32, f32)| {
            let c = to_canvas(p, shift);
            c.0 >= button.x && c.0 < button.x + button.w && c.1 >= button.y && c.1 < button.y + button.h
        };
        assert!(hit((drawn.x + 5.0, drawn.y + 5.0)), "a press on the drawn button's corner hits it");
        assert!(hit((drawn.x + drawn.w / 2.0, drawn.y + drawn.h / 2.0)), "and its middle");
        assert!(!hit((button.x + 5.0, button.y + 5.0)), "the place the button has with no pan does not");
        assert!(!hit((drawn.x - 1.0, drawn.y + 5.0)), "a press just outside it does not");
    }

    #[test]
    fn the_bar_takes_the_presses_over_a_window_that_covers_it() {
        // The bar's top at 711 on a 800-high screen: a press at 711 and below is the bar's.
        assert!(covers(true, 711.0, 711.0));
        assert!(covers(true, 790.0, 711.0));
        assert!(!covers(true, 710.0, 711.0), "just above the bar is the window's");
        assert!(!covers(false, 790.0, 711.0), "no bar over the window, nothing taken");
    }

    #[test]
    fn a_grid_of_drag_sources_is_the_box_around_its_cells() {
        let cells = [Rect::new(10.0, 20.0, 8.0, 12.0), Rect::new(30.0, 20.0, 8.0, 12.0), Rect::new(10.0, 50.0, 8.0, 12.0)];
        assert_eq!(bounds(cells), Some(Rect::new(10.0, 20.0, 28.0, 42.0)));
        assert_eq!(bounds(std::iter::empty()), None, "no cells, no drag source");
    }
}
