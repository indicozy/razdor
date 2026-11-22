//! The performance reports' hooks in the UI. The numbers are in `telemetry::perf`: this file gives it each
//! frame's time and screen, the loads (start, map, battle entry), the device and the quality settings once
//! a session, and sends a window's report every ten minutes and at the session's end. Nothing runs without
//! the hub (tests, snapshots, scenes, `RAZDOR_TELEMETRY=off`).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::Duration;

use macroquad::window;
use razdor::diag;
use razdor::telemetry::perf::{self, Collector};
use razdor::telemetry::state::Consent;
use razdor::telemetry::{self, Hub};
use razdor_wire::analytics::DeviceInfo;
use razdor_wire::limits;

use super::audio::Settings;
use super::{frame_state, perf_log, quality_preset, App};

/// A window is sent this long after it began, so a long session sends several reports, none twice.
const WINDOW: Duration = Duration::from_secs(600);
/// The screens that end the map and battle loads.
const MAP_SCREEN: &str = "world map";
const BATTLE_SCREEN: &str = "battle";
/// The GL strings (`GL_RENDERER`, `GL_VERSION`); `perf_log::gl_string` answers `unknown` when it fails.
const GL_RENDERER: u32 = 0x1F01;
const GL_VERSION: u32 = 0x1F02;
const UNKNOWN_GL: &str = "unknown";

struct Glue {
    collector: Collector,
    /// The device and the quality settings: read at the first frame, when the GL context exists.
    device: Option<(DeviceInfo, BTreeMap<String, String>)>,
    window_start: Duration,
    /// When a map was asked for and its first frame has not come yet.
    map_asked: Option<Duration>,
    /// When a battle was asked for and its first frame has not come yet.
    battle_asked: Option<Duration>,
}

impl Glue {
    fn new(now: Duration) -> Glue {
        Glue { collector: Collector::default(), device: None, window_start: now, map_asked: None, battle_asked: None }
    }
}

thread_local! {
    static GLUE: RefCell<Option<Glue>> = const { RefCell::new(None) };
}

/// Runs `f` with the hub and the glue (made at `now` on first use). Nothing when the analytics are off.
fn with_glue(now: Duration, f: impl FnOnce(&Hub, &mut Glue)) {
    let Some(hub) = telemetry::hub() else { return };
    GLUE.with(|g| f(hub, g.borrow_mut().get_or_insert_with(|| Glue::new(now))));
}

/// Called once a frame, after the frame's work and drawing (`main.rs`), with the time the frame took
/// without the wait for the next one.
pub fn frame(app: &App, frame_time: Duration) {
    let now = diag::since_start();
    with_glue(now, |hub, glue| {
        let screen = app.screen_name();
        if glue.device.is_none() {
            glue.device = Some(device(app));
            glue.collector.load("start", millis(now));
        }
        glue.collector.frame(now, frame_time.as_secs_f32() * 1000.0, screen);
        if screen == MAP_SCREEN {
            if let Some(asked) = glue.map_asked.take() {
                glue.collector.load("map", millis(now.saturating_sub(asked)));
            }
        }
        if screen == BATTLE_SCREEN {
            if let Some(asked) = glue.battle_asked.take() {
                glue.collector.load("battle_entry", millis(now.saturating_sub(asked)));
            }
        }
        if now.saturating_sub(glue.window_start) >= WINDOW {
            send(hub, glue, now);
        }
    });
}

/// A campaign or a save starts a map (`ui::analytics`): its first frame in the map ends the `map` load.
pub fn map_requested() {
    let now = diag::since_start();
    with_glue(now, |_, glue| glue.map_asked = Some(now));
}

/// A battle is asked for (`saves::battle`): its first frame ends the `battle_entry` load.
pub fn battle_requested() {
    let now = diag::since_start();
    with_glue(now, |_, glue| glue.battle_asked = Some(now));
}

/// The session ends (`ui::analytics::session_end`, every exit): the window's report is queued.
pub fn session_end() {
    let now = diag::since_start();
    with_glue(now, |hub, glue| send(hub, glue, now));
}

/// Empties the window and queues its report (consent yes only). The next window begins at `now`.
fn send(hub: &Hub, glue: &mut Glue, now: Duration) {
    glue.window_start = now;
    let window = glue.collector.take();
    if hub.consent() != Consent::Yes {
        return;
    }
    let Some((device, quality)) = glue.device.as_ref() else { return };
    if let Some(report) = perf::report(&hub.install_id(), device, quality, window, perf::peak_ram_mb()) {
        let _ = perf::enqueue(hub, &report);
    }
}

fn millis(d: Duration) -> u32 {
    d.as_millis().min(u128::from(u32::MAX)) as u32
}

/// The device once a session (`DeviceInfo`) and the quality settings as catalog keys.
fn device(app: &App) -> (DeviceInfo, BTreeMap<String, String>) {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let info = DeviceInfo {
        os: std::env::consts::OS.to_string(),
        os_version: perf::os_version(),
        gpu: gl_text(GL_RENDERER),
        gpu_driver: gl_text(GL_VERSION),
        cpu_cores: u32::try_from(cores).unwrap_or(limits::MAX_TEXT as u32).min(1024),
        ram_mb: perf::total_ram_mb(),
        screen_w: window::screen_width().max(0.0) as u32,
        screen_h: window::screen_height().max(0.0) as u32,
        scale_pct: (frame_state::dpi() * 100.0).round().clamp(1.0, 1000.0) as u32,
        phone: None,
    };
    (info, quality(&app.audio.settings))
}

/// A GL string as text, without the unknown marker and control characters, short enough for the wire.
fn gl_text(name: u32) -> String {
    let s = perf_log::gl_string(name);
    if s == UNKNOWN_GL {
        return String::new();
    }
    s.chars().filter(|c| !c.is_control()).take(limits::MAX_TEXT).collect()
}

/// The graphics preset and the map's quality parts: the names of the choices (lower case), no typed values.
fn quality(s: &Settings) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let preset = quality_preset::current(s).map_or_else(|| "none".to_string(), |p| format!("{p:?}").to_lowercase());
    m.insert("preset".to_string(), preset);
    macro_rules! parts {
        ($($field:ident),* $(,)?) => {
            $( m.insert(stringify!($field).to_string(), format!("{:?}", s.$field).to_lowercase()); )*
        };
    }
    parts!(
        map_render_scale,
        frame_cap,
        battery_saver,
        map_anim,
        map_light,
        map_light_quality,
        map_shadows,
        map_mist,
        map_live_water,
        map_water_rate,
        map_water_quality,
        map_weather,
        map_living,
        map_tilt,
        map_first_person,
        map_horizon,
        map_globe,
        map_sprite_size,
        map_extended,
        map_steady,
        map_zoom,
        ui_scale,
        text_size,
        display,
    );
    m
}
