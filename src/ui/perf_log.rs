//! The performance log: what a player's report needs to be read from `razdor.log` alone. The device
//! and its display, the GL driver, the world map's view and settings, and the phone's heat. Logging
//! only: nothing here changes a frame, a setting or a screen. Nothing runs per frame but a clock check
//! in the debug mode ([`tick`]).
//!
//! The lines: `device:`, `gl:` and `display:` once the window is open ([`startup`]); `display:` again
//! when an Android activity resumes ([`on_resume`]); `map:`, `view:` and `settings:` when a game's world
//! map opens ([`after_frame`]); `settings:` again when a settings window closes with a graphics setting
//! changed; `thermal:` every 60 s in the debug mode, on Android ([`tick`]). The Android strings are the
//! activity's (`android/MainActivityInject.java`: `razdorDeviceInfo`, `razdorDisplayInfo`,
//! `razdorThermalInfo`). No serial number or id is written.

use super::audio::Settings;
use super::{bench, frame_cap, frame_state, map_scale, quality_preset, terrain, world_view, App};
use macroquad::miniquad::gl;
use razdor::diag;
use razdor::rules::game::Game;
use razdor::rules::save::{self, ScenarioRef};
use std::cell::{Cell, RefCell};
use std::ffi::CStr;
use std::time::{Duration, Instant};

/// How often the thermal line is written in the debug mode.
pub const THERMAL_EVERY: Duration = Duration::from_secs(60);

/// How often the `mem:` line is written in the debug mode (the frame probe's period).
pub const MEM_EVERY: Duration = Duration::from_secs(5);

/// GL enums: the vendor, renderer, version and shading language strings, and the texture size limit.
pub const GL_RENDERER: u32 = 0x1F01;
const GL_VENDOR: u32 = 0x1F00;
const GL_VERSION: u32 = 0x1F02;
const GL_SHADING_LANGUAGE_VERSION: u32 = 0x8B8C;
const GL_MAX_TEXTURE_SIZE: u32 = 0x0D33;

razdor::local! {
    /// The settings line last logged: a settings window's close logs only a change from it.
    static LAST_SETTINGS: RefCell<Option<String>> = const { RefCell::new(None) };
    /// When the thermal line was last written (the debug mode).
    static LAST_THERMAL: Cell<Option<Instant>> = const { Cell::new(None) };
    /// When the `mem:` line was last written (the debug mode).
    static LAST_MEM: Cell<Option<Instant>> = const { Cell::new(None) };
    /// A game's world map has been logged in this run (`map:`).
    static MAP_LOGGED: Cell<bool> = const { Cell::new(false) };
}

/// The lines of the window's start, once it is open (`main`): the device, the GL driver and the display.
pub fn startup() {
    diag::log(&format!("device: {}", device_text()));
    diag::log(&gl_line());
    diag::log(&format!("display: {}", display_text()));
}

/// The display again, when the Android activity resumes: the mode and the rates may have changed while
/// the app was away (`android::follow_orientation` sees the resume).
#[cfg(target_os = "android")]
pub fn on_resume() {
    diag::log(&format!("display: {}", display_text()));
}

/// Called once a frame, after `App::frame`, with the screen the frame began on: a game's world map
/// opening writes its `map:`, `view:` and `settings:` lines, and a settings window's close writes its
/// `settings:` line when a graphics setting changed.
pub fn after_frame(app: &App, before: &str) {
    let now = app.screen_name();
    let changed = now != before;
    if changed && matches!(before, "settings" | "options") {
        settings_closed(&app.audio.settings);
    }
    // The run's first world map is logged too, though no screen change brought it (a scene or a save
    // that starts on the map).
    let first = !MAP_LOGGED.get();
    if now == "world map" && ((changed && starts_map(before)) || first) {
        map_opened(app);
    }
}

/// The screens a game's world map opens from: a new game (class select, the tutorial offer), a loaded save
/// (the main menu, the load window), the next campaign map (victory, the summary), a restart (the menu)
/// and the editor's test play.
pub fn starts_map(from: &str) -> bool {
    matches!(from, "main menu" | "scenario select" | "class select" | "tutorial offer" | "load" | "menu" | "victory" | "summary" | "editor")
}

/// Called once a frame: in the debug mode, the `mem:` line every [`MEM_EVERY`] and the thermal line every
/// [`THERMAL_EVERY`] (both at once when due). Nothing else runs per frame. Desktop writes no thermal line.
pub fn tick() {
    if !diag::debug_mode() {
        return;
    }
    let now = Instant::now();
    if razdor::frame_probe::due(LAST_MEM.get(), now, MEM_EVERY) {
        LAST_MEM.set(Some(now));
        diag::log(&mem_line());
    }
    if razdor::frame_probe::due(LAST_THERMAL.get(), now, THERMAL_EVERY) {
        LAST_THERMAL.set(Some(now));
        if let Some(line) = thermal_extra() {
            diag::log(&format!("thermal: {line}"));
        }
    }
}

/// The `mem:` line: the process's resident set, the GPU textures' estimate (the largest three by name) and,
/// on Android, the activity's heap and memory (`razdorMemInfo`).
pub fn mem_line() -> String {
    let rss = razdor::mem_probe::rss_mb().map_or("na".to_string(), |m| format!("{m:.0}"));
    let mut s = format!("mem: rss_mb={rss} {}", razdor::mem_probe::texture_text(3));
    push_extra(&mut s, mem_extra());
    s
}

/// The header lines of a performance test's report (`bench::Report::setup`): the device, the display, the
/// render size and the player's settings when the run started. The cells change some settings, and the
/// cell lines say which.
pub fn bench_header(s: &Settings) -> Vec<String> {
    let (w, h) = window_px();
    let scale = map_scale::in_use(s.map_render_scale);
    let (mw, mh) = map_scale::target_size(w, h, scale);
    vec![
        format!("Device: {}", device_text()),
        format!("Display: {}", display_text()),
        format!("Render: window {w}x{h}, map {mw}x{mh} (scale {}), view {}", map_scale::label(scale), view_kind(world_view::tilt_view(), s.map_first_person)),
        format!("Settings: {}", settings_line(s)),
    ]
}

/// The GL string `name` of the context (`glGetString`), or `unknown`. Called on the main thread with the
/// context current (the window is open).
pub fn gl_string(name: u32) -> String {
    // SAFETY: called on the main thread once the context exists; GL returns a NUL-terminated string that
    // lives as long as the context.
    let p = unsafe { gl::glGetString(name) };
    if p.is_null() {
        return "unknown".into();
    }
    // SAFETY: as above.
    unsafe { CStr::from_ptr(p as *const std::ffi::c_char) }.to_string_lossy().into_owned()
}

/// The `gl:` line: the vendor, renderer, version, shading language and the largest texture side.
pub fn gl_line() -> String {
    let mut max = 0i32;
    // SAFETY: as in `gl_string`; the out pointer is a live `i32`.
    unsafe { gl::glGetIntegerv(GL_MAX_TEXTURE_SIZE, &mut max) };
    format!(
        "gl: vendor={} renderer={} version={} glsl={} max_texture={max}",
        bench::quoted(&gl_string(GL_VENDOR)),
        bench::quoted(&gl_string(GL_RENDERER)),
        bench::quoted(&gl_string(GL_VERSION)),
        bench::quoted(&gl_string(GL_SHADING_LANGUAGE_VERSION)),
    )
}

/// The device's text (the `device:` line after its prefix): the OS, the CPU count and the OS's name, then
/// on Android the activity's model, SoC, version and memory.
pub fn device_text() -> String {
    let cpus = std::thread::available_parallelism().map_or(0, |n| n.get());
    let mut s = format!(
        "os={} arch={} cpus={cpus} os_name={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        bench::quoted(&bench::os_name())
    );
    push_extra(&mut s, device_extra());
    s
}

/// The display's text (the `display:` line after its prefix): the window's size and DPI, then on Android
/// the activity's modes, rates, the window's preference and the frame cap.
pub fn display_text() -> String {
    let (w, h) = window_px();
    let mut s = format!("screen={w}x{h} dpi={}", frame_state::dpi());
    push_extra(&mut s, display_extra());
    s
}

/// The `map:` line of a game: its map's file and title, the mod played, the map's size in cells, its objects
/// (decorations), buildings and armies, and the hero's cell.
pub fn map_line(game: &Game) -> String {
    let file = match &game.origin {
        Some(ScenarioRef::Map { file, .. }) => file.clone(),
        Some(ScenarioRef::Demo) | None => "demo".into(),
    };
    let played = save::played_mod().map_or("none".to_string(), |m| format!("{}@{}", m.id, m.version));
    let map = &game.world.map;
    let (hx, hy) = game.tile();
    format!(
        "map: file={} title={} mod={played} cells={}x{} objects={} buildings={} armies={} hero=({hx},{hy})",
        bench::quoted(&file),
        bench::quoted(&game.world.title),
        map.w,
        map.h,
        map.objects.len(),
        game.world.locations.len(),
        game.world.armies.len(),
    )
}

/// The `view:` line: the window's size and DPI, the map's drawn size (`map_render_scale` applied), the map's
/// zoom, the view kind, the frame cap in use at this moment, and the terrain options' label.
pub fn view_line(zoom: f32, s: &Settings, window: (u32, u32), dpi: f32) -> String {
    let scale = map_scale::in_use(s.map_render_scale);
    let (mw, mh) = map_scale::target_size(window.0, window.1, scale);
    format!(
        "view: window={}x{} dpi={dpi} map_render={mw}x{mh} map_scale={} map_zoom={zoom:.2} view={} frame_cap_now={} terrain={}",
        window.0,
        window.1,
        map_scale::label(scale),
        view_kind(world_view::tilt_view(), s.map_first_person),
        frame_cap::peek(),
        bench::quoted(&terrain::options().label()),
    )
}

/// The view's name in the log: `flat`, `2.5D` (tilted) or `first-person` (tilted, with the first-person view).
pub fn view_kind(tilted: bool, first_person: bool) -> &'static str {
    match (tilted, first_person) {
        (false, _) => "flat",
        (true, false) => "2.5D",
        (true, true) => "first-person",
    }
}

/// The `settings:` text: the graphics-relevant settings, each as its value (Debug for the enums), with the
/// quality preset the settings match (`none` for a custom mix). Static values only: the frame cap in use
/// as set, not the idle cap, which changes from frame to frame (`view:` has it), so a change compares equal
/// while nothing was changed.
pub fn settings_line(s: &Settings) -> String {
    let preset = quality_preset::current(s).map_or("none".to_string(), |p| format!("{p:?}"));
    format!(
        "quality={preset} map_render_scale={} frame_cap={} battery_saver={} map_anim={:?} map_light={:?} map_light_quality={:?} map_shadows={:?} map_mist={:?} map_live_water={} map_water_rate={:?} map_water_quality={:?} map_weather={} map_living={} map_tilt={} map_first_person={} map_eyes={} map_horizon={} map_globe={} map_sprite_size={} map_extended={} map_steady={} map_zoom={} ui_scale={} text_size={} display={:?} show_fps={} debug_mode={} record_game={}",
        s.map_render_scale,
        s.frame_cap,
        s.battery_saver,
        s.map_anim,
        s.map_light,
        s.map_light_quality,
        s.map_shadows,
        s.map_mist,
        s.map_live_water,
        s.map_water_rate,
        s.map_water_quality,
        s.map_weather,
        s.map_living,
        s.map_tilt,
        s.map_first_person,
        s.map_eyes,
        s.map_horizon,
        s.map_globe,
        s.map_sprite_size,
        s.map_extended,
        s.map_steady,
        s.map_zoom,
        s.ui_scale,
        s.text_size,
        s.display,
        s.show_fps,
        s.debug_mode,
        s.record_game,
    )
}

/// A game's world map opened: its `map:`, `view:` and the `settings:` snapshot the later close compares with.
fn map_opened(app: &App) {
    let Some(game) = app.game.as_ref() else { return };
    MAP_LOGGED.set(true);
    let s = &app.audio.settings;
    let (w, h) = window_px();
    diag::log(&map_line(game));
    diag::log(&view_line(app.map_view.zoom, s, (w, h), frame_state::dpi()));
    diag::log(&format!("gpu-textures: {}", razdor::mem_probe::texture_text(5)));
    remember(s);
}

/// A settings window closed (`before` was settings or options): logs the `settings:` line when it differs
/// from the last logged one, else nothing.
fn settings_closed(s: &Settings) {
    let line = settings_line(s);
    let changed = LAST_SETTINGS.with_borrow(|last| last.as_deref() != Some(line.as_str()));
    if changed {
        remember(s);
    }
}

/// Logs the `settings:` line of `s` and keeps it as the last one logged.
fn remember(s: &Settings) {
    let line = settings_line(s);
    diag::log(&format!("settings: {line}"));
    LAST_SETTINGS.with_borrow_mut(|last| *last = Some(line));
}

/// The window's size in whole pixels, as this frame sees it.
fn window_px() -> (u32, u32) {
    (frame_state::screen_w().round().max(0.0) as u32, frame_state::screen_h().round().max(0.0) as u32)
}

/// Appends an extra part (the activity's line on Android) with a space, unless it is missing or empty.
fn push_extra(s: &mut String, extra: Option<String>) {
    if let Some(more) = extra.filter(|m| !m.is_empty()) {
        s.push(' ');
        s.push_str(&more);
    }
}

#[cfg(target_os = "android")]
fn device_extra() -> Option<String> {
    super::android::device_info()
}

#[cfg(not(target_os = "android"))]
fn device_extra() -> Option<String> {
    None
}

#[cfg(target_os = "android")]
fn display_extra() -> Option<String> {
    super::android::display_info()
}

#[cfg(not(target_os = "android"))]
fn display_extra() -> Option<String> {
    None
}

#[cfg(target_os = "android")]
fn thermal_extra() -> Option<String> {
    super::android::thermal_info()
}

#[cfg(not(target_os = "android"))]
fn thermal_extra() -> Option<String> {
    None
}

#[cfg(target_os = "android")]
fn mem_extra() -> Option<String> {
    super::android::mem_info()
}

#[cfg(not(target_os = "android"))]
fn mem_extra() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_map_opens_from_the_start_screens_and_a_menu_but_not_from_inside_a_game() {
        for from in ["main menu", "scenario select", "class select", "tutorial offer", "load", "menu", "victory", "summary", "editor"] {
            assert!(starts_map(from), "{from} starts a game's map");
        }
        for from in ["building", "battle", "army", "hero", "settings", "options", "world map", "save"] {
            assert!(!starts_map(from), "{from} does not open a game's map");
        }
    }

    #[test]
    fn the_view_kind_names_the_flat_map_the_tilted_one_and_the_first_person_one() {
        assert_eq!(view_kind(false, false), "flat");
        assert_eq!(view_kind(false, true), "flat", "the first person needs the tilt");
        assert_eq!(view_kind(true, false), "2.5D");
        assert_eq!(view_kind(true, true), "first-person");
    }

    #[test]
    fn the_settings_line_has_the_graphics_settings_as_their_values() {
        let s = Settings::default();
        let line = settings_line(&s);
        assert!(!line.contains('\n'), "one line");
        for part in ["map_render_scale=1 ", "frame_cap=0 ", "battery_saver=false ", "map_tilt=false ", "map_mist=", "map_anim=", "display=", "quality="] {
            assert!(line.contains(part), "{part} is in {line}");
        }
        assert!(!line.contains("keys="), "the rebound keys are not a graphics setting");
    }

    #[test]
    fn a_changed_graphics_setting_changes_the_line_and_an_unchanged_one_does_not() {
        let mut s = Settings::default();
        let before = settings_line(&s);
        assert_eq!(settings_line(&s), before, "the same settings give the same line");
        s.map_render_scale = 0.75;
        assert!(settings_line(&s).contains("map_render_scale=0.75"));
        assert_ne!(settings_line(&s), before, "a changed graphics setting shows");
        let t = Settings { debug_mode: true, ..Settings::default() };
        assert_ne!(settings_line(&t), before, "debug mode is logged too");
    }

    #[test]
    fn the_thermal_line_is_due_at_first_and_then_every_sixty_seconds() {
        let t0 = Instant::now();
        let due = |last, now| razdor::frame_probe::due(last, now, THERMAL_EVERY);
        assert!(due(None, t0), "the first one at once");
        assert!(!due(Some(t0), t0 + Duration::from_secs(59)));
        assert!(due(Some(t0), t0 + Duration::from_secs(60)));
    }

    #[test]
    fn the_mem_line_names_the_process_and_the_textures() {
        let line = mem_line();
        assert!(line.starts_with("mem: rss_mb="), "{line}");
        assert!(line.contains(" gpu_tex_est_mb="), "{line}");
        assert!(!line.contains('\n'), "one line");
    }

    #[test]
    fn an_activity_line_is_added_only_when_it_has_text() {
        let mut s = String::from("os=linux");
        push_extra(&mut s, None);
        push_extra(&mut s, Some(String::new()));
        assert_eq!(s, "os=linux");
        push_extra(&mut s, Some("model=\"moto g32\"".into()));
        assert_eq!(s, "os=linux model=\"moto g32\"");
    }
}
