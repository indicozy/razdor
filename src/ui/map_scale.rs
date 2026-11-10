//! The world map's drawing resolution: the setting «Разрешение карты» (`audio::Settings::map_render_scale`,
//! 100%, 75% or 50%; for low-end phone GPUs). Below 100% the world (`world_view::draw_world` and the
//! flat view's fog, the map under a window) is drawn into a target of `ceil(screen * scale)` pixels and
//! laid over the screen with linear filtering. Everything after it (the bottom bar, the minimap window,
//! windows, tooltips, text, the cursor) is drawn at native resolution. The drawing keeps its screen
//! coordinates: `render_cache::with_base` sends every camera reset inside the world to the target, and
//! the picking maths is unchanged. At 100% this module runs the closure directly, as before.
//!
//! A profiling switch overrides the setting: `RAZDOR_MAP_SCALE=0.75` or `0.5` (desktop), the file
//! `profile-map-scale-75` / `profile-map-scale-50` in the app folder (Android).

use super::render_cache;
use super::sharp;
use macroquad::color::Color;
use macroquad::math::Rect;
use macroquad::shapes::draw_rectangle;
use macroquad::texture::{render_target, FilterMode, RenderTarget};
use macroquad::window::{screen_height, screen_width};

/// The values the setting takes, in the order its row cycles them.
pub const SCALES: [f32; 3] = [1.0, 0.75, 0.5];

/// The largest target side in physical pixels; past it the map is drawn at native resolution.
const MAX_SIDE: f32 = 8192.0;

/// The setting's value, or 100% for a value not in [`SCALES`] (a file's own, or not finite).
pub fn valid(f: f32) -> f32 {
    SCALES.iter().copied().find(|&s| (s - f).abs() < 1e-3).unwrap_or(1.0)
}

/// The value the row shows after `f`: 100% → 75% → 50% → 100%. A value not in [`SCALES`] goes to 100%.
pub fn next(f: f32) -> f32 {
    match SCALES.iter().position(|&s| (s - f).abs() < 1e-3) {
        Some(i) => SCALES[(i + 1) % SCALES.len()],
        None => SCALES[0],
    }
}

/// The row's value: «100%», «75%», «50%».
pub fn label(f: f32) -> String {
    format!("{}%", (valid(f) * 100.0).round() as u32)
}

/// The target's size in physical pixels for a `w` × `h` screen at `scale`: each side rounded up.
pub fn target_size(w: u32, h: u32, scale: f32) -> (u32, u32) {
    let side = |v: u32| (v as f32 * scale).ceil().max(1.0) as u32;
    (side(w), side(h))
}

/// The scale a profiling switch names, if one is set: the environment value (`env`) first, then the
/// files (`has_file`, the app folder on Android), `None` when neither is.
pub fn switched(env: Option<&str>, has_file: impl Fn(&str) -> bool) -> Option<f32> {
    if let Some(v) = env.and_then(|v| v.trim().parse::<f32>().ok()) {
        return Some(valid(v));
    }
    if has_file("profile-map-scale-75") {
        return Some(0.75);
    }
    if has_file("profile-map-scale-50") {
        return Some(0.5);
    }
    None
}

/// The scale in use: the profiling switch when one is set (read once), else the setting.
pub fn in_use(setting: f32) -> f32 {
    static SWITCH: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    SWITCH
        .get_or_init(|| {
            let env = if cfg!(target_os = "android") { None } else { std::env::var("RAZDOR_MAP_SCALE").ok() };
            switched(env.as_deref(), |f| cfg!(target_os = "android") && razdor::platform::app_dir().is_some_and(|d| d.join(f).is_file()))
        })
        .unwrap_or_else(|| valid(setting))
}

razdor::local! {
    /// The map's target, kept between frames: made again when the physical size or the scale changes.
    static TARGET: std::cell::RefCell<Option<RenderTarget>> = const { std::cell::RefCell::new(None) };
}

/// The target of `w` × `h` physical pixels, kept while the size holds.
fn target_for(w: u32, h: u32) -> RenderTarget {
    TARGET.with(|t| {
        let mut t = t.borrow_mut();
        if let Some(rt) = t.as_ref().filter(|rt| rt.texture.width() as u32 == w && rt.texture.height() as u32 == h) {
            return rt.clone();
        }
        let rt = render_target(w, h);
        rt.texture.set_filter(FilterMode::Linear);
        *t = Some(rt.clone());
        rt
    })
}

/// Runs `f` (the world's drawing) at the map's resolution: into the target, which is then laid over the
/// screen. `background` is the colour the world starts from (the frame's clear colour). Runs `f` on the
/// screen as before at 100%, past the size limit, and while the screen is panned (`ui::pan`: its offset
/// camera is not redirected).
pub fn draw<R>(setting: f32, background: Color, f: impl FnOnce() -> R) -> R {
    let scale = in_use(setting);
    let (w, h) = macroquad::miniquad::window::screen_size();
    let usable = w >= 1.0 && h >= 1.0 && w <= MAX_SIDE && h <= MAX_SIDE;
    if scale >= 1.0 || !usable || super::pan::shift() != (0.0, 0.0) {
        return f();
    }
    let (pw, ph) = target_size(w as u32, h as u32, scale);
    let rt = target_for(pw, ph);
    let (sw, sh) = (screen_width(), screen_height());
    let out = render_cache::with_base(&rt, Rect::new(0.0, 0.0, sw, sh), || {
        draw_rectangle(0.0, 0.0, sw, sh, background);
        // The sharp band follows the target's pixels: target px per 2D unit.
        sharp::set_raster(pw as f32 / sw);
        let out = f();
        sharp::set_raster(1.0);
        out
    });
    render_cache::blit_screen(&rt.texture);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_is_the_screen_scaled_and_rounded_up() {
        assert_eq!(target_size(1600, 720, 1.0), (1600, 720));
        assert_eq!(target_size(1600, 720, 0.75), (1200, 540));
        assert_eq!(target_size(1601, 721, 0.75), (1201, 541), "odd sizes round up, never down");
        assert_eq!(target_size(1080, 2340, 0.5), (540, 1170));
        assert_eq!(target_size(1, 1, 0.5), (1, 1), "never empty");
    }

    #[test]
    fn the_setting_cycles_and_a_stored_value_is_checked() {
        assert_eq!(next(1.0), 0.75);
        assert_eq!(next(0.75), 0.5);
        assert_eq!(next(0.5), 1.0);
        assert_eq!(next(0.6), 1.0, "a value not in the list goes to 100%");
        assert_eq!(next(f32::NAN), 1.0);
        assert_eq!(valid(0.6), 1.0);
        assert_eq!(valid(f32::NAN), 1.0);
        assert_eq!(valid(0.5), 0.5);
        assert_eq!(label(1.0), "100%");
        assert_eq!(label(0.75), "75%");
        assert_eq!(label(0.5), "50%");
    }

    #[test]
    fn the_switch_reads_the_environment_then_the_files() {
        let none = |_: &str| false;
        assert_eq!(switched(None, none), None);
        assert_eq!(switched(Some("0.75"), none), Some(0.75));
        assert_eq!(switched(Some(" 0.5 "), none), Some(0.5));
        assert_eq!(switched(Some("junk"), none), None, "an unreadable value is no switch");
        assert_eq!(switched(Some("1"), |_| true), Some(1.0), "the environment wins over a file");
        assert_eq!(switched(None, |f| f == "profile-map-scale-75"), Some(0.75));
        assert_eq!(switched(None, |f| f == "profile-map-scale-50"), Some(0.5));
    }
}
