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
    // `measure_exact` applies the phone floor itself, so the capital is measured without it.
    dt_font::legacy_metrics_if(true, || {
        widgets::without_floor(|| widgets::raise_to_cap(size, min_cap, |s| widgets::measure_exact("Н", s).offset_y))
    })
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

    /// The width of the text as one line, in px (the size and metrics `draw_one` uses).
    pub fn width(&self) -> f32 {
        let phone = display::dp_px().is_some();
        self.in_face(|| dt_font::legacy_metrics_if(phone, || widgets::measure(self.s, self.size()).width))
    }

    /// Draws the text as one line with its right end at `x_right`, its baseline at `baseline`.
    pub fn draw_right_one(&self, x_right: f32, baseline: f32) {
        self.draw_one(x_right - self.width(), baseline);
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

    /// Whether `r` runs past the area at its top or bottom (it is partly scrolled out; half a px
    /// of rounding is not).
    pub fn cut(&self, r: Rect) -> bool {
        r.y < self.area.y - 0.5 || r.y + r.h > self.area.y + self.area.h + 0.5
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
    // A control cut by the flow's edge is drawn but not pressable until it is whole.
    if flow.cut(r) {
        chrome::marble_button(r, label, enabled, false);
        return false;
    }
    button(r, label, enabled)
}

// ------------------------------------------------------------------------------------------
// Phone windows
// ------------------------------------------------------------------------------------------

/// A phone window (Razdor's own, the owner's decision, 2026-11-01): one design px per dp on every
/// phone, held upright or sideways. `design` is its size in design px; it is as wide as the design
/// or the screen allows (`screen` less `side` on each side), and as tall as the design grown by `g`
/// (the button growth) or the room between `top` and `bottom`, the whole room when `fill`. Centred
/// horizontally and vertically in `top`..`bottom`. Returns its scale (`dp`) and its rectangle.
/// Pure twin of [`phone_window`].
#[allow(clippy::too_many_arguments)] // the room (`top`, `bottom`, `side`) and the design, as they are read
pub fn phone_window_at(screen: (f32, f32), dp: f32, top: f32, bottom: f32, side: f32, design: (f32, f32), fill: bool, g: f32) -> (f32, Rect) {
    let k = dp;
    let avail_w = (screen.0 - 2.0 * side).max(0.0);
    let avail_h = (bottom - top).max(0.0);
    let w = (design.0 * k).min(avail_w);
    let h = if fill { avail_h } else { (design.1 * k * g).min(avail_h) };
    let x = (screen.0 - w) / 2.0;
    let y = top + (avail_h - h) / 2.0;
    (k, Rect::new(x.round(), y.round(), w.round(), h.round()))
}

/// A phone window's scale and rectangle for the phone on screen, `None` on the desktop. Its room
/// runs from the top inset (8 px down) to the title's foot when held upright, to the bottom inset
/// sideways, or to the bar's top over the map (`over_map`). See [`phone_window_at`].
pub fn phone_window(over_map: bool, design: (f32, f32), fill: bool) -> Option<(f32, Rect)> {
    let dp = phone()?;
    let safe = display::safe_area();
    let top = safe.top + 8.0;
    let side = 8.0 + safe.left.max(safe.right);
    let bottom = if over_map {
        screen_height() - chrome::bar_height() - 8.0
    } else if upright() {
        window_layout::title_bottom()
    } else {
        screen_height() - safe.bottom - 8.0
    };
    Some(phone_window_at((screen_width(), screen_height()), dp, top, bottom, side, design, fill, display::button_grow()))
}

// ------------------------------------------------------------------------------------------
// Phone rows: a checkbox and a setting
// ------------------------------------------------------------------------------------------

/// A phone checkbox row `r`: the box `box_design` px square (× k) at the row's left, vertically
/// centred, the label wrapped beside it in the Subtitle face. The whole row is the hit area. True
/// when it is clicked (the button cue plays).
#[track_caller]
pub fn check(r: Rect, label: &str, on: bool, ink: Color, box_design: f32) -> bool {
    check_at(r, true, label, on, ink, box_design)
}

/// [`check`] inside a flow: a row cut by the flow's edge is drawn but cannot be ticked until it is
/// whole (its hit area is never half a row).
#[track_caller]
pub fn check_in(flow: &Flow, r: Rect, label: &str, on: bool, ink: Color, box_design: f32) -> bool {
    check_at(r, !flow.cut(r), label, on, ink, box_design)
}

/// Draws the checkbox in `r`; `live`: whether it takes presses (not measured or pressed otherwise).
fn check_at(r: Rect, live: bool, label: &str, on: bool, ink: Color, box_design: f32) -> bool {
    let k = chrome::k();
    let bs = box_design * k;
    let b = Rect::new(r.x, r.y + (r.h - bs) / 2.0, bs, bs);
    let (art, mask) = if on { ("CBoxChecked", "CBoxChecked-Alpha") } else { ("CBoxUn", "CBoxUn-Alpha") };
    match chrome::win_masked(art, mask) {
        Some(t) => chrome::tex(&t, b, WHITE),
        None => {
            draw_rectangle(b.x, b.y, b.w, b.h, Color::new(0.0, 0.0, 0.0, 0.5));
            chrome::silver_frame(b, 1.0);
            if on {
                // The fallback tick of `main_menu::option_check`, drawn for a 26 px box.
                let s = bs / 26.0;
                draw_line(b.x + 3.0 * s, b.y + 9.0 * s, b.x + 8.0 * s, b.y + 15.0 * s, 2.5 * s, GREEN);
                draw_line(b.x + 8.0 * s, b.y + 15.0 * s, b.x + 16.0 * s, b.y + 2.0 * s, 2.5 * s, GREEN);
            }
        }
    }
    let t = Text::new(label, 15.0, ink).face(dt_font::Face::Subtitle);
    let lw = (r.w - bs - 8.0 * k).max(1.0);
    let pitch = t.line_pitch();
    let block = t.lines(lw).len() as f32 * pitch;
    t.draw(r.x + bs + 8.0 * k, r.y + (r.h - block) / 2.0 + pitch / 2.0 + t.size() * 0.35, lw);
    let pressed = live && widgets::button_state(r, true).2;
    if pressed {
        audio::cue(audio::Cue::Button);
    }
    pressed
}

/// A phone target at least [`widgets::FINGER_DP`] dp (48) on its shorter side, so that the rows
/// next to it are not crowded (the touch probe's rule). `h` is at least `h`.
pub fn finger_h(h: f32) -> f32 {
    match phone() {
        Some(d) => h.max(widgets::FINGER_DP * d),
        None => h,
    }
}

/// The height of a phone checkbox row `width` px wide for `label` ([`check`]): the touch height
/// (26 design px grown, at least [`finger_h`]), or the label's wrapped lines plus 8 px, whichever is
/// taller.
pub fn check_h(label: &str, width: f32, box_design: f32) -> f32 {
    let k = chrome::k();
    let lw = (width - box_design * k - 8.0 * k).max(1.0);
    let text_h = Text::new(label, 15.0, WHITE).face(dt_font::Face::Subtitle).height(lw);
    finger_h(touch_h(26.0)).max(text_h + 8.0 * k)
}

/// A setting row on a phone, in `flow`: `label` (14 px, cream) on the left and the button `value` on
/// the right, `button_design_w` px wide (at most half the flow) and [`touch_h`] 28 high (at least
/// [`finger_h`]). When the value does not fit beside the label at its own size, the label goes on
/// its own line and the button takes the full width under it: a phone's value is never shrunk to
/// an ellipsis. The label's lines are centred on the button beside it; the row is as tall as its
/// content, plus 6 px. Returns whether the button was clicked, and its rectangle.
#[track_caller]
pub fn setting(flow: &mut Flow, label: &str, value: &str, button_design_w: f32) -> (bool, Rect) {
    let k = chrome::k();
    let (w, bh) = (flow.width(), finger_h(touch_h(28.0)));
    let side_w = (button_design_w * k).min(w * 0.5);
    let beside = chrome::button_label_width(bh, value) <= side_w - 12.0 * k;
    let (bw, lw) = if beside { (side_w, (w - side_w - 12.0 * k).max(1.0)) } else { (w, w) };
    let t = Text::new(label, 14.0, chrome::CREAM);
    let (pitch, size) = (t.line_pitch(), t.size());
    let block = t.lines(lw).len() as f32 * pitch;
    let content = if beside { bh.max(block) } else { block + 4.0 * k + bh };
    let row = flow.row_h(content + 6.0 * k, |_| {});
    let (label_y, br) = if beside {
        let centre = row.y + content / 2.0;
        (centre - block / 2.0 + pitch / 2.0 + size * 0.35, Rect::new(row.x + row.w - bw, centre - bh / 2.0, bw, bh))
    } else {
        (row.y + pitch / 2.0 + size * 0.35, Rect::new(row.x, row.y + block + 4.0 * k, bw, bh))
    };
    t.draw(row.x, label_y, lw);
    (button_in(flow, br, value, true), br)
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
    fn phone_window_is_one_px_per_dp_and_as_wide_as_the_screen_allows_upright() {
        // 594 design px at 2.75 px per dp is wider than a 1080 px screen less its margins: the screen decides.
        let (k, r) = phone_window_at((1080.0, 2400.0), 2.75, 40.0, 2000.0, 16.0, (594.0, 500.0), true, 1.0);
        assert!(close(k, 2.75), "{k}");
        assert!(close(r.w, 1080.0 - 32.0) && close(r.x, 16.0), "{r:?}");
        assert!(close(r.y, 40.0) && close(r.h, 1960.0), "fills the room from the top to the bottom: {r:?}");
    }

    #[test]
    fn phone_window_takes_its_design_width_sideways() {
        let (k, r) = phone_window_at((2400.0, 1080.0), 2.75, 8.0, 1072.0, 20.0, (594.0, 650.0), true, 1.0);
        assert!(close(k, 2.75));
        assert!(close(r.w, (594.0 * 2.75f32).round()), "the design width, rounded: {r:?}");
        assert!(close(r.x, ((2400.0 - r.w) / 2.0).round()), "centred: {r:?}");
    }

    #[test]
    fn phone_window_grows_its_design_height_with_the_button_growth_and_is_centred() {
        // 300 design px tall at 2.75 px per dp, grown 1.2×: 990 px in a 1064 px room, centred in it.
        let (_, r) = phone_window_at((2400.0, 1080.0), 2.75, 8.0, 1072.0, 20.0, (594.0, 300.0), false, 1.2);
        assert!(close(r.h, 990.0), "{r:?}");
        assert!(close(r.y, 45.0), "8 + (1064 - 990) / 2: {r:?}");
    }
}
