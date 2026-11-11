//! The frame rate cap: the setting «Ограничение FPS» (`audio::Settings::frame_cap`: 0 off, the
//! default; 30 or 60). With a cap the frames start no closer than 1/cap s apart: after each frame is
//! presented the loop sleeps the rest of the period (`Pacer::pace`). Nothing sleeps in the snapshots
//! and the trailer. Profiling switch: `RAZDOR_FRAME_CAP=30` (desktop), the file `profile-frame-cap-30`
//! (or `-60`) in the app folder (Android); it overrides the setting.

use std::time::{Duration, Instant};

/// The values the setting takes, in the order its row cycles them (0 is off).
pub const CAPS: [u32; 3] = [0, 30, 60];

/// The setting's value, or off (0) for a value not in [`CAPS`].
pub fn valid(cap: u32) -> u32 {
    if CAPS.contains(&cap) { cap } else { 0 }
}

/// The value the row shows after `cap`: off → 30 → 60 → off.
pub fn next(cap: u32) -> u32 {
    match CAPS.iter().position(|&c| c == cap) {
        Some(i) => CAPS[(i + 1) % CAPS.len()],
        None => CAPS[0],
    }
}

/// How long a frame must wait after the previous frame's start: the rest of `1 / cap` s, zero when
/// the cap is off or the frame already took that long (never a sleep when behind).
pub fn wait_for(since_last_start: Duration, cap: u32) -> Duration {
    if cap == 0 {
        return Duration::ZERO;
    }
    let period = Duration::from_secs_f64(1.0 / cap as f64);
    period.saturating_sub(since_last_start)
}

/// The cap a profiling switch names: the environment value first, then the files (`has_file`), `None`
/// when neither is set.
pub fn switched(env: Option<&str>, has_file: impl Fn(&str) -> bool) -> Option<u32> {
    if let Some(v) = env.and_then(|v| v.trim().parse::<u32>().ok()) {
        return Some(valid(v));
    }
    if has_file("profile-frame-cap-30") {
        return Some(30);
    }
    if has_file("profile-frame-cap-60") {
        return Some(60);
    }
    None
}

/// The cap in use: the profiling switch when one is set (read once), else the setting.
pub fn in_use(setting: u32) -> u32 {
    static SWITCH: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    SWITCH
        .get_or_init(|| {
            let env = if cfg!(target_os = "android") { None } else { std::env::var("RAZDOR_FRAME_CAP").ok() };
            switched(env.as_deref(), |f| cfg!(target_os = "android") && razdor::platform::app_dir().is_some_and(|d| d.join(f).is_file()))
        })
        .unwrap_or_else(|| valid(setting))
}

/// The setting as the settings window last pushed it (`main_menu::apply_options`); `u32::MAX` until then.
static SETTING: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);

/// Puts the cap from the settings. On Android it is also written to `frame-cap` in the app folder when
/// it changes: the activity asks the display for this rate (`razdorFrameCap` in
/// `android/MainActivityInject.java`).
pub fn set_setting(cap: u32) {
    let cap = valid(cap);
    let before = SETTING.swap(cap, std::sync::atomic::Ordering::Relaxed);
    if before != cap && cfg!(target_os = "android") {
        if let Some(dir) = razdor::platform::app_dir() {
            // Not fatal: without the file the activity keeps the top refresh rate.
            let _ = std::fs::write(dir.join("frame-cap"), cap.to_string());
        }
    }
}

/// The cap the loop paces to now: the profiling switch or the setting (0 = off), or the idle rate after an
/// idle map frame with «Экономия батареи» on (`battery_cap`). Called once a frame, by the pacer, which takes
/// the idle flag the frame set.
pub fn current() -> u32 {
    let setting = SETTING.load(std::sync::atomic::Ordering::Relaxed);
    let base = in_use(if setting == u32::MAX { 0 } else { setting });
    battery_cap(base, BATTERY.load(std::sync::atomic::Ordering::Relaxed), IDLE.swap(false, std::sync::atomic::Ordering::Relaxed))
}

/// The idle frame rate of «Экономия батареи» (the owner's request 2026-11-11: 5 to 10 a second, so the map's
/// animations still tick slowly).
pub const IDLE_FPS: u32 = 10;

/// The cap of a frame: `base`, but after an idle frame with the battery saver on, no more than [`IDLE_FPS`].
pub fn battery_cap(base: u32, battery: bool, idle: bool) -> u32 {
    if battery && idle && (base == 0 || base > IDLE_FPS) {
        IDLE_FPS
    } else {
        base
    }
}

/// «Экономия батареи» is on (`audio::Settings::battery_saver`, pushed by [`set_battery`]).
static BATTERY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The map frame just drawn changed nothing the player can act on (`world_view::note_idle_frame`).
static IDLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Puts the battery saver from the settings (called with the other settings).
pub fn set_battery(on: bool) {
    BATTERY.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Marks the frame just drawn as idle or not (the map frame sets it; the pacer reads it once).
pub fn note_idle(idle: bool) {
    IDLE.store(idle, std::sync::atomic::Ordering::Relaxed);
}

/// The margin the sleep leaves to a spin: the OS sleep may wake late, so the last part of the wait is
/// spun (yielding) to the deadline.
const SPIN: Duration = Duration::from_millis(1);

/// Paces the loop: called once a frame, after it is presented. Keeps the frames' starts a period apart.
#[derive(Default)]
pub struct Pacer {
    /// The previous frame's start (the moment the previous `pace` returned).
    last: Option<Instant>,
}

impl Pacer {
    /// Waits until a period has passed since the previous frame's start (per `cap`, 0 for none), then
    /// records this frame's start. `paced` is false in the snapshots and the trailer: no sleep there.
    pub fn pace(&mut self, cap: u32, paced: bool) {
        let cap = if paced { cap } else { 0 };
        if let (true, Some(last)) = (cap > 0, self.last) {
            let wait = wait_for(last.elapsed(), cap);
            sleep_until(Instant::now() + wait);
        }
        self.last = Some(Instant::now());
    }
}

/// Sleeps until `deadline`: the bulk with the OS sleep, the last [`SPIN`] spun (yielding).
fn sleep_until(deadline: Instant) {
    let now = Instant::now();
    if deadline <= now {
        return;
    }
    let left = deadline - now;
    if left > SPIN {
        std::thread::sleep(left - SPIN);
    }
    while Instant::now() < deadline {
        std::thread::yield_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wait_is_the_rest_of_the_period_and_never_negative() {
        assert_eq!(wait_for(Duration::from_millis(5), 0), Duration::ZERO, "no cap, no wait");
        let w = wait_for(Duration::from_millis(10), 30);
        assert!((w.as_secs_f64() - (1.0 / 30.0 - 0.010)).abs() < 1e-6, "{w:?}");
        let w = wait_for(Duration::from_millis(5), 60);
        assert!((w.as_secs_f64() - (1.0 / 60.0 - 0.005)).abs() < 1e-6, "{w:?}");
        assert_eq!(wait_for(Duration::from_millis(20), 60), Duration::ZERO, "a frame over the period is never slept");
        assert_eq!(wait_for(Duration::ZERO, 30), Duration::from_secs_f64(1.0 / 30.0));
    }

    #[test]
    fn the_battery_saver_caps_only_an_idle_frame_and_never_raises_the_cap() {
        assert_eq!(battery_cap(0, true, true), IDLE_FPS, "no cap: the idle rate");
        assert_eq!(battery_cap(60, true, true), IDLE_FPS, "a higher cap: the idle rate");
        assert_eq!(battery_cap(30, true, true), IDLE_FPS);
        assert_eq!(battery_cap(10, true, true), 10, "a cap at or under the idle rate stays");
        assert_eq!(battery_cap(60, true, false), 60, "a busy frame keeps the cap");
        assert_eq!(battery_cap(0, true, false), 0, "a busy frame with no cap is not paced");
        assert_eq!(battery_cap(60, false, true), 60, "the saver off: no change");
    }

    #[test]
    fn the_setting_cycles_off_30_60_and_a_stored_value_is_checked() {
        assert_eq!(next(0), 30);
        assert_eq!(next(30), 60);
        assert_eq!(next(60), 0);
        assert_eq!(next(45), 0, "a value not in the list goes to off");
        assert_eq!(valid(45), 0);
        assert_eq!(valid(30), 30);
        assert_eq!(valid(60), 60);
    }

    #[test]
    fn the_switch_reads_the_environment_then_the_files() {
        let none = |_: &str| false;
        assert_eq!(switched(None, none), None);
        assert_eq!(switched(Some("30"), none), Some(30));
        assert_eq!(switched(Some("junk"), none), None);
        assert_eq!(switched(Some("0"), |_| true), Some(0), "the environment wins over a file");
        assert_eq!(switched(None, |f| f == "profile-frame-cap-30"), Some(30));
        assert_eq!(switched(None, |f| f == "profile-frame-cap-60"), Some(60));
    }

    #[test]
    fn the_pacer_keeps_frame_starts_a_period_apart_and_skips_when_unpaced() {
        let mut p = Pacer::default();
        let t0 = Instant::now();
        p.pace(60, true);
        p.pace(60, true);
        let gap = p.last.unwrap() - t0;
        assert!(gap >= Duration::from_secs_f64(1.0 / 60.0) - Duration::from_micros(200), "{gap:?}");
        let mut snap = Pacer::default();
        let s = Instant::now();
        snap.pace(60, false);
        snap.pace(60, false);
        assert!(s.elapsed() < Duration::from_millis(8), "snapshots and the trailer never sleep");
    }
}
