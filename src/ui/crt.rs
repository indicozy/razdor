//! The CRT effect («ЭЛТ-эффект», `Settings::crt`): an old monitor's look over the whole screen.
//!
//! While it is on, the frame is drawn into a target the size of the window (physical pixels), which
//! stands in for the screen (`render_cache::set_screen_target`); [`end_frame`] then copies it to the
//! real screen. Everything after [`begin_frame`] (the map, the windows, the text, the pointer) reaches
//! the screen through that copy, so the look covers the whole frame. Look only: no rule changes.
//! Off by default; a snapshot or a photo reads the stand-in (`target_image`), so no effect is on them
//! where the picture is read before the screen (`map_freeze` stays off while it is on).

use macroquad::camera::set_default_camera;
use macroquad::color::BLACK;
use macroquad::prelude::clear_background;
use macroquad::texture::{render_target, FilterMode, Image, RenderTarget};
use macroquad::window::get_internal_gl;
use serde::{Deserialize, Serialize};

use super::render_cache;

/// The largest side in physical pixels the effect runs at; a larger window is drawn without it.
const MAX_SIDE: f32 = 8192.0;

/// «ЭЛТ-эффект» (`Settings::crt`): how strong the look is. «Выкл» is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strength {
    #[default]
    Off,
    Light,
    Normal,
    Strong,
}

/// «Качество ЭЛТ» (`Settings::crt_quality`): the cost of the look. The platform default is
/// [`platform_default`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Fast,
    Normal,
    High,
}

/// The quality when the setting is «none»: `Fast` on a phone, `Normal` on a desktop.
#[allow(dead_code)] // the shader's pass reads it (the next change)
pub fn platform_default() -> Quality {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        Quality::Fast
    } else {
        Quality::Normal
    }
}

/// The values of a profiling switch (`RAZDOR_CRT*`), desktop only, read once. The shader reads
/// the quality, the curve and the passthrough (the next change), so they are unused until then.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Switch {
    strength: Option<Strength>,
    quality: Option<Quality>,
    curve: Option<bool>,
    passthrough: bool,
}

/// `off`, `light`, `normal` or `strong` (any case); anything else is no value.
pub fn parse_strength(s: &str) -> Option<Strength> {
    match s.trim().to_ascii_lowercase().as_str() {
        "off" => Some(Strength::Off),
        "light" => Some(Strength::Light),
        "normal" => Some(Strength::Normal),
        "strong" => Some(Strength::Strong),
        _ => None,
    }
}

/// `fast`, `normal` or `high` (any case); anything else is no value.
#[allow(dead_code)]
pub fn parse_quality(s: &str) -> Option<Quality> {
    match s.trim().to_ascii_lowercase().as_str() {
        "fast" => Some(Quality::Fast),
        "normal" => Some(Quality::Normal),
        "high" => Some(Quality::High),
        _ => None,
    }
}

/// `1` (the curve on) or `0` (off); anything else is no value.
#[allow(dead_code)]
pub fn parse_curve(s: &str) -> Option<bool> {
    match s.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// `RAZDOR_CRT_PASSTHROUGH=1`: the frame goes through the stand-in but is copied as it is (the check
/// that the redirection alone changes no pixel).
#[allow(dead_code)]
pub fn parse_passthrough(s: &str) -> bool {
    s.trim() == "1"
}

/// The profiling switch from the environment, read once. Phones have none.
fn switch() -> Switch {
    static SWITCH: std::sync::OnceLock<Switch> = std::sync::OnceLock::new();
    *SWITCH.get_or_init(|| {
        if cfg!(any(target_os = "android", target_os = "ios")) {
            return Switch::default();
        }
        let get = |key: &str| std::env::var(key).ok();
        Switch {
            strength: get("RAZDOR_CRT").and_then(|v| parse_strength(&v)),
            quality: get("RAZDOR_CRT_QUALITY").and_then(|v| parse_quality(&v)),
            curve: get("RAZDOR_CRT_CURVE").and_then(|v| parse_curve(&v)),
            passthrough: get("RAZDOR_CRT_PASSTHROUGH").is_some_and(|v| parse_passthrough(&v)),
        }
    })
}

/// The settings the window pushes (`main_menu::apply_options`, every frame).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Setting {
    strength: Strength,
    quality: Option<Quality>,
    curve: bool,
}

razdor::local! {
    /// The settings in use, as the window last pushed them.
    static SETTING: std::cell::Cell<Setting> = const { std::cell::Cell::new(Setting { strength: Strength::Off, quality: None, curve: false }) };
    /// Whether this frame is drawn through the stand-in (`begin_frame`).
    static ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The stand-in, kept between frames while the physical size holds.
    static TARGET: std::cell::RefCell<Option<RenderTarget>> = const { std::cell::RefCell::new(None) };
}

/// Pushes the settings: the strength, the quality (`None`: the platform's) and the curve.
pub fn set_settings(strength: Strength, quality: Option<Quality>, curve: bool) {
    SETTING.with(|s| s.set(Setting { strength, quality, curve }));
}

/// The strength in use: the profiling switch when one is set, else the setting.
fn strength_in_use() -> Strength {
    switch().strength.unwrap_or_else(|| SETTING.with(|s| s.get().strength))
}

/// Whether this frame is drawn through the CRT stand-in.
pub fn active() -> bool {
    ACTIVE.with(|a| a.get())
}

/// The stand-in of `w` × `h` physical pixels, kept while the size holds.
fn target_for(w: u32, h: u32) -> RenderTarget {
    TARGET.with(|t| {
        let mut t = t.borrow_mut();
        if let Some(rt) = t.as_ref().filter(|rt| rt.texture.width() as u32 == w && rt.texture.height() as u32 == h) {
            return rt.clone();
        }
        let rt = render_target(w, h);
        razdor::mem_probe::set_texture("crt target", razdor::mem_probe::texture_bytes(w, h, 4));
        rt.texture.set_filter(FilterMode::Linear);
        *t = Some(rt.clone());
        rt
    })
}

/// The frame starts: with the effect on, the 2D space and the cameras draw into the stand-in.
pub fn begin_frame() {
    let (w, h) = macroquad::miniquad::window::screen_size();
    let fits = |v: f32| (1.0..=MAX_SIDE).contains(&v);
    let on = strength_in_use() != Strength::Off && fits(w) && fits(h);
    ACTIVE.with(|a| a.set(on));
    if !on {
        render_cache::set_screen_target(None);
        return;
    }
    render_cache::set_screen_target(Some(target_for(w as u32, h as u32)));
    render_cache::to_screen();
    clear_background(BLACK);
}

/// The frame is drawn: the stand-in goes to the real screen. Nothing when the effect is off.
pub fn end_frame() {
    if !ACTIVE.with(|a| a.replace(false)) {
        return;
    }
    let Some(rt) = render_cache::screen_target() else {
        return;
    };
    render_cache::set_screen_target(None);
    set_default_camera();
    // task 2: CRT material
    render_cache::blit_screen(&rt.texture);
}

/// The frame so far as the stand-in holds it, top row first and opaque, for the photo shutter
/// (`world_view::take_shot`): a photo has no effect on it. `None` when the effect is off.
pub fn target_image() -> Option<Image> {
    if !active() {
        return None;
    }
    let rt = render_cache::screen_target()?;
    unsafe { get_internal_gl() }.flush();
    let mut img = rt.texture.get_texture_data();
    flip_rows_opaque(&mut img.bytes, img.width as usize, img.height as usize);
    Some(img)
}

/// Flips an RGBA picture of `w` × `h` pixels top to bottom (a render target's texture comes out
/// upside down) and makes every pixel opaque. A short buffer is left as it is.
pub fn flip_rows_opaque(bytes: &mut [u8], w: usize, h: usize) {
    let stride = w * 4;
    if bytes.len() < stride * h {
        return;
    }
    for y in 0..h / 2 {
        let (top, bottom) = bytes.split_at_mut((h - 1 - y) * stride);
        top[y * stride..(y + 1) * stride].swap_with_slice(&mut bottom[..stride]);
    }
    for px in bytes[..stride * h].chunks_exact_mut(4) {
        px[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strength_names_parse_in_any_case_and_junk_is_no_value() {
        assert_eq!(parse_strength("off"), Some(Strength::Off));
        assert_eq!(parse_strength(" Light "), Some(Strength::Light));
        assert_eq!(parse_strength("NORMAL"), Some(Strength::Normal));
        assert_eq!(parse_strength("strong"), Some(Strength::Strong));
        assert_eq!(parse_strength("max"), None);
        assert_eq!(parse_strength(""), None);
    }

    #[test]
    fn the_quality_curve_and_passthrough_switches_parse() {
        assert_eq!(parse_quality("fast"), Some(Quality::Fast));
        assert_eq!(parse_quality("High"), Some(Quality::High));
        assert_eq!(parse_quality("ultra"), None);
        assert_eq!(parse_curve("1"), Some(true));
        assert_eq!(parse_curve("0"), Some(false));
        assert_eq!(parse_curve("yes"), None);
        assert!(parse_passthrough("1"));
        assert!(!parse_passthrough("0"));
    }

    #[test]
    fn the_default_is_off_and_the_platform_quality_is_normal_on_desktop() {
        assert_eq!(Strength::default(), Strength::Off);
        if !cfg!(any(target_os = "android", target_os = "ios")) {
            assert_eq!(platform_default(), Quality::Normal);
        }
    }

    #[test]
    fn a_picture_is_flipped_top_down_and_made_opaque() {
        // Two pixels wide, three high: rows 0, 1, 2 hold the values 10, 20, 30 (alpha 0).
        let mut px: Vec<u8> = [10u8, 0, 0, 0, 11, 0, 0, 0, 20, 0, 0, 0, 21, 0, 0, 0, 30, 0, 0, 0, 31, 0, 0, 0].to_vec();
        flip_rows_opaque(&mut px, 2, 3);
        let reds: Vec<u8> = px.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(reds, vec![30, 31, 20, 21, 10, 11]);
        assert!(px.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn a_short_buffer_is_left_alone() {
        let mut px = vec![1u8, 2, 3, 4];
        flip_rows_opaque(&mut px, 2, 2);
        assert_eq!(px, vec![1, 2, 3, 4]);
    }
}
