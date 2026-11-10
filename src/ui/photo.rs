//! Photo mode (Razdor's, the player's request of 2026-11-10): from the map's game menu («Фото») or
//! the photo key (P), the game stands still and the map is seen from a free camera (pitch 8° to
//! top-down, any turn, a wider zoom), with every piece of UI hidden but a small control strip.
//! The shutter saves the frame, without the strip, as a PNG in the app's `photos` folder.
//!
//! Visual only: no rule, save or game state changes. The world frame (`world_view::photo_frame`)
//! runs no tick, no event and no AI move; a press does not finish a paused AI step (`may_tick`),
//! and leaving the mode gives the view back as it was (`leave`).

use std::cell::{Cell, RefCell};

use macroquad::prelude::*;
use razdor::i18n::{n_, tr};
use razdor::local;

use super::view25;
use super::widgets;

/// The camera's pitch range in degrees: a low view (`PITCH_MIN`) to top-down (`PITCH_TOP`, the
/// camera's own limit: at exactly 90 `look_at` is degenerate, so 89.9 is the top-down view).
pub const PITCH_MIN: f32 = 8.0;
pub const PITCH_TOP: f32 = view25::FREE_PITCH_MAX;
/// The zoom in notches of 1.1 (the wheel's): the zoom bounds are the play's whole-map floor times
/// `ZOOM_OUT` at the bottom, and the wheel's top times `ZOOM_IN` at the top.
const ZOOM_STEP: f32 = 1.1;
const ZOOM_OUT: f32 = 0.5;
const ZOOM_IN: f32 = 2.0;
/// Degrees of yaw and pitch per pixel a mouse or a finger moves in a turn (right button held, or
/// the two-finger twist's own angle is used as it is).
pub const DRAG_YAW: f32 = 0.4;
pub const DRAG_PITCH: f32 = 0.25;
/// Held keys: degrees a second of turn and tilt, screen pixels a second of pan.
pub const KEY_YAW: f32 = 90.0;
pub const KEY_PITCH: f32 = 40.0;
pub const KEY_PAN: f32 = 600.0;
/// Pixels the mouse moves with a button held before the press is a drag (as the map's own).
pub const DRAG_START: f32 = 4.0;

/// The mode's state while it is on.
#[derive(Clone, Debug, PartialEq)]
pub struct Photo {
    /// The camera's pitch and yaw in degrees, the zoom as the map's (`MapView::zoom`).
    pub pitch: f32,
    pub yaw: f32,
    pub zoom: f32,
    /// The control strip is shown (not hidden for a clean shot).
    pub strip: bool,
    /// A shutter press is pending: the next frame's picture is saved.
    pub shot: bool,
    /// The map's look (`MapView::look`) before the mode, given back on leaving.
    pub saved_look: Option<(f32, f32)>,
    /// A mouse or finger drag in progress.
    pub drag: Option<Drag>,
    /// The strip's buttons held down this frame (shutter, hide, exit).
    pub down: [bool; 3],
}

impl Photo {
    /// The mode's state for a camera at `pitch` and `yaw` (degrees, clamped and wrapped), zoom
    /// `zoom`, with the map's look `saved_look` to give back.
    pub fn new(pitch: f32, yaw: f32, zoom: f32, saved_look: Option<(f32, f32)>) -> Photo {
        Photo { pitch: clamp_pitch(pitch), yaw: wrap_yaw(yaw), zoom, strip: true, shot: false, saved_look, drag: None, down: [false; 3] }
    }
}

/// Which button a drag was started with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// Pans the map with the pointer.
    Left,
    /// Turns (horizontal) and tilts (vertical) the camera.
    Right,
}

/// A drag in progress: the button, where it started, where the pointer was last, and whether it
/// has moved past [`DRAG_START`] (a press and a release without moving is a tap).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub button: Button,
    pub start: (f32, f32),
    pub last: (f32, f32),
    pub moved: bool,
}

/// What a drag's frame asks of the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragOut {
    None,
    /// The pointer's move in screen pixels: the map follows it.
    Pan((f32, f32)),
    /// The pointer's move in screen pixels: yaw and pitch change by it.
    Turn((f32, f32)),
}

impl Drag {
    pub fn start(button: Button, at: (f32, f32)) -> Drag {
        Drag { button, start: at, last: at, moved: false }
    }

    /// One frame of the drag with the pointer at `at` and its button `down`.
    pub fn step(&mut self, at: (f32, f32), down: bool) -> DragOut {
        if !down {
            return DragOut::None;
        }
        let delta = (at.0 - self.last.0, at.1 - self.last.1);
        self.last = at;
        if !self.moved && (at.0 - self.start.0).hypot(at.1 - self.start.1) <= DRAG_START {
            return DragOut::None;
        }
        self.moved = true;
        match self.button {
            Button::Left => DragOut::Pan(delta),
            Button::Right => DragOut::Turn(delta),
        }
    }
}

local! {
    /// The mode's state, `None` while the map is played as before.
    static MODE: RefCell<Option<Photo>> = RefCell::new(None);
    /// Photo mode is asked for (the game menu's «Фото»); the map frame turns it on.
    static REQUEST: Cell<bool> = Cell::new(false);
}

/// Whether photo mode is on.
pub fn active() -> bool {
    MODE.with(|m| m.borrow().is_some())
}

/// Whether the game may advance this frame: never in photo mode (the clock, the walk, the waits,
/// the AI and the events all stand still). The world frame and the press that finishes a paused
/// AI step (`App::frame_content`) both ask it.
pub fn may_tick() -> bool {
    !active()
}

/// Asks for photo mode (the game menu's «Фото»): the next map frame turns it on.
pub fn request() {
    REQUEST.with(|r| r.set(true));
}

/// Takes the request, if one is made.
pub fn take_request() -> bool {
    REQUEST.with(|r| r.replace(false))
}

/// Turns the mode on with `p`.
pub fn enter(p: Photo) {
    MODE.with(|m| *m.borrow_mut() = Some(p));
}

/// Turns the mode off, returning its state (its `saved_look` is the map's to give back).
pub fn leave() -> Option<Photo> {
    MODE.with(|m| m.borrow_mut().take())
}

/// The mode's state, changed by `f`, when the mode is on.
pub fn with<R>(f: impl FnOnce(&mut Photo) -> R) -> Option<R> {
    MODE.with(|m| m.borrow_mut().as_mut().map(f))
}

/// The mode's camera (pitch, yaw) in degrees, when the mode is on.
pub fn angles() -> Option<(f32, f32)> {
    with(|p| (p.pitch, p.yaw))
}

/// Clamps a pitch to the photo camera's range (NaN is the low end, a camera never gets it).
pub fn clamp_pitch(deg: f32) -> f32 {
    if deg.is_nan() { PITCH_MIN } else { deg.clamp(PITCH_MIN, PITCH_TOP) }
}

/// Wraps a yaw to (-180, 180].
pub fn wrap_yaw(deg: f32) -> f32 {
    if !deg.is_finite() {
        return 0.0;
    }
    let w = deg.rem_euclid(360.0);
    if w > 180.0 { w - 360.0 } else { w }
}

/// The zoom's bounds for a play floor `play_floor` (the whole map's zoom in play): further out
/// than the play's floor and further in than its top (`world_view::ZOOM_WHEEL_MAX`).
pub fn zoom_bounds(play_floor: f32) -> (f32, f32) {
    ((play_floor * ZOOM_OUT).max(0.01), super::world_view::ZOOM_WHEEL_MAX * ZOOM_IN)
}

/// The zoom `notches` wheel steps from `zoom` (positive zooms in), within `bounds`.
pub fn zoom_by(zoom: f32, notches: f32, bounds: (f32, f32)) -> f32 {
    if !zoom.is_finite() {
        return 1.0f32.clamp(bounds.0, bounds.1);
    }
    (zoom * ZOOM_STEP.powf(notches)).clamp(bounds.0, bounds.1)
}

/// The control strip's rectangles on a screen of `screen` pixels: the shutter («Снимок»), the
/// hide button («Скрыть») and the exit («Выход»), in a panel at the bottom left on the desktop
/// (the interface scale `k`), and a row across the bottom on a phone (`dp`: its pixels per dp,
/// buttons 48 dp high), above the safe area's bottom inset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub panel: Rect,
    pub buttons: [Rect; 3],
}

pub fn layout(screen: (f32, f32), dp: Option<f32>, k: f32, bottom_inset: f32) -> Layout {
    let (w, h) = screen;
    match dp {
        None => {
            let (bh, gap, pad, margin) = (30.0 * k, 6.0 * k, 6.0 * k, 10.0 * k);
            let widths = [104.0 * k, 96.0 * k, 96.0 * k];
            let total: f32 = widths.iter().sum::<f32>() + gap * 2.0;
            let y = h - bottom_inset - margin - bh;
            let mut x = margin + pad;
            let mut buttons = [Rect::default(); 3];
            for (b, bw) in buttons.iter_mut().zip(widths) {
                *b = Rect::new(x, y, bw, bh);
                x += bw + gap;
            }
            Layout { panel: Rect::new(margin, y - pad, total + 2.0 * pad, bh + 2.0 * pad), buttons }
        }
        Some(d) => {
            let (bh, gap, pad, margin) = (48.0 * d, 6.0 * d, 4.0 * d, 10.0 * d);
            let bw = ((w - 2.0 * margin - 2.0 * gap - 2.0 * pad) / 3.0).max(0.0);
            let y = h - bottom_inset - margin - bh;
            let mut x = margin + pad;
            let mut buttons = [Rect::default(); 3];
            for b in buttons.iter_mut() {
                *b = Rect::new(x, y, bw, bh);
                x += bw + gap;
            }
            Layout { panel: Rect::new(margin, y - pad, w - 2.0 * margin, bh + 2.0 * pad), buttons }
        }
    }
}

/// The strip's button labels, in the game's language (Russian, the catalog's; English is the key).
pub fn labels() -> [String; 3] {
    [tr(n_("Shot")).to_string(), tr(n_("Hide the strip")).to_string(), tr(n_("Exit photo mode")).to_string()]
}

/// The strip's button presses this frame, as `(pressed, held)` for [`Layout::buttons`] (the
/// shutter, hide, exit). Registers each button with the touch hit areas as the others do.
pub fn strip_input(l: &Layout) -> ([bool; 3], [bool; 3]) {
    let mut pressed = [false; 3];
    let mut held = [false; 3];
    for (i, r) in l.buttons.into_iter().enumerate() {
        let (_, down, press) = widgets::button_state(r, true);
        held[i] = down;
        pressed[i] = press;
    }
    (pressed, held)
}

/// Draws the strip: its panel and the three buttons, `held` ones pressed in. On a phone (`dp`: its
/// pixels per dp) the labels are 16 sp, as the phone texts (the desktop's labels fit their buttons).
pub fn draw_strip(l: &Layout, held: [bool; 3], dp: Option<f32>) {
    let panel = Color { a: 0.55, ..super::widgets::PANEL };
    draw_rectangle(l.panel.x, l.panel.y, l.panel.w, l.panel.h, panel);
    let names = labels();
    for ((r, name), down) in l.buttons.into_iter().zip(names.iter()).zip(held) {
        match dp {
            Some(d) => super::chrome::marble_button_sized(r, name, true, down, 16.0 * d),
            None => super::chrome::marble_button(r, name, true, down),
        }
    }
}

/// Whether a point is on the strip's panel.
pub fn on_strip(l: &Layout, at: (f32, f32)) -> bool {
    l.panel.contains(vec2(at.0, at.1))
}

/// The date and time a shot is named by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// The stamp of Unix time `secs` shifted by `offset` seconds (the local zone's, 0 for UTC).
pub fn stamp_of(secs: i64, offset: i64) -> Stamp {
    let t = secs + offset;
    let days = t.div_euclid(86_400);
    let rem = t.rem_euclid(86_400);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    Stamp { year, month, day, hour: (rem / 3600) as u32, minute: (rem % 3600 / 60) as u32, second: (rem % 60) as u32 }
}

/// The offset of the local zone from UTC at Unix time `secs`, in seconds (0 where it is not read).
#[cfg(unix)]
// The field is an i32 on some targets and an i64 on others: the cast is for the first.
#[allow(clippy::unnecessary_cast)]
fn local_offset(secs: i64) -> i64 {
    // SAFETY: `localtime_r` writes the zeroed struct it is given and reads the time by value.
    unsafe {
        let t = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff as i64
    }
}

#[cfg(not(unix))]
fn local_offset(_secs: i64) -> i64 {
    0
}

/// The local date and time now.
pub fn now_stamp() -> Stamp {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    stamp_of(secs, local_offset(secs))
}

/// The file name of a shot taken at `s`: `razdor-2026-11-10-18-05-12.png`; the `n`th one of that
/// second (`n` above 0) gets `-2`, `-3`, … before the extension.
pub fn file_name(s: &Stamp, n: u32) -> String {
    let copy = if n == 0 { String::new() } else { format!("-{}", n + 1) };
    format!("razdor-{:04}-{:02}-{:02}-{:02}-{:02}-{:02}{copy}.png", s.year, s.month, s.day, s.hour, s.minute, s.second)
}

/// The screen read's rows as the picture's: the read comes bottom row first, so the rows go in
/// reverse, and the alpha is opaque (as the trailer's frames are). `bytes` is RGBA, `width` pixels
/// a row, `height` rows.
pub fn upright(bytes: &[u8], width: usize, height: usize) -> Vec<u8> {
    let stride = width * 4;
    if stride == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(stride * height);
    for row in bytes.chunks_exact(stride).take(height).rev() {
        out.extend_from_slice(row);
    }
    for px in out.chunks_exact_mut(4) {
        px[3] = 255;
    }
    out
}

/// The RGBA rows as a PNG file's bytes.
pub fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// The folder the shots go to: `photos` in the app's folder (`platform::app_dir`).
pub fn folder() -> Option<std::path::PathBuf> {
    razdor::platform::app_dir().map(|d| d.join("photos"))
}

/// Saves the screen read `bytes` (`width` by `height`, RGBA as the screen read gives them) as a
/// PNG in [`folder`], under a name no file has yet. Returns the path written.
pub fn save_shot(bytes: &[u8], width: u32, height: u32) -> Result<std::path::PathBuf, String> {
    let dir = folder().ok_or("no folder for the photos")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = now_stamp();
    let path = (0..).map(|n| dir.join(file_name(&stamp, n))).find(|p| !p.exists()).unwrap_or_else(|| dir.join(file_name(&stamp, 0)));
    let png = encode_png(&upright(bytes, width as usize, height as usize), width, height)?;
    std::fs::write(&path, png).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Snapshot runs (`ui::snapshot`): `RAZDOR_SCENE_PHOTO=1` asks for photo mode at the scene's
/// start. Read by the snapshot's stage only.
pub fn snapshot_request() {
    if std::env::var("RAZDOR_SCENE_PHOTO").is_ok_and(|v| v.trim() == "1") {
        request();
    }
}

/// Snapshot runs: the mode's start as the scene sets it: `RAZDOR_SCENE_PHOTO_PITCH` and
/// `RAZDOR_SCENE_PHOTO_YAW` (degrees), `RAZDOR_SCENE_PHOTO_ZOOM` (the zoom), `RAZDOR_SCENE_PHOTO_STRIP=0`
/// (the strip hidden), `RAZDOR_SCENE_PHOTO_SHOT=1` (a shot on the mode's first frame).
pub fn snapshot_start(p: &mut Photo) {
    let num = |name: &str| std::env::var(name).ok().and_then(|v| v.trim().parse::<f32>().ok());
    if let Some(v) = num("RAZDOR_SCENE_PHOTO_PITCH") {
        p.pitch = clamp_pitch(v);
    }
    if let Some(v) = num("RAZDOR_SCENE_PHOTO_YAW") {
        p.yaw = wrap_yaw(v);
    }
    if let Some(v) = num("RAZDOR_SCENE_PHOTO_ZOOM") {
        p.zoom = v;
    }
    if std::env::var("RAZDOR_SCENE_PHOTO_STRIP").is_ok_and(|v| v.trim() == "0") {
        p.strip = false;
    }
    if std::env::var("RAZDOR_SCENE_PHOTO_SHOT").is_ok_and(|v| v.trim() == "1") {
        p.shot = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Puts the mode and its request back (each test runs on its own thread, so this is a guard).
    fn fresh() {
        let _ = leave();
        let _ = take_request();
    }

    #[test]
    fn pitch_runs_from_low_to_top_down_and_clamps_outside() {
        assert_eq!(clamp_pitch(3.0), PITCH_MIN);
        assert_eq!(clamp_pitch(55.0), 55.0);
        assert_eq!(clamp_pitch(90.0), PITCH_TOP, "top-down is the camera's 89.9");
        assert_eq!(clamp_pitch(f32::NAN), PITCH_MIN);
    }

    #[test]
    fn yaw_wraps_to_half_a_turn_either_way() {
        assert_eq!(wrap_yaw(190.0), -170.0);
        assert_eq!(wrap_yaw(-190.0), 170.0);
        assert_eq!(wrap_yaw(180.0), 180.0);
        assert_eq!(wrap_yaw(f32::NAN), 0.0);
        assert_eq!(wrap_yaw(725.0), 5.0);
    }

    #[test]
    fn zoom_reaches_further_than_play_and_stays_in_bounds() {
        let (lo, hi) = zoom_bounds(0.4);
        assert!(lo < 0.4, "further out than the play's whole-map floor");
        assert!(hi > super::super::world_view::ZOOM_WHEEL_MAX, "closer than the wheel's top");
        assert!((zoom_by(1.0, 1.0, (lo, hi)) - 1.1).abs() < 1e-5);
        assert_eq!(zoom_by(hi, 5.0, (lo, hi)), hi);
        assert_eq!(zoom_by(lo, -50.0, (lo, hi)), lo);
    }

    #[test]
    fn a_drag_pans_past_its_slop_and_a_still_press_is_a_tap() {
        let mut d = Drag::start(Button::Left, (100.0, 100.0));
        assert_eq!(d.step((102.0, 100.0), true), DragOut::None, "under the slop: not a drag yet");
        assert!(!d.moved);
        assert_eq!(d.step((110.0, 100.0), true), DragOut::Pan((8.0, 0.0)), "the move since the last frame");
        assert!(d.moved);
        assert_eq!(d.step((110.0, 100.0), false), DragOut::None);
        let mut t = Drag::start(Button::Left, (5.0, 5.0));
        t.step((5.0, 5.0), true);
        assert!(!t.moved, "a press and a release without moving is a tap");
    }

    #[test]
    fn a_right_drag_turns_and_the_move_is_its_own() {
        let mut d = Drag::start(Button::Right, (0.0, 0.0));
        assert_eq!(d.step((20.0, -10.0), true), DragOut::Turn((20.0, -10.0)));
        assert_eq!(d.step((20.0, -4.0), true), DragOut::Turn((0.0, 6.0)));
    }

    #[test]
    fn the_strip_stays_on_screen_with_phone_targets_of_48_dp() {
        for (screen, dp) in [((1080.0, 2400.0), Some(2.625)), ((2400.0, 1080.0), Some(2.625)), ((1280.0, 800.0), None)] {
            let l = layout(screen, dp, 1.0, 0.0);
            for r in l.buttons {
                assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= screen.0 && r.y + r.h <= screen.1, "{r:?} on {screen:?}");
                if let Some(d) = dp {
                    assert!(r.h >= 48.0 * d - 0.01, "a phone target is 48 dp high: {r:?}");
                    assert!(r.w >= 40.0 * d, "and 40 dp wide at least: {r:?}");
                }
            }
            assert!(l.panel.contains(vec2(l.buttons[0].x + 1.0, l.buttons[0].y + 1.0)));
            for pair in l.buttons.windows(2) {
                assert!(pair[0].x + pair[0].w <= pair[1].x, "the buttons do not overlap");
            }
        }
    }

    #[test]
    fn the_stamp_is_the_calendar_date_of_the_second() {
        // 2026-11-10 18:05:12 UTC (python: datetime(2026,11,10,18,5,12, utc).timestamp()).
        let s = stamp_of(1_794_333_912, 0);
        assert_eq!(s, Stamp { year: 2026, month: 11, day: 10, hour: 18, minute: 5, second: 12 });
        assert_eq!(stamp_of(0, 0), Stamp { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 });
        assert_eq!(stamp_of(1_794_333_912, 3600).hour, 19, "the zone's offset shifts the clock");
        assert_eq!(stamp_of(951_782_400, 0), Stamp { year: 2000, month: 2, day: 29, hour: 0, minute: 0, second: 0 }, "a leap day");
    }

    #[test]
    fn a_shot_is_named_by_its_second_and_later_ones_get_a_copy_number() {
        let s = Stamp { year: 2026, month: 11, day: 10, hour: 18, minute: 5, second: 12 };
        assert_eq!(file_name(&s, 0), "razdor-2026-11-10-18-05-12.png");
        assert_eq!(file_name(&s, 1), "razdor-2026-11-10-18-05-12-2.png");
    }

    #[test]
    fn the_screen_read_comes_out_upright_and_opaque() {
        // Two rows of two pixels, bottom row first as the screen read gives them.
        let read = [9, 9, 9, 0, 8, 8, 8, 0, 1, 2, 3, 0, 4, 5, 6, 0];
        let up = upright(&read, 2, 2);
        assert_eq!(up, [1, 2, 3, 255, 4, 5, 6, 255, 9, 9, 9, 255, 8, 8, 8, 255]);
    }

    #[test]
    fn the_png_decodes_to_the_upright_rows() {
        let read: Vec<u8> = (0..3 * 2 * 4).map(|i| (i * 7) as u8).collect();
        let up = upright(&read, 3, 2);
        let bytes = encode_png(&up, 3, 2).expect("encodes");
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().expect("a png");
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).expect("a frame");
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(&buf[..info.buffer_size()], &up[..]);
    }

    #[test]
    fn leaving_gives_back_the_look_and_the_request_is_taken_once() {
        fresh();
        assert!(!active());
        request();
        assert!(take_request());
        assert!(!take_request(), "a request is taken once");
        enter(Photo::new(20.0, 400.0, 1.5, Some((12.0, 34.0))));
        assert!(active());
        assert!(!may_tick(), "photo mode stops the game's clock");
        assert_eq!(angles(), Some((20.0, 40.0)));
        with(|p| p.yaw = 90.0);
        let back = leave().expect("it was on");
        assert_eq!(back.saved_look, Some((12.0, 34.0)));
        assert!(!active());
        assert!(may_tick(), "the game runs again once the mode is off");
        assert_eq!(angles(), None);
    }

    #[test]
    fn a_pitch_and_yaw_given_at_entry_are_clamped_and_wrapped() {
        let p = Photo::new(2.0, -200.0, 1.0, None);
        assert_eq!((p.pitch, p.yaw), (PITCH_MIN, 160.0));
    }
}
