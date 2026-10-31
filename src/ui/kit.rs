//! Razdor's own (the owner's decision, 2026-11-01): the shared pieces every window is built
//! from, so that on a phone no text is under 12 sp and no touch target under 40 dp, and an
//! upright phone gets a real upright layout. On the desktop every function returns what the
//! hand-written code computed (`design * k`), so the desktop draws as before. See
//! `docs/flows/ui-kit.md`.

use macroquad::prelude::*;
use razdor::local;
use std::cell::RefCell;
use std::collections::HashMap;

use super::{audio, chrome, display, dt_font, widgets, window_layout};

// ------------------------------------------------------------------------------------------
// Metrics
// ------------------------------------------------------------------------------------------

/// Phone screen's dp per screen px, or `None` on the desktop.
pub fn phone() -> Option<f32> {
    display::dp_px()
}

/// Whether the phone is held upright (the stacked layout).
pub fn upright() -> bool {
    window_layout::portrait()
}

/// The smallest text on a phone, in sp (the owner's decision, 2026-11-01).
pub const FLOOR_SP: f32 = 12.0;

/// The phone's default text size (`display::TEXT_SIZES`' middle one).
pub const PHONE_DEFAULT_TEXT: f32 = 1.25;

/// The smallest capital height a phone text may have, in px: [`FLOOR_SP`] sp (a capital is
/// 0.7 of the sp size), scaled up with a larger text size and never down. `None` on the desktop.
/// Pure twin of [`min_cap_px`].
pub fn min_cap_at(dp: Option<f32>, text_size: f32) -> Option<f32> {
    dp.map(|d| FLOOR_SP * 0.7 * d * (text_size / PHONE_DEFAULT_TEXT).max(1.0))
}

/// [`min_cap_at`] for the current phone and text size.
pub fn min_cap_px() -> Option<f32> {
    min_cap_at(display::dp_px(), display::text_size())
}

/// `size` raised so that its capital height reaches `min_cap`: `size * min_cap / cap` when
/// `cap` (the capital height at `size`) is below it, else `size`. No floor (`None`) keeps `size`.
/// Pure twin of [`floored`].
pub fn floored_at(size: f32, cap: f32, min_cap: Option<f32>) -> f32 {
    match min_cap {
        Some(m) if cap > 0.0 && cap < m => size * m / cap,
        _ => size,
    }
}

/// `size` raised to the phone's floor for the current face (its capital "Н" measured with the
/// legacy metrics, as the phone draws it). Unchanged on the desktop.
pub fn floored(size: f32) -> f32 {
    let Some(min_cap) = min_cap_px() else {
        return size;
    };
    let cap = dt_font::legacy_metrics_if(true, || widgets::measure_exact("Н", size).offset_y);
    floored_at(size, cap, Some(min_cap))
}

/// The pre-floor size of a text the design gives `design` px: desktop `design * k`, phone
/// `design * k * text_size`. Pure twin of [`text_px`].
pub fn text_px_at(design: f32, k: f32, dp: Option<f32>, text_size: f32) -> f32 {
    match dp {
        None => design * k,
        Some(_) => design * k * text_size,
    }
}

/// The size of a text the design gives `design` px: desktop `design * k`; phone the same with
/// the text size, raised to the floor ([`floored`]).
pub fn text_px(design: f32) -> f32 {
    let k = chrome::k();
    let dp = display::dp_px();
    match dp {
        None => text_px_at(design, k, None, display::text_size()),
        Some(_) => floored(text_px_at(design, k, dp, display::text_size())),
    }
}

/// The smallest touch target on a phone, in dp (the owner's decision, 2026-11-01).
pub const TOUCH_DP: f32 = 44.0;

/// A button's height: `design_h * k * g` (`g` the button growth with the text size), and on a
/// phone at least [`TOUCH_DP`] dp. Pure twin of [`touch_h`].
pub fn touch_h_at(design_h: f32, k: f32, g: f32, dp: Option<f32>) -> f32 {
    let h = design_h * k * g;
    match dp {
        Some(d) => h.max(TOUCH_DP * d),
        None => h,
    }
}

/// A button's height for a design height `design_h`, for the current scale and text size.
pub fn touch_h(design_h: f32) -> f32 {
    touch_h_at(design_h, chrome::k(), display::button_grow(), display::dp_px())
}

// ------------------------------------------------------------------------------------------
// Text
// ------------------------------------------------------------------------------------------

/// A text drawn by the kit: the string, its design size in px, its colour, an optional face,
/// the shadow (default on) and the line pitch in design px (default `design * 17 / 12`, the
/// settings' hints).
pub struct Text<'a> {
    /// The text (`\n` starts a new paragraph).
    pub s: &'a str,
    /// The size in design px (at the desktop's scale 1).
    pub design: f32,
    /// The colour.
    pub color: Color,
    /// The face to draw in; `None` keeps the current one.
    pub face: Option<dt_font::Face>,
    /// Whether to draw the original's dark shadow.
    pub shadow: bool,
    /// The line pitch in design px.
    pub pitch: f32,
}

impl<'a> Text<'a> {
    /// A shadowed text of `design` px in `color`, in the current face.
    pub fn new(s: &'a str, design: f32, color: Color) -> Text<'a> {
        Text { s, design, color, face: None, shadow: true, pitch: design * 17.0 / 12.0 }
    }

    /// Draws in `face`.
    pub fn face(mut self, face: dt_font::Face) -> Self {
        self.face = Some(face);
        self
    }

    /// Draws without the shadow.
    pub fn plain(mut self) -> Self {
        self.shadow = false;
        self
    }

    /// Sets the line pitch in design px.
    pub fn pitch(mut self, p: f32) -> Self {
        self.pitch = p;
        self
    }

    /// Runs `f` in this text's face when it has one.
    fn in_face<R>(&self, f: impl FnOnce() -> R) -> R {
        match self.face {
            Some(face) => dt_font::with_face(face, f),
            None => f(),
        }
    }

    /// The size the text is drawn at ([`text_px`] of the design size, in its face).
    pub fn size(&self) -> f32 {
        self.in_face(|| text_px(self.design))
    }

    /// The distance between two lines: desktop `pitch * k`; phone `pitch * k * text_size`, but
    /// at least 1.4 times the size.
    pub fn line_pitch(&self) -> f32 {
        let k = chrome::k();
        match display::dp_px() {
            None => self.pitch * k,
            Some(_) => (self.pitch * k * display::text_size()).max(self.size() * 1.4),
        }
    }

    /// The lines of the text wrapped to `width` px.
    pub fn lines(&self, width: f32) -> Vec<String> {
        let size = self.size();
        self.in_face(|| dt_font::legacy_metrics_if(display::dp_px().is_some(), || widgets::wrap(self.s, width, size)))
    }

    /// The height of the wrapped text, `width` px wide.
    pub fn height(&self, width: f32) -> f32 {
        self.lines(width).len() as f32 * self.line_pitch()
    }

    /// Draws the text wrapped to `width`, its first baseline at `baseline` and each next line one
    /// pitch lower.
    pub fn draw(&self, x: f32, baseline: f32, width: f32) {
        let pitch = self.line_pitch();
        for (i, line) in self.lines(width).iter().enumerate() {
            self.draw_line(line, x, baseline + i as f32 * pitch);
        }
    }

    /// Draws the text as one line (no wrapping), its baseline at `baseline`.
    pub fn draw_one(&self, x: f32, baseline: f32) {
        self.draw_line(self.s, x, baseline);
    }

    /// Draws one line at `(x, y)`: the desktop as the hand-written code did (`design * k`, no
    /// legacy metrics), the phone with the legacy metrics at [`Text::size`].
    fn draw_line(&self, s: &str, x: f32, y: f32) {
        let phone = display::dp_px().is_some();
        let size = if phone { self.size() } else { self.design * chrome::k() };
        self.in_face(|| {
            dt_font::legacy_metrics_if(phone, || {
                if self.shadow {
                    chrome::shadow_text(s, x, y, size, self.color);
                } else {
                    widgets::text(s, x, y, size, self.color);
                }
            })
        });
    }
}

// ------------------------------------------------------------------------------------------
// Flow: a measured column that scrolls
// ------------------------------------------------------------------------------------------

/// Scroll state of one flow, kept per id between frames.
#[derive(Clone, Copy, Default)]
struct Scroll {
    /// The offset in px (0 when the column does not overflow).
    offset: f32,
    /// Last frame's content height in px.
    content: f32,
}

local! {
    /// The scroll state of every flow, by its id.
    static SCROLL: RefCell<HashMap<&'static str, Scroll>> = RefCell::new(HashMap::new());
}

/// Px a wheel step or a row of a swipe scrolls (40 design px).
const STEP_DESIGN: f32 = 40.0;

/// The width of the flow's scrollbar at the right edge, in design px.
const BAR_DESIGN: f32 = 10.0;

/// An immediate-mode column: a cursor that stacks rows from the top of `area`. When its content
/// runs past `area` (last frame's height), the column scrolls and is clipped to `area`.
pub struct Flow {
    area: Rect,
    id: &'static str,
    y: f32,
    offset: f32,
    overflow: bool,
    clip: Option<widgets::Clip>,
}

impl Flow {
    /// Starts a flow in `area`, its scroll state kept under `id`. Input (wheel, swipe) is read
    /// here; call [`Flow::finish`] when the rows are drawn.
    pub fn begin(area: Rect, id: &'static str) -> Flow {
        let prev = SCROLL.with(|s| s.borrow().get(id).copied().unwrap_or_default());
        let overflow = prev.content > area.h + 0.5;
        let mut offset = 0.0;
        let mut clip = None;
        if overflow {
            let k = chrome::k();
            let most = (prev.content - area.h).max(0.0);
            widgets::swipe_rows(area, STEP_DESIGN * k);
            let mut off = prev.offset;
            let (mx, my) = widgets::pointer();
            if area.contains(vec2(mx, my)) {
                off -= widgets::wheel() * STEP_DESIGN * k;
            }
            offset = off.clamp(0.0, most);
            clip = Some(widgets::Clip::new(area));
        }
        Flow { area, id, y: area.y - offset, offset, overflow, clip }
    }

    /// The cursor: the top of the next row, in screen px.
    pub fn cursor(&self) -> f32 {
        self.y
    }

    /// The area the flow fills.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// The width of the area.
    pub fn width(&self) -> f32 {
        self.area.w
    }

    /// Moves the cursor down `design * k`.
    pub fn gap(&mut self, design: f32) {
        self.y += design * chrome::k();
    }

    /// Draws `text` at `x` (screen px) and `width` px wide, its first baseline `baseline_offset`
    /// px below the cursor. Advances the cursor by the larger of `design_h * k` and the text's
    /// own height from that baseline. Returns the rectangle the text took.
    pub fn text_at(&mut self, text: &Text, x: f32, baseline_offset: f32, width: f32, design_h: f32) -> Rect {
        let top = self.y;
        let lines = text.lines(width).len() as f32;
        let pitch = text.line_pitch();
        let own = baseline_offset + (lines - 1.0).max(0.0) * pitch + pitch * 0.4;
        let h = (design_h * chrome::k()).max(own);
        text.draw(x, top + baseline_offset, width);
        self.y += h;
        Rect::new(x, top, width, h)
    }

    /// A row `design_h * k` tall at the cursor, its rectangle passed to `f`.
    pub fn row(&mut self, design_h: f32, f: impl FnOnce(Rect)) -> Rect {
        self.row_h(design_h * chrome::k(), f)
    }

    /// A row `h` screen px tall at the cursor, its rectangle passed to `f`.
    pub fn row_h(&mut self, h: f32, f: impl FnOnce(Rect)) -> Rect {
        let r = Rect::new(self.area.x, self.y, self.area.w, h);
        f(r);
        self.y += h;
        r
    }

    /// Whether `r` overlaps the area.
    pub fn visible(&self, r: Rect) -> bool {
        r.x < self.area.x + self.area.w && r.x + r.w > self.area.x && r.y < self.area.y + self.area.h && r.y + r.h > self.area.y
    }

    /// The part of `r` inside the area, `None` when none of it is.
    pub fn hit(&self, r: Rect) -> Option<Rect> {
        let x0 = r.x.max(self.area.x);
        let y0 = r.y.max(self.area.y);
        let x1 = (r.x + r.w).min(self.area.x + self.area.w);
        let y1 = (r.y + r.h).min(self.area.y + self.area.h);
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    /// Ends the flow: stores the content's height for the next frame, draws the scrollbar when
    /// the column overflowed, keeps the offset and drops the clip.
    pub fn finish(mut self) {
        let content = (self.y + self.offset) - self.area.y;
        if self.overflow {
            let k = chrome::k();
            let most = (content - self.area.h).max(0.0);
            let bar = Rect::new(self.area.x + self.area.w - BAR_DESIGN * k, self.area.y, BAR_DESIGN * k, self.area.h);
            widgets::scrollbar_px(bar, &mut self.offset, most, self.area.h, STEP_DESIGN * k);
        } else {
            self.offset = 0.0;
        }
        self.clip = None;
        let stored = Scroll { offset: self.offset, content };
        SCROLL.with(|s| s.borrow_mut().insert(self.id, stored));
    }
}

// ------------------------------------------------------------------------------------------
// Buttons
// ------------------------------------------------------------------------------------------

/// A marble button (the settings' pattern: `widgets::button_state`, `chrome::marble_button`, the
/// button cue). True when clicked this frame.
#[track_caller]
pub fn button(r: Rect, label: &str, enabled: bool) -> bool {
    let (_, down, pressed) = widgets::button_state(r, enabled);
    chrome::marble_button(r, label, enabled, down);
    if pressed {
        audio::cue(audio::Cue::Button);
    }
    pressed
}

/// [`button`] inside a flow: when `r` is scrolled out of the flow's area it is drawn but cannot
/// be pressed.
#[track_caller]
pub fn button_in(flow: &Flow, r: Rect, label: &str, enabled: bool) -> bool {
    if flow.hit(r).is_none() {
        chrome::marble_button(r, label, enabled, false);
        return false;
    }
    button(r, label, enabled)
}

// ------------------------------------------------------------------------------------------
// Upright window
// ------------------------------------------------------------------------------------------

/// The design width an upright window is laid out at: its scale is `width / UPRIGHT_W`.
pub const UPRIGHT_W: f32 = 360.0;

/// An upright window `margin` px in from each side of a `screen` (w, h), from `top` to
/// `bottom`. Returns its scale (`width / UPRIGHT_W`) and its rectangle. Pure twin of
/// [`upright_window`].
pub fn upright_window_at(screen: (f32, f32), top: f32, bottom: f32, margin: f32) -> (f32, Rect) {
    let width = screen.0 - 2.0 * margin;
    (width / UPRIGHT_W, Rect::new(margin, top, width, bottom - top))
}

/// The upright window's scale and rectangle for the current phone: `None` unless the phone is
/// held upright. It starts under the top safe inset and ends at the title's foot, or above the
/// bottom bar when it is over the map (`over_map`).
pub fn upright_window(over_map: bool) -> Option<(f32, Rect)> {
    if !upright() {
        return None;
    }
    let safe = display::safe_area();
    let top = safe.top + 8.0;
    let bottom = if over_map {
        screen_height() - chrome::bar_height() - 8.0
    } else {
        window_layout::title_bottom()
    };
    let margin = 8.0 + safe.left.max(safe.right);
    Some(upright_window_at((screen_width(), screen_height()), top, bottom, margin))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn min_cap_is_none_on_desktop() {
        assert_eq!(min_cap_at(None, 1.25), None);
        assert_eq!(min_cap_at(None, 1.5), None);
    }

    #[test]
    fn min_cap_is_12_sp_at_the_default_text_size() {
        let c = min_cap_at(Some(2.75), 1.25).unwrap();
        assert!(close(c, 12.0 * 0.7 * 2.75), "{c}");
        assert!(close(min_cap_at(Some(2.75), 1.0).unwrap(), c), "a smaller text size keeps the floor");
    }

    #[test]
    fn min_cap_grows_with_a_larger_text_size() {
        let base = min_cap_at(Some(2.75), 1.25).unwrap();
        let big = min_cap_at(Some(2.75), 1.5).unwrap();
        assert!(close(big, base * 1.2), "{big} vs {base}");
    }

    #[test]
    fn floored_raises_a_small_size_only() {
        // Capital 11 px at size 10; the floor 23.1 px: the size goes to 10 * 23.1 / 11 = 21.
        let raised = floored_at(10.0, 11.0, Some(23.1));
        assert!(close(raised, 21.0), "{raised}");
        assert!(close(floored_at(30.0, 26.0, Some(23.1)), 30.0), "a big size is kept");
        assert!(close(floored_at(10.0, 11.0, None), 10.0), "no floor (desktop) keeps the size");
        assert!(close(floored_at(10.0, 0.0, Some(23.1)), 10.0), "no capital measured keeps the size");
    }

    #[test]
    fn text_px_at_scales_by_k_and_text_size_on_phones_only() {
        assert!(close(text_px_at(10.0, 2.0, None, 1.5), 20.0));
        assert!(close(text_px_at(10.0, 2.0, Some(2.75), 1.5), 30.0));
        assert!(close(text_px_at(10.0, 2.0, Some(2.75), 1.0), 20.0));
    }

    #[test]
    fn touch_h_is_design_on_desktop_and_at_least_44_dp_on_phones() {
        assert!(close(touch_h_at(20.0, 1.0, 1.0, None), 20.0));
        assert!(close(touch_h_at(20.0, 1.0, 1.2, None), 24.0));
        assert!(close(touch_h_at(20.0, 1.0, 1.0, Some(2.75)), 44.0 * 2.75));
        assert!(close(touch_h_at(100.0, 1.0, 1.0, Some(2.0)), 100.0), "a big target keeps its size");
    }

    #[test]
    fn upright_window_fills_the_width_less_the_margins() {
        let (k, r) = upright_window_at((1080.0, 1920.0), 40.0, 1500.0, 8.0);
        assert!(close(k, (1080.0 - 16.0) / 360.0), "{k}");
        assert!(close(r.x, 8.0) && close(r.y, 40.0) && close(r.w, 1064.0) && close(r.h, 1460.0), "{r:?}");
    }
}
