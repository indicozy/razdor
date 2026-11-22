//! Performance reports (`PerfReport`): the frame times of each screen, the loads, the peak memory and
//! the device, collected while the game runs and turned into one report per window. Pure: no macroquad,
//! the time is passed in. The UI glue is `ui::perf_analytics`.

use std::collections::BTreeMap;
use std::time::Duration;

use razdor_wire::analytics::{DeviceInfo, LoadTime, MemoryInfo, PerfReport, ScreenStats};
use razdor_wire::{limits, paths, InstallId};

use super::queue::Class;
use super::{Hub, Refused};

/// Width of a histogram bin, in ms.
pub const BIN_MS: f32 = 0.5;
/// The bins below the overflow bin: frames up to 100 ms each have their own bin.
const BINS: usize = 200;
/// Frames at or over this share the overflow bin.
const OVERFLOW_MS: f32 = BIN_MS * BINS as f32;
/// A pause between two frames counts at most this long, so a pause does not count as minutes on a screen.
pub const MAX_GAP: Duration = Duration::from_secs(1);
/// The screen that takes the names beyond the screen limit.
pub const OTHER: &str = "other";
/// The largest frame time or load kept (the wire's own limits are above this).
const LARGEST_MS: f32 = 600_000.0;

/// Frame times of one screen in fixed bins, so the percentiles need no stored frames.
#[derive(Clone, Debug)]
pub struct Histogram {
    /// `BINS` bins of [`BIN_MS`], then the overflow bin.
    bins: Vec<u32>,
    count: u64,
    /// The worst frame in the overflow bin.
    overflow_worst: f32,
}

impl Default for Histogram {
    fn default() -> Self {
        Histogram { bins: vec![0; BINS + 1], count: 0, overflow_worst: 0.0 }
    }
}

impl Histogram {
    pub fn record(&mut self, ms: f32) {
        let ms = if ms.is_finite() { ms.clamp(0.0, LARGEST_MS) } else { 0.0 };
        let bin = if ms >= OVERFLOW_MS {
            self.overflow_worst = self.overflow_worst.max(ms);
            BINS
        } else {
            (ms / BIN_MS) as usize
        };
        self.bins[bin] = self.bins[bin].saturating_add(1);
        self.count += 1;
    }

    /// The frame time that `p` (0..=1) of the frames do not exceed: the middle of its bin, so within half
    /// a bin. The overflow bin answers with its worst frame.
    pub fn percentile(&self, p: f64) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let rank = ((p * self.count as f64).ceil() as u64).clamp(1, self.count);
        let mut seen = 0u64;
        for (i, &n) in self.bins.iter().enumerate() {
            seen += u64::from(n);
            if seen >= rank {
                return if i == BINS { self.overflow_worst } else { (i as f32 + 0.5) * BIN_MS };
            }
        }
        self.overflow_worst
    }

    pub fn count(&self) -> u64 {
        self.count
    }
}

/// One screen's frames and the seconds spent there in the current window.
struct Slot {
    name: String,
    frames: Histogram,
    seconds: f64,
}

/// What a window holds: the wire's per-screen stats, the loads and the low-memory warnings.
pub struct Window {
    pub screens: Vec<ScreenStats>,
    pub loads: Vec<LoadTime>,
    pub low_memory: u32,
}

/// Collects the frames, the loads and the warnings of the current window.
#[derive(Default)]
pub struct Collector {
    slots: Vec<Slot>,
    last_at: Option<Duration>,
    loads: Vec<LoadTime>,
    low_memory: u32,
}

impl Collector {
    /// A frame at `at` (the time since the process start) that took `frame_ms` and drew `screen`.
    /// The time since the last frame counts on this screen, at most [`MAX_GAP`].
    pub fn frame(&mut self, at: Duration, frame_ms: f32, screen: &str) {
        let gap = self.last_at.map_or(Duration::ZERO, |last| at.saturating_sub(last)).min(MAX_GAP);
        self.last_at = Some(at);
        let i = self.slot(screen);
        let slot = &mut self.slots[i];
        slot.frames.record(frame_ms);
        slot.seconds += gap.as_secs_f64();
    }

    /// The slot of `name`. Past the screen limit, the names share the `other` slot.
    fn slot(&mut self, name: &str) -> usize {
        if let Some(i) = self.slots.iter().position(|s| s.name == name) {
            return i;
        }
        let name = if self.slots.len() < limits::MAX_SCREENS - 1 { name } else { OTHER };
        if let Some(i) = self.slots.iter().position(|s| s.name == name) {
            return i;
        }
        self.slots.push(Slot { name: name.to_string(), frames: Histogram::default(), seconds: 0.0 });
        self.slots.len() - 1
    }

    /// A load time (`start`, `map`, `battle_entry`); the first `MAX_LOADS` of a window are kept.
    pub fn load(&mut self, what: &str, ms: u32) {
        if self.loads.len() < limits::MAX_LOADS {
            self.loads.push(LoadTime { what: what.to_string(), ms });
        }
    }

    /// The platform called its low-memory warning once.
    pub fn low_memory_warning(&mut self) {
        self.low_memory = self.low_memory.saturating_add(1);
    }

    /// The window's numbers. The collector starts an empty window, and the next frame's gap is not counted.
    pub fn take(&mut self) -> Window {
        let screens = self
            .slots
            .drain(..)
            .map(|s| ScreenStats {
                screen: s.name,
                median_ms: s.frames.percentile(0.5),
                p99_ms: s.frames.percentile(0.99),
                p999_ms: s.frames.percentile(0.999),
                minutes: (s.seconds / 60.0) as f32,
            })
            .collect();
        self.last_at = None;
        Window { screens, loads: std::mem::take(&mut self.loads), low_memory: std::mem::take(&mut self.low_memory) }
    }
}

/// The report of a window: the device and the quality settings, the window's frames, loads and warnings, and
/// the peak RAM so far. `None` when the window holds nothing or the install id is not valid. The profiler's
/// top parts are not in it (the frame probe is off without profiling, so it has no stage times to send).
pub fn report(
    install_id: &str,
    device: &DeviceInfo,
    quality: &BTreeMap<String, String>,
    window: Window,
    peak_ram_mb: Option<u32>,
) -> Option<PerfReport> {
    if window.screens.is_empty() && window.loads.is_empty() {
        return None;
    }
    Some(PerfReport {
        schema: razdor_wire::SCHEMA,
        install_id: InstallId::parse(install_id).ok()?,
        razdor: env!("CARGO_PKG_VERSION").to_string(),
        device: device.clone(),
        quality: quality.clone(),
        screens: window.screens,
        loads: window.loads,
        memory: MemoryInfo { peak_ram_mb, peak_vram_mb: None, low_memory_warnings: window.low_memory },
        profile: Vec::new(),
    })
}

/// Queues a report for the server (`paths::PERF`, the consent class). Refused without a yes, or when the
/// body does not validate (nothing is queued then).
pub fn enqueue(hub: &Hub, report: &PerfReport) -> Result<u64, Refused> {
    hub.enqueue_checked(paths::PERF, Class::Consent, report)
}

/// The number of the `kB` line `field` (for example `VmHWM:`) of a `/proc` status text, in kB.
pub fn kb_field(text: &str, field: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with(field))?;
    line[field.len()..].split_whitespace().next()?.parse().ok()
}

/// The process's peak resident memory in MB (`VmHWM`). Linux only; `None` elsewhere.
pub fn peak_ram_mb() -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/self/status").ok()?;
        kb_field(&text, "VmHWM:").map(|kb| (kb / 1024).min(u64::from(u32::MAX)) as u32)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// The machine's total RAM in MB (`MemTotal`). Linux only; 0 elsewhere.
pub fn total_ram_mb() -> u32 {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|t| kb_field(&t, "MemTotal:"))
            .map_or(0, |kb| (kb / 1024).min(u64::from(u32::MAX)) as u32)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// The OS release (`/proc/sys/kernel/osrelease`). Empty where it is not read.
pub fn os_version() -> String {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/sys/kernel/osrelease").map(|s| s.trim().to_string()).unwrap_or_default()
    }
    #[cfg(not(target_os = "linux"))]
    {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::state::Consent;
    use razdor_wire::Validate;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn secs(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    fn device() -> DeviceInfo {
        DeviceInfo {
            os: "linux".into(),
            os_version: String::new(),
            gpu: String::new(),
            gpu_driver: String::new(),
            cpu_cores: 8,
            ram_mb: 16_000,
            screen_w: 1920,
            screen_h: 1080,
            scale_pct: 100,
            phone: None,
        }
    }

    fn quality() -> BTreeMap<String, String> {
        BTreeMap::from([("preset".to_string(), "high".to_string())])
    }

    fn hub(name: &str, consent: Consent) -> (Hub, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("razdor-perf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let hub = Hub::open(&dir, limits::CLIENT_QUEUE_BYTES).expect("the hub opens");
        if consent != Consent::Unknown {
            hub.set_consent(consent, "test");
        }
        (hub, dir)
    }

    #[test]
    fn percentiles_of_a_known_distribution_are_within_a_bin() {
        // 990 frames at 10 ms and 10 at 50 ms: the median and p99 are 10 ms, p99.9 is 50 ms.
        let mut h = Histogram::default();
        for _ in 0..990 {
            h.record(10.0);
        }
        for _ in 0..10 {
            h.record(50.0);
        }
        assert_eq!(h.count(), 1000);
        assert!((h.percentile(0.5) - 10.0).abs() <= BIN_MS);
        assert!((h.percentile(0.99) - 10.0).abs() <= BIN_MS);
        assert!((h.percentile(0.999) - 50.0).abs() <= BIN_MS);
    }

    #[test]
    fn percentiles_of_a_uniform_spread() {
        // 0.0, 0.1, ... 99.9 ms: the median is 50 ms, p99 98.9 ms, p99.9 99.8 ms.
        let mut h = Histogram::default();
        for i in 0..1000 {
            h.record(i as f32 * 0.1);
        }
        assert!((h.percentile(0.5) - 50.0).abs() <= BIN_MS);
        assert!((h.percentile(0.99) - 98.9).abs() <= BIN_MS);
        assert!((h.percentile(0.999) - 99.8).abs() <= BIN_MS);
    }

    #[test]
    fn overflow_frames_answer_with_their_worst() {
        let mut h = Histogram::default();
        for _ in 0..100 {
            h.record(250.0);
        }
        h.record(f32::NAN);
        assert!((h.percentile(0.99) - 250.0).abs() < 0.01);
        assert_eq!(Histogram::default().percentile(0.5), 0.0);
    }

    #[test]
    fn a_pause_counts_one_second_at_most() {
        let mut c = Collector::default();
        c.frame(secs(0.0), 10.0, "world map");
        c.frame(secs(10.0), 10.0, "world map");
        let w = c.take();
        assert_eq!(w.screens.len(), 1);
        assert!((f64::from(w.screens[0].minutes) * 60.0 - 1.0).abs() < 1e-4);
    }

    #[test]
    fn screens_past_the_limit_fold_into_other() {
        let mut c = Collector::default();
        for i in 0..20 {
            c.frame(secs(f64::from(i)), 5.0, &format!("screen {i}"));
        }
        let w = c.take();
        assert_eq!(w.screens.len(), limits::MAX_SCREENS);
        let other = w.screens.iter().find(|s| s.screen == OTHER).expect("the other slot");
        assert!(other.minutes > 0.0);
    }

    #[test]
    fn loads_are_capped() {
        let mut c = Collector::default();
        for _ in 0..40 {
            c.load("map", 100);
        }
        assert_eq!(c.take().loads.len(), limits::MAX_LOADS);
    }

    #[test]
    fn a_window_reports_only_its_own_frames_and_time() {
        let mut c = Collector::default();
        c.frame(secs(0.0), 10.0, "battle");
        c.frame(secs(2.0), 10.0, "battle");
        c.load("start", 1200);
        let first = c.take();
        assert!((f64::from(first.screens[0].minutes) * 60.0 - 1.0).abs() < 1e-4);
        assert!(c.take().screens.is_empty(), "the window was emptied");
        c.frame(secs(100.0), 20.0, "battle");
        c.frame(secs(101.0), 20.0, "battle");
        let second = c.take();
        assert!((f64::from(second.screens[0].minutes) * 60.0 - 1.0).abs() < 1e-4);
        assert!(second.loads.is_empty());
    }

    #[test]
    fn a_report_validates_and_is_queued_only_with_consent() {
        let mut c = Collector::default();
        c.frame(secs(0.0), 12.0, "world map");
        c.frame(secs(0.5), 13.0, "world map");
        c.load("start", 900);
        c.low_memory_warning();
        let report = report(ID, &device(), &quality(), c.take(), Some(512)).expect("a report");
        report.validate().expect("the report validates");
        assert_eq!(report.memory.low_memory_warnings, 1);
        assert!(report.profile.is_empty());

        let (no, dir_no) = hub("perf-no", Consent::No);
        assert_eq!(enqueue(&no, &report), Err(Refused::NoConsent));
        assert_eq!(no.queue().len(), 0);
        let _ = std::fs::remove_dir_all(&dir_no);

        let (yes, dir_yes) = hub("perf-yes", Consent::Yes);
        assert!(enqueue(&yes, &report).is_ok());
        let paths: Vec<String> = yes.queue().metas().map(|m| m.path.clone()).collect();
        assert_eq!(paths, vec![paths::PERF.to_string()]);
        let _ = std::fs::remove_dir_all(&dir_yes);
    }

    #[test]
    fn an_empty_window_or_a_bad_id_makes_no_report() {
        let mut c = Collector::default();
        assert!(report(ID, &device(), &quality(), c.take(), None).is_none());
        c.frame(secs(0.0), 12.0, "menu");
        assert!(report("not hex", &device(), &quality(), c.take(), None).is_none());
    }

    #[test]
    fn the_proc_fields_parse() {
        let status = "Name:\tRazdor\nVmHWM:\t  204800 kB\nVmRSS:\t 100 kB\n";
        assert_eq!(kb_field(status, "VmHWM:"), Some(204_800));
        assert_eq!(kb_field(status, "VmPeak:"), None);
    }
}
