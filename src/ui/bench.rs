//! Razdor's performance test («Тест производительности», the owner's request, 2026-11-12): a button
//! in Settings → «Прочее» that plays fixed map scenes and shows how fast they run on this device.
//!
//! Each cell is one configuration of the settings (the player's own, or one of the quality presets,
//! in 2.5D, and «Оригинал» in the flat view) on one map, near (the default zoom) or far (the whole
//! map on screen). A cell loads a new game of the map (fog explored), settles for [`SETTLE_SECS`],
//! then measures the frames for [`MEASURE_SECS`]. The settings apply for the run only: the player's
//! settings file is not written while the test runs (`audio::Settings::save`), and the player's game,
//! dialogs and screen come back at the end (or when the test is cancelled, Esc or the Back gesture).
//!
//! The run is driven from the app's frame: [`before_frame`] sets a cell up, the main loop's [`sample`]
//! hands each frame's work and total time over, and the cell's numbers go to the log and to logcat
//! (`RazdorBench key=value…`, then `RazdorBench DONE`), so a phone's lab run can be read from logcat
//! (`tools/perf/phone_bench.py`). Nothing here draws: the windows are in `bench_view`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

use macroquad::prelude::{screen_height, screen_width};
use razdor::i18n::{n_, tr};
use razdor::local;
use razdor::rules::content::HeroClass;
use razdor::rules::game::Game;

use super::audio::Settings;
use super::dialog::Dialog;
use super::quality_preset::{self, Preset};
use super::{App, Screen};

/// The settle before a cell's measure, and the measure, in seconds (the owner's numbers, 2026-11-12).
pub const SETTLE_SECS: f64 = 3.0;
pub const MEASURE_SECS: f64 = 12.0;
/// The pan cells (`Pos::Pan`, lab only: the trigger file's `pan` line): the camera moves for [`PAN_MOVE_SECS`], holds still for
/// [`PAN_HOLD_SECS`] (the ground's still frame, its Fill step), and moves again, from the settle's start to the measure's end.
/// The speed is in map cells per second (the view's world units: one cell's width at the default zoom): 19.2 cells a second,
/// one screen width per 2 s on the desktop's 1280-px window (38.4 cells across at the default zoom, `bench pan:` log line);
/// a phone shows fewer cells across, so the same speed covers more screens there. The path is the map's middle row, from the
/// clamp's left end to its right end and back.
pub const PAN_MOVE_SECS: f64 = 3.0;
pub const PAN_HOLD_SECS: f64 = 0.5;
pub const PAN_CELLS_PER_SEC: f64 = 19.2;
/// A frame longer than this is a slow one (a 30 fps frame, with a margin).
pub const SLOW_MS: f32 = 33.0;
/// The bench maps, the first the install has: the campaign's base map, then the first campaign map.
pub const MAPS: [&str; 2] = ["Другой берег", "РК1-Начало пути"];

/// What the player picked in the choice window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// The player's own settings and view («Мои настройки»).
    Mine,
    /// Every preset in 2.5D, and «Оригинал» in the flat view («Все пресеты»).
    All,
}

/// The two views of a map in a cell: the default zoom, or the whole map on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pos {
    Near,
    Far,
    /// The default zoom with the camera panning (the lab's `pan` line only: [`pan_cells`]).
    Pan,
}

/// One configuration of the settings: a preset (None: the player's own), and the view (None: the player's own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub key: &'static str,
    /// The label (a `tr` key: the English text, as the rest of the interface).
    pub label: &'static str,
    pub preset: Option<Preset>,
    pub tilt: Option<bool>,
}

/// The player's own settings.
pub const MINE: Config = Config { key: "mine", label: n_("My settings"), preset: None, tilt: None };

/// «Все пресеты»: the four presets in 2.5D, and «Оригинал» flat.
pub const ALL: [Config; 5] = [
    Config { key: "original-25d", label: n_("Original, 2.5D"), preset: Some(Preset::Original), tilt: Some(true) },
    Config { key: "low-25d", label: n_("Low, 2.5D"), preset: Some(Preset::Low), tilt: Some(true) },
    Config { key: "medium-25d", label: n_("Medium, 2.5D"), preset: Some(Preset::Medium), tilt: Some(true) },
    Config { key: "high-25d", label: n_("High, 2.5D"), preset: Some(Preset::High), tilt: Some(true) },
    Config { key: "original-flat", label: n_("Original, flat"), preset: Some(Preset::Original), tilt: Some(false) },
];

/// One cell of a run: a configuration, near or far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub config: Config,
    pub pos: Pos,
}

impl Slot {
    /// The stable id of the cell (`mine.near`, `high-25d.far`…): the log's `cell=` key.
    pub fn id(&self) -> String {
        let pos = match self.pos {
            Pos::Near => "near",
            Pos::Far => "far",
            Pos::Pan => "pan",
        };
        format!("{}.{pos}", self.config.key)
    }

    /// The cell's name for the window and the log (translated).
    pub fn name(&self) -> String {
        let pos = match self.pos {
            Pos::Near => tr("near"),
            Pos::Far => tr("whole map"),
            Pos::Pan => tr("panning"),
        };
        format!("{} · {pos}", tr(self.config.label))
    }
}

/// The cells of a choice: two per configuration, near then far.
pub fn plan(choice: Choice) -> Vec<Slot> {
    let configs: &[Config] = match choice {
        Choice::Mine => std::slice::from_ref(&MINE),
        Choice::All => &ALL,
    };
    configs.iter().flat_map(|&config| [Pos::Near, Pos::Far].map(|pos| Slot { config, pos })).collect()
}

/// The lab's pan cells (the trigger file's `pan` line, never the player's menu): «Оригинал» in 2.5D, then flat, panning.
pub fn pan_cells() -> Vec<Slot> {
    ALL.iter().filter(|c| c.key == "original-25d" || c.key == "original-flat").map(|&config| Slot { config, pos: Pos::Pan }).collect()
}

/// The cells of a run: the choice's cells, then the pan cells when the run asks for them.
pub fn cells_for(choice: Choice, pan: bool) -> Vec<Slot> {
    let mut cells = plan(choice);
    if pan {
        cells.extend(pan_cells());
    }
    cells
}

/// The pan's position in time: the seconds the camera has moved after `t` seconds from the settle's start. It moves for
/// [`PAN_MOVE_SECS`] of each [`PAN_MOVE_SECS`] + [`PAN_HOLD_SECS`] and stands still in the hold, so the value is the same
/// for every frame of a hold (the ground sees no motion there).
pub fn pan_moved_secs(t: f64) -> f64 {
    let cycle = PAN_MOVE_SECS + PAN_HOLD_SECS;
    let k = (t / cycle).floor();
    k * PAN_MOVE_SECS + (t - k * cycle).min(PAN_MOVE_SECS)
}

/// The pan's offset along its path of `range` cells, after `moved` seconds of motion at `speed` cells a second: a
/// ping-pong from the left end (0) to the right end (`range`) and back. Zero for no range (the map fits the view).
pub fn pan_offset(moved: f64, speed: f64, range: f64) -> f64 {
    if range <= 0.0 {
        return 0.0;
    }
    let u = (moved * speed).rem_euclid(2.0 * range);
    if u <= range {
        u
    } else {
        2.0 * range - u
    }
}

/// The settings a cell runs with: its configuration on the player's settings, and the run's own
/// switches (no frame cap, no battery saver, no game recording), which the player's settings
/// keep for after the test.
pub fn configure(s: &mut Settings, cell: &Slot) {
    if let Some(p) = cell.config.preset {
        quality_preset::set(s, p);
    }
    if let Some(tilt) = cell.config.tilt {
        s.map_tilt = tilt;
        if tilt {
            // The 2.5D cells are the third-person view, as the default one.
            s.map_first_person = false;
            s.map_eyes = false;
            s.map_horizon = false;
            s.map_globe = false;
        }
    }
    s.frame_cap = 0;
    s.battery_saver = false;
    s.record_game = false;
}

/// The view a settings value names in the log (`25d` or `flat`).
pub fn view_name(s: &Settings) -> &'static str {
    if s.map_tilt {
        "25d"
    } else {
        "flat"
    }
}

/// The nearest-rank percentile (`p` in 0..=1) of sorted values; 0 for none.
pub fn percentile(sorted: &[f32], p: f64) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// One cell's numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub frames: usize,
    /// The measure's length in seconds, as it ran.
    pub secs: f64,
    /// Frames per second over the measure.
    pub fps: f64,
    /// Frame time (the whole frame, vsync included), median, 90th and 99th percentile, in ms.
    pub frame_med: f32,
    pub frame_p90: f32,
    pub frame_p99: f32,
    /// The game's work per frame (before drawing), median, in ms.
    pub cpu_med: f32,
    /// Frames longer than [`SLOW_MS`].
    pub slow: usize,
    /// The load of the cell's game, in ms.
    pub load_ms: f32,
    /// The process's resident memory at the cell's end, when the platform gives it.
    pub rss_mb: Option<f32>,
}

/// Each frame's (work ms, total ms) over the measure, and the measure's length and the load and memory.
pub fn stats(frames: &[(f32, f32)], secs: f64, load_ms: f32, rss_mb: Option<f32>) -> Stats {
    let mut total: Vec<f32> = frames.iter().map(|f| f.1).collect();
    let mut work: Vec<f32> = frames.iter().map(|f| f.0).collect();
    total.sort_by(f32::total_cmp);
    work.sort_by(f32::total_cmp);
    Stats {
        frames: frames.len(),
        secs,
        fps: if secs > 0.0 { frames.len() as f64 / secs } else { 0.0 },
        frame_med: percentile(&total, 0.5),
        frame_p90: percentile(&total, 0.9),
        frame_p99: percentile(&total, 0.99),
        cpu_med: percentile(&work, 0.5),
        slow: frames.iter().filter(|f| f.1 > SLOW_MS).count(),
        load_ms,
        rss_mb,
    }
}

/// The device the results are for: written in every result line, so one line is enough to read.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub version: String,
    /// The window's size in pixels.
    pub screen: (u32, u32),
    /// The GL renderer's string.
    pub gpu: String,
    pub os: String,
    /// The vertical sync: as the device sets it (Razdor does not change it).
    pub vsync: String,
}

/// A pan cell's counts over its measure: the ground cache's steps (`terrain::ground_steps`: moving, Fill, Scroll) and the
/// frames whose drawn camera moved (`world_view::drawn_look`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Motion {
    pub fills: u64,
    pub scrolls: u64,
    pub moving: u64,
    pub cam_moves: u32,
}

/// One finished cell.
#[derive(Clone, Debug, PartialEq)]
pub struct CellResult {
    pub id: String,
    pub name: String,
    /// The view the cell ran in (`25d` or `flat`).
    pub view: &'static str,
    pub stats: Stats,
    /// The cell's stage breakdown (`frame_probe::stages_payload`), when the run had profiling on.
    pub stages: Option<String>,
    /// The pan cells' counts (None for the other cells, whose line is unchanged).
    pub motion: Option<Motion>,
}

/// A quoted text value of a log line (its quotes become apostrophes).
pub fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "'"))
}

/// The log line of one finished cell: `RazdorBench key=value…`, the values with spaces quoted.
pub fn result_line(r: &CellResult, dev: &Device) -> String {
    let s = &r.stats;
    let rss = s.rss_mb.map_or("na".to_string(), |v| format!("{v:.0}"));
    let motion = r.motion.map_or(String::new(), |m| format!(" fills={} scrolls={} moving={} cam_moves={}", m.fills, m.scrolls, m.moving, m.cam_moves));
    format!(
        "RazdorBench cell={} name={} view={} pos={} fps={:.1} frames={} secs={:.1} frame_ms_med={:.2} frame_ms_p90={:.2} frame_ms_p99={:.2} cpu_ms_med={:.2} slow={} load_ms={:.0} rss_mb={}{motion} vsync={} screen={}x{} gpu={} os={} version={}",
        r.id,
        quoted(&r.name),
        r.view,
        r.id.rsplit('.').next().unwrap_or("near"),
        s.fps,
        s.frames,
        s.secs,
        s.frame_med,
        s.frame_p90,
        s.frame_p99,
        s.cpu_med,
        s.slow,
        s.load_ms,
        rss,
        quoted(&dev.vsync),
        dev.screen.0,
        dev.screen.1,
        quoted(&dev.gpu),
        quoted(&dev.os),
        dev.version,
    )
}

/// The log line of one cell's stage breakdown (`RazdorBenchStages cell=… <stage>_p50=…`), or None when the cell
/// ran without profiling. It is a second tag beside `RazdorBench`, so the cell line's format stays as it was.
pub fn stages_line(r: &CellResult) -> Option<String> {
    r.stages.as_ref().map(|payload| format!("RazdorBenchStages {payload}"))
}

/// What a finished run shows and saves.
#[derive(Clone, Debug)]
pub struct Report {
    pub choice: Choice,
    pub device: Device,
    /// The map the cells played.
    pub map: String,
    pub cells: Vec<CellResult>,
    /// The saved file, when it was saved.
    pub file: Option<PathBuf>,
    /// The text went to the clipboard.
    pub copied: bool,
    /// Why the file was not saved, when it was not.
    pub error: Option<String>,
    /// The header lines after the vertical sync: the device, the display, the render size and the settings
    /// (`perf_log::bench_header`).
    pub setup: Vec<String>,
}

/// The text of a saved report: a readable summary, then the log lines (what `phone_bench.py` reads).
pub fn report_text(r: &Report) -> String {
    let mut out = String::new();
    out.push_str("Razdor: performance test\n");
    out.push_str(&format!("Choice: {}\n", if r.choice == Choice::Mine { "my settings" } else { "all presets" }));
    out.push_str(&format!("Map: {}\n", r.map));
    out.push_str(&format!("Renderer: {}\n", r.device.gpu));
    out.push_str(&format!("Screen: {}x{}\n", r.device.screen.0, r.device.screen.1));
    out.push_str(&format!("Razdor {}; OS: {}\n", r.device.version, r.device.os));
    out.push_str(&format!("Vertical sync: {}\n", r.device.vsync));
    for line in &r.setup {
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    out.push_str("Cell | FPS | frame ms median | frame ms p99 | slow frames\n");
    for c in &r.cells {
        out.push_str(&format!("{} | {:.1} | {:.1} | {:.1} | {}\n", c.name, c.stats.fps, c.stats.frame_med, c.stats.frame_p99, c.stats.slow));
    }
    out.push('\n');
    for c in &r.cells {
        out.push_str(&result_line(c, &r.device));
        out.push('\n');
        if let Some(line) = stages_line(c) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push_str("RazdorBench DONE\n");
    out
}

/// The file name of a saved report, from the time it was saved (seconds since the epoch, UTC).
pub fn file_name(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86_400) as i64;
    let rest = epoch_secs % 86_400;
    let (y, m, d) = civil(days);
    format!("razdor-bench-{y:04}-{m:02}-{d:02}-{:02}{:02}{:02}.txt", rest / 3600, rest / 60 % 60, rest % 60)
}

/// The civil date of a day count from 1970-01-01 (Howard Hinnant's algorithm).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// The phases the window shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Closed,
    /// The choice window; the error of the last try, if any (no install, no map).
    Choice(Option<String>),
    Running,
    Done,
}

/// Where the run is: the cell being measured (its index, its name, the phase's name and its elapsed seconds).
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub index: usize,
    pub total: usize,
    pub name: String,
    pub measuring: bool,
    pub elapsed: f64,
}

/// The player's state while the test runs: restored at its end.
struct Saved {
    settings: Settings,
    game: Option<Game>,
    dialogs: VecDeque<Dialog>,
    message: Option<String>,
}

/// A cell's stage: its game is being built, the frames settle, or the frames are measured.
enum Stage {
    Setup,
    Loading(Instant),
    Settle(Instant),
    Measure { start: Instant, frames: Vec<(f32, f32)> },
}

/// A run in progress. A `fake` run is a snapshot's stand-in: it draws, and never plays.
struct Run {
    choice: Choice,
    cells: Vec<Slot>,
    index: usize,
    stage: Stage,
    results: Vec<CellResult>,
    /// Read at the first measured cell, once a frame has been shown (the GL string needs one).
    device: Option<Device>,
    map: String,
    map_index: usize,
    load_ms: f32,
    saved: Option<Saved>,
    fake: bool,
    /// The run profiles itself (`frame_probe`'s cells, the stage lines): the trigger file's `profile` line.
    profile: bool,
    /// A pan cell's clock: when its settle began (the camera's motion counts from here), and its path (set on that frame).
    pan_origin: Option<Instant>,
    pan_path: Option<super::world_view::PanPath>,
    /// The measure's camera: the look last measured, the frames it moved, and the ground's steps when the measure began.
    last_look: Option<(f32, f32)>,
    cam_moves: u32,
    steps_at: (u64, u64, u64),
}

impl Run {
    /// The next cell starts: its pan clock and its camera counts start over.
    fn next_cell(&mut self) {
        self.pan_origin = None;
        self.pan_path = None;
        self.last_look = None;
        self.cam_moves = 0;
    }
}

enum Phase {
    Closed,
    Choice(Option<String>),
    Running(Box<Run>),
    Done(Box<Report>),
}

local! {
    static PHASE: RefCell<Phase> = const { RefCell::new(Phase::Closed) };
    /// The end-of-run screenshot's path (`RAZDOR_BENCH_END`), taken once when the run is done.
    static END_SHOT: Cell<bool> = const { Cell::new(false) };
    /// The mid-run picture was taken (`take_mid_shot`).
    static MID_SHOT: Cell<bool> = const { Cell::new(false) };
}

/// The test is on: its windows are up, or a run is playing. Input is held and the player's
/// settings and files are not written while it is.
pub fn running() -> bool {
    PHASE.with_borrow(|p| matches!(p, Phase::Running(_)))
}

/// Any of the test's windows is up (the choice, the run or the results): the screen below takes no input.
pub fn window_open() -> bool {
    PHASE.with_borrow(|p| !matches!(p, Phase::Closed))
}

/// The phase, for the windows to draw.
pub fn kind() -> Kind {
    PHASE.with_borrow(|p| match p {
        Phase::Closed => Kind::Closed,
        Phase::Choice(e) => Kind::Choice(e.clone()),
        Phase::Running(_) => Kind::Running,
        Phase::Done(_) => Kind::Done,
    })
}

/// The run's progress, for its window.
pub fn progress() -> Option<Progress> {
    PHASE.with_borrow(|p| match p {
        Phase::Running(r) => {
            let (measuring, elapsed) = match &r.stage {
                Stage::Settle(t) => (false, t.elapsed().as_secs_f64()),
                Stage::Measure { start, .. } => (true, start.elapsed().as_secs_f64()),
                _ => (false, 0.0),
            };
            Some(Progress { index: r.index, total: r.cells.len(), name: r.cells.get(r.index).map(Slot::name).unwrap_or_default(), measuring, elapsed })
        }
        _ => None,
    })
}

/// A copy of the finished report, for its window.
pub fn report() -> Option<Report> {
    PHASE.with_borrow(|p| match p {
        Phase::Done(r) => Some((**r).clone()),
        _ => None,
    })
}

/// Opens the choice window (the settings' «Запустить»).
pub fn open_choice() {
    PHASE.with_borrow_mut(|p| {
        if matches!(p, Phase::Closed) {
            *p = Phase::Choice(None);
        }
    });
}

/// Closes the test's window without a run (the choice's «Отмена», the results' «Закрыть»).
pub fn close() {
    PHASE.with_borrow_mut(|p| {
        if !matches!(p, Phase::Running(_)) {
            *p = Phase::Closed;
        }
    });
}

/// Takes the run out of its phase while it is worked on (`put_run` gives it back).
fn take_run() -> Option<Box<Run>> {
    PHASE.with_borrow_mut(|p| match std::mem::replace(p, Phase::Closed) {
        Phase::Running(r) => Some(r),
        other => {
            *p = other;
            None
        }
    })
}

fn put_run(run: Box<Run>) {
    PHASE.with_borrow_mut(|p| *p = Phase::Running(run));
}

/// Milliseconds of a duration, as the frame numbers are.
fn ms(d: Duration) -> f32 {
    d.as_secs_f32() * 1000.0
}

/// The GL renderer's string (after the first frame is shown), or `unknown`.
pub fn gpu_renderer() -> String {
    super::perf_log::gl_string(super::perf_log::GL_RENDERER)
}

/// The operating system's name: the release's pretty name on Linux, else the target's.
pub fn os_name() -> String {
    if let Ok(text) = std::fs::read_to_string("/etc/os-release") {
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("PRETTY_NAME=") {
                return v.trim_matches('"').to_string();
            }
        }
    }
    std::env::consts::OS.to_string()
}

/// The device of a snapshot's stand-in results (no GL call before a frame).
fn fake_device() -> Device {
    Device { version: env!("CARGO_PKG_VERSION").to_string(), screen: (1920, 1080), gpu: "NVIDIA GeForce RTX 4060/PCIe/SSE2".into(), os: "Fedora Linux 44".into(), vsync: "device".into() }
}

/// The device's line for the results.
pub fn device() -> Device {
    let vsync = match std::env::var("__GL_SYNC_TO_VBLANK") {
        Ok(v) if !cfg!(target_os = "android") => format!("device (__GL_SYNC_TO_VBLANK={v})"),
        _ => n_("device").to_string(),
    };
    Device {
        version: env!("CARGO_PKG_VERSION").to_string(),
        screen: (screen_width().round().max(0.0) as u32, screen_height().round().max(0.0) as u32),
        gpu: gpu_renderer(),
        os: os_name(),
        vsync,
    }
}

/// The process's resident memory in MB (Linux and Android, from `/proc`), or `None`.
pub fn rss_mb() -> Option<f32> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = text.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: f32 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024.0)
}

/// The map a run plays: the first of [`MAPS`] the install has, else the install's first map.
fn map_index(app: &App) -> Option<usize> {
    MAPS.iter().find_map(|name| app.scenarios.iter().position(|e| e.file == *name)).or((!app.scenarios.is_empty()).then_some(0))
}

/// Writes one log line to the log file and, on Android, to logcat (tag `RazdorBench`).
fn log_line(line: &str) {
    razdor::diag::log(line);
    logcat(line);
}

#[cfg(target_os = "android")]
fn logcat(line: &str) {
    use std::os::raw::{c_char, c_int};
    extern "C" {
        fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    }
    let tag = std::ffi::CString::new("RazdorBench").unwrap_or_default();
    let text = std::ffi::CString::new(line.replace('\0', " ")).unwrap_or_default();
    // SAFETY: both strings are NUL-terminated and live through the call; prio 4 is INFO.
    unsafe {
        __android_log_write(4, tag.as_ptr(), text.as_ptr());
    }
}

#[cfg(not(target_os = "android"))]
fn logcat(_line: &str) {}

/// Starts a run of `choice` from the settings: the player's state is kept, and the first cell is
/// set up on the next frame. Errors (no install, no map) are returned for the choice window.
pub fn start(app: &mut App, choice: Choice) -> Result<(), String> {
    start_on(app, choice, None)
}

/// [`start`] on the named map (its file name in the game: `bench_autorun`'s `map=` line), or on the
/// default one when None.
pub fn start_on(app: &mut App, choice: Choice, map: Option<&str>) -> Result<(), String> {
    start_run(app, choice, map, false, false)
}

/// [`start_on`] with the run's profiling and its pan cells: `profile` turns profiling on for this run only (its frames' stage
/// lines, `RazdorBenchStages`, and the debug mode's GPU samples), as the trigger file's `profile` line asks; `pan` adds the
/// lab's pan cells after the choice's (`pan_cells`), as its `pan` line asks.
pub fn start_run(app: &mut App, choice: Choice, map: Option<&str>, profile: bool, pan: bool) -> Result<(), String> {
    let Some(_) = app.dt_content.clone() else {
        return Err(tr("The test needs the game: the Discord Times folder.").to_string());
    };
    let map_index = match map {
        Some(name) => app.scenarios.iter().position(|e| e.file == name).ok_or_else(|| format!("no map {name} in the game"))?,
        None => map_index(app).ok_or_else(|| tr("No map of the game was found.").to_string())?,
    };
    let cells = cells_for(choice, pan);
    let map = app.scenarios[map_index].file.clone();
    let saved = Saved {
        settings: app.audio.settings.clone(),
        game: app.game.take(),
        dialogs: std::mem::take(&mut app.dialogs),
        message: app.message.take(),
    };
    keep_awake(true);
    razdor::diag::set_forced_profile(profile);
    let profiled = if profile { " profile" } else { "" };
    let panned = if pan { " pan" } else { "" };
    let switches = razdor::lab::current().describe();
    let switches = if switches.is_empty() { String::new() } else { format!(" lab={switches}") };
    log_line(&format!("RazdorBench START choice={} cells={} map={}{profiled}{panned}{switches}", if choice == Choice::Mine { "mine" } else { "all" }, cells.len(), quoted(&map)));
    let run = Run {
        choice,
        cells,
        index: 0,
        stage: Stage::Setup,
        results: Vec::new(),
        device: None,
        map,
        map_index,
        load_ms: 0.0,
        saved: Some(saved),
        fake: false,
        profile,
        pan_origin: None,
        pan_path: None,
        last_look: None,
        cam_moves: 0,
        steps_at: (0, 0, 0),
    };
    PHASE.with_borrow_mut(|p| *p = Phase::Running(Box::new(run)));
    Ok(())
}

/// The screen stays awake for the run (Android: the activity's window flag; the desktop keeps its own setting,
/// Razdor does not hold the desktop's screen saver off).
fn keep_awake(on: bool) {
    #[cfg(target_os = "android")]
    super::android::keep_awake(on);
    #[cfg(not(target_os = "android"))]
    let _ = on;
}

/// Puts the player's settings, game, dialogs and message back, and the map's view and the screen
/// to the settings window.
fn restore(app: &mut App, saved: Option<Saved>) {
    super::world_view::set_bench_fit(false);
    keep_awake(false);
    razdor::diag::set_forced_profile(false);
    // The lab's switches go back to the environment's (a run's trigger file set them for this run only).
    razdor::lab::set(razdor::lab::Flags::from_env());
    // A cell cut short (Esc, Cancel, a quit) keeps no samples: its probe cell ends here (a no-op after a whole run).
    let _ = razdor::frame_probe::cell_end();
    if let Some(s) = saved {
        app.audio.settings = s.settings;
        app.game = s.game;
        app.dialogs = s.dialogs;
        app.message = s.message;
    }
    app.screen = Screen::Options;
}

/// Sets the cell up: its settings, its view (the whole map or the default zoom), a new game of the
/// map with its fog explored.
fn setup(app: &mut App, run: &mut Run) {
    let Some(cell) = run.cells.get(run.index).copied() else { return };
    let Some(content) = app.dt_content.clone() else { return };
    let t0 = Instant::now();
    configure(&mut app.audio.settings, &cell);
    super::world_view::set_bench_fit(cell.pos == Pos::Far);
    let content = Arc::new(content.with_experimental(super::new_game::experimental_rules()));
    let mut game = Game::from_scenario(content, &app.scenarios[run.map_index].scenario, HeroClass::Knight);
    game.seed_events(0x5EED);
    for y in 0..game.world.map.h {
        for x in 0..game.world.map.w {
            game.fog.mark((x, y));
        }
    }
    app.map_view.open_around_hero(&game);
    app.map_view.reset();
    app.map_view.forget_map();
    app.game = Some(game);
    app.dialogs.clear();
    app.message = None;
    app.screen = Screen::WorldMap;
    run.stage = Stage::Loading(t0);
}

/// Each frame before the screen's: a cell is set up, and the map's windows that open during a cell
/// (an event's, the start's messages) are closed as the screenshots do, so nothing waits for a click.
pub fn before_frame(app: &mut App) {
    let Some(mut run) = take_run() else { return };
    if !run.fake {
        if matches!(run.stage, Stage::Setup) {
            setup(app, &mut run);
        } else {
            app.dialogs.clear();
            if let Some(game) = app.game.as_mut() {
                if game.event_window().is_some() {
                    let _ = game.event_window_closed();
                    game.drain_events();
                }
            }
        }
        pan_frame(app, &mut run);
    }
    put_run(run);
}

/// Each frame of a pan cell's settle and measure: the camera is set to where the pan path has it after the time since the
/// settle began (`pan_moved_secs`, `pan_offset`). The path's ends and the speed are logged once per cell.
fn pan_frame(app: &mut App, run: &mut Run) {
    let Some(cell) = run.cells.get(run.index).copied() else { return };
    if cell.pos != Pos::Pan || !matches!(run.stage, Stage::Settle(_) | Stage::Measure { .. }) {
        return;
    }
    let Some(game) = app.game.as_ref() else { return };
    let now = Instant::now();
    let origin = *run.pan_origin.get_or_insert(now);
    let path = *run.pan_path.get_or_insert_with(|| {
        let path = super::world_view::bench_pan_path(game, app.map_view.zoom);
        razdor::diag::log(&format!(
            "bench pan: cell={} from={:.2} to={:.2} row={:.2} view_cells={:.2} speed={PAN_CELLS_PER_SEC} cells/s move={PAN_MOVE_SECS}s hold={PAN_HOLD_SECS}s",
            cell.id(),
            path.from,
            path.to,
            path.row,
            path.view_cells
        ));
        path
    });
    let t = now.duration_since(origin).as_secs_f64();
    let range = f64::from(path.to - path.from);
    let x = f64::from(path.from) + pan_offset(pan_moved_secs(t), PAN_CELLS_PER_SEC, range);
    super::world_view::bench_pan_look(game, &mut app.map_view, (x as f32, path.row));
}

/// Each frame after the work and the total time are known: the frame is measured if the cell is
/// measuring, and the stages move on by the clock. A cell's end logs its line; the last one ends the run.
pub fn sample(app: &mut App, work: Duration, total: Duration) {
    let Some(mut run) = take_run() else { return };
    if run.fake {
        put_run(run);
        return;
    }
    let now = Instant::now();
    if matches!(run.stage, Stage::Measure { .. }) {
        let look = super::world_view::drawn_look();
        if look != run.last_look {
            run.cam_moves += 1;
            run.last_look = look;
        }
    }
    let next = match &mut run.stage {
        Stage::Setup => None,
        Stage::Loading(t0) => {
            run.load_ms = ms(t0.elapsed());
            run.device.get_or_insert_with(device);
            Some(Stage::Settle(now))
        }
        Stage::Settle(since) => (now.duration_since(*since).as_secs_f64() >= SETTLE_SECS).then(|| Stage::Measure { start: now, frames: Vec::new() }),
        Stage::Measure { frames, .. } => {
            frames.push((ms(work), ms(total)));
            None
        }
    };
    if let Some(stage) = next {
        let measuring = matches!(stage, Stage::Measure { .. });
        run.stage = stage;
        if measuring && run.profile {
            razdor::frame_probe::cell_begin();
        }
        if measuring {
            run.steps_at = super::terrain::ground_steps();
            run.cam_moves = 0;
            run.last_look = super::world_view::drawn_look();
        }
    }
    let measure_done = match &run.stage {
        Stage::Measure { start, .. } => now.duration_since(*start).as_secs_f64() >= MEASURE_SECS,
        _ => false,
    };
    if !measure_done {
        put_run(run);
        return;
    }
    let Stage::Measure { start, frames } = std::mem::replace(&mut run.stage, Stage::Setup) else { return };
    let secs = start.elapsed().as_secs_f64();
    let cell = run.cells[run.index];
    let st = stats(&frames, secs, run.load_ms, rss_mb());
    // The probe's samples of the cell (empty when the run is not profiled): the stage breakdown of its frames.
    let samples = if run.profile { razdor::frame_probe::cell_end() } else { Vec::new() };
    let stages = (!samples.is_empty()).then(|| razdor::frame_probe::stages_payload(&cell.id(), &samples));
    let motion = (cell.pos == Pos::Pan).then(|| {
        let (moving, fills, scrolls) = super::terrain::ground_steps();
        Motion {
            fills: fills.saturating_sub(run.steps_at.1),
            scrolls: scrolls.saturating_sub(run.steps_at.2),
            moving: moving.saturating_sub(run.steps_at.0),
            cam_moves: run.cam_moves,
        }
    });
    let result = CellResult { id: cell.id(), name: cell.name(), view: view_name(&app.audio.settings), stats: st, stages, motion };
    let dev = run.device.get_or_insert_with(device);
    log_line(&result_line(&result, dev));
    if let Some(line) = stages_line(&result) {
        log_line(&line);
    }
    run.results.push(result);
    run.index += 1;
    run.next_cell();
    if run.index < run.cells.len() {
        put_run(run);
        return;
    }
    // The last cell: the player's state comes back, and the report waits in the results window.
    log_line("RazdorBench DONE");
    restore(app, run.saved.take());
    let mut report = Report {
        choice: run.choice,
        device: run.device.unwrap_or_else(device),
        map: run.map,
        cells: run.results,
        file: None,
        copied: false,
        error: None,
        setup: super::perf_log::bench_header(&app.audio.settings),
    };
    // `RAZDOR_BENCH_SAVE` (a lab's or a test's run, with no one to press Save): the file is saved at once.
    // A run from the trigger file (`bench_autorun`) is saved too, and its end is written there (`finished`).
    let from_file = super::bench_autorun::from_file();
    if from_file || std::env::var_os("RAZDOR_BENCH_SAVE").is_some() {
        let file = save(&report);
        report.copied = false;
        match file {
            Ok(path) => report.file = Some(path),
            Err(e) => report.error = Some(e),
        }
    }
    if from_file {
        let result = match (&report.file, &report.error) {
            (Some(path), _) => Ok(path.clone()),
            (None, error) => Err(error.clone().unwrap_or_else(|| "the results were not saved".to_string())),
        };
        super::bench_autorun::finished(result);
    }
    PHASE.with_borrow_mut(|p| *p = Phase::Done(Box::new(report)));
}

/// The choice window's message: why the run did not start.
pub fn choice_error(e: String) {
    PHASE.with_borrow_mut(|p| {
        if let Phase::Choice(err) = p {
            *err = Some(e);
        }
    });
}

/// Stops the run now (Esc, the Back gesture, the window's Cancel): the player's state comes back,
/// and nothing is kept.
pub fn cancel(app: &mut App) {
    if let Some(mut run) = take_run() {
        log_line("RazdorBench CANCELLED");
        restore(app, run.saved.take());
        super::bench_autorun::finished(Err("the run was cancelled".to_string()));
    }
    PHASE.with_borrow_mut(|p| *p = Phase::Closed);
}

/// The app quits during a run: the player's settings come back before they are written.
pub fn abort(app: &mut App) {
    if let Some(mut run) = take_run() {
        restore(app, run.saved.take());
    }
    PHASE.with_borrow_mut(|p| *p = Phase::Closed);
}

/// Esc (or Back) during the test: the choice and the results close, a run is cancelled. True when
/// the key was used, so the screen below does not act on it.
pub fn handle_escape(app: &mut App) -> bool {
    if !super::widgets::esc_pressed() {
        return false;
    }
    match kind() {
        Kind::Closed => false,
        Kind::Choice(_) | Kind::Done => {
            close();
            true
        }
        Kind::Running => {
            cancel(app);
            true
        }
    }
}

/// Saves the report's text into the app's folder (`platform::app_dir`), and copies it to the
/// clipboard where that works. Returns the file or why it was not written.
pub fn save(report: &Report) -> Result<PathBuf, String> {
    let dir = razdor::platform::app_dir().ok_or_else(|| tr("No folder to save into.").to_string())?;
    let epoch = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(file_name(epoch));
    std::fs::write(&path, report_text(report)).map_err(|e| e.to_string())?;
    // The cloud keeps the report pending until it is sent (`ui::cloud_store`); nothing is sent here.
    super::cloud_store::bench_finished(&cloud_report(report, epoch));
    Ok(path)
}

/// The report as the cloud carries it (`razdor_wire::cloud::BenchReport`): the device, the settings
/// as the setup lines, and each cell's median and 99th-percentile frame time.
pub fn cloud_report(r: &Report, at: u64) -> razdor_wire::cloud::BenchReport {
    let device = razdor_wire::analytics::DeviceInfo {
        os: r.device.os.clone(),
        os_version: String::new(),
        gpu: r.device.gpu.clone(),
        gpu_driver: String::new(),
        cpu_cores: std::thread::available_parallelism().map_or(1, |n| n.get() as u32),
        ram_mb: 0,
        screen_w: r.device.screen.0,
        screen_h: r.device.screen.1,
        scale_pct: 100,
        phone: None,
    };
    razdor_wire::cloud::BenchReport {
        schema: razdor_wire::SCHEMA,
        razdor: r.device.version.clone(),
        at,
        device,
        settings: r.setup.iter().enumerate().map(|(i, line)| (format!("setup{i}"), line.clone())).collect(),
        scenes: r
            .cells
            .iter()
            .map(|c| razdor_wire::cloud::BenchScene {
                name: c.name.clone(),
                median_ms: c.stats.frame_med,
                p99_ms: c.stats.frame_p99,
                frames: c.stats.frames as u32,
            })
            .collect(),
    }
}

/// Records the saved file (or why it failed) and the clipboard's answer on the shown report.
pub fn note_saved(file: Result<PathBuf, String>, copied: bool) {
    PHASE.with_borrow_mut(|p| {
        if let Phase::Done(r) = p {
            match file {
                Ok(path) => {
                    r.file = Some(path);
                    r.copied = copied;
                }
                Err(e) => r.error = Some(e),
            }
        }
    });
}

/// The mid-run picture (`RAZDOR_BENCH_MID=<png>` with `RAZDOR_BENCH_MID_CELL=<n>`, a test's check of a cell's
/// view): true once, while cell `n` is being measured.
pub fn take_mid_shot() -> bool {
    let Some(want) = std::env::var("RAZDOR_BENCH_MID_CELL").ok().and_then(|v| v.trim().parse::<usize>().ok()) else {
        return false;
    };
    if std::env::var_os("RAZDOR_BENCH_MID").is_none() {
        return false;
    }
    let measuring = PHASE.with_borrow(|p| matches!(p, Phase::Running(r) if r.index == want && matches!(r.stage, Stage::Measure { .. })));
    measuring && !MID_SHOT.with(|s| s.replace(true))
}

/// The end-of-run picture (`RAZDOR_BENCH_END=<png>`): true once, when the results are up.
pub fn take_end_shot() -> bool {
    if std::env::var_os("RAZDOR_BENCH_END").is_none() {
        return false;
    }
    let done = PHASE.with_borrow(|p| matches!(p, Phase::Done(_)));
    done && !END_SHOT.with(|s| s.replace(true))
}

/// Stand-ins for the snapshots (`RAZDOR_SCENE=bench:choice|running|results`): the windows as they
/// look, with no run and no numbers of a run.
pub fn snapshot_stage(which: &str) {
    match which {
        "running" => {
            let run = Run {
                choice: Choice::All,
                cells: plan(Choice::All),
                index: 3,
                stage: Stage::Setup,
                results: Vec::new(),
                device: Some(fake_device()),
                map: MAPS[0].to_string(),
                map_index: 0,
                load_ms: 0.0,
                saved: None,
                fake: true,
                profile: false,
                pan_origin: None,
                pan_path: None,
                last_look: None,
                cam_moves: 0,
                steps_at: (0, 0, 0),
            };
            PHASE.with_borrow_mut(|p| *p = Phase::Running(Box::new(run)));
        }
        "results" => {
            let cells = plan(Choice::All);
            let results: Vec<CellResult> = cells
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let frames: Vec<(f32, f32)> = (0..120).map(|f| {
                        let t = 14.0 + ((f * 7 + i * 3) % 13) as f32 * 0.4;
                        (t * 0.4, t)
                    }).collect();
                    CellResult { id: c.id(), name: c.name(), view: if c.config.tilt == Some(false) { "flat" } else { "25d" }, stats: stats(&frames, MEASURE_SECS, 1200.0 + i as f32 * 90.0, Some(540.0)), stages: None, motion: None }
                })
                .collect();
            let report = Report {
                choice: Choice::All,
                device: fake_device(),
                map: MAPS[0].to_string(),
                cells: results,
                file: None,
                copied: false,
                error: None,
                setup: Vec::new(),
            };
            PHASE.with_borrow_mut(|p| *p = Phase::Done(Box::new(report)));
        }
        _ => PHASE.with_borrow_mut(|p| *p = Phase::Choice(None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_is_two_cells_per_configuration() {
        let mine = plan(Choice::Mine);
        assert_eq!(mine.iter().map(Slot::id).collect::<Vec<_>>(), ["mine.near", "mine.far"]);
        let all = plan(Choice::All);
        assert_eq!(all.len(), 10, "five configurations, near and far");
        assert_eq!(all[0].id(), "original-25d.near");
        assert_eq!(all[9].id(), "original-flat.far");
        assert!(all.iter().all(|c| c.config.tilt.is_some()), "every preset cell has its view");
    }

    #[test]
    fn a_cell_takes_its_preset_and_view_and_the_run_switches() {
        let mut s = Settings { map_first_person: true, frame_cap: 60, battery_saver: true, record_game: true, ..Settings::default() };
        configure(&mut s, &Slot { config: ALL[4], pos: Pos::Near });
        assert!(!s.map_tilt, "the flat view");
        assert_eq!((s.frame_cap, s.battery_saver, s.record_game), (0, false, false));
        configure(&mut s, &Slot { config: ALL[3], pos: Pos::Far });
        assert!(s.map_tilt && !s.map_first_person, "2.5D is the third-person view");
        assert_eq!(quality_preset::current(&s), Some(Preset::High));
    }

    #[test]
    fn my_settings_keep_the_players_look() {
        let mut s = Settings { map_tilt: true, map_first_person: true, frame_cap: 30, ..Settings::default() };
        configure(&mut s, &Slot { config: MINE, pos: Pos::Far });
        assert!(s.map_tilt && s.map_first_person, "the player's own view");
        assert_eq!(s.frame_cap, 0);
    }

    #[test]
    fn a_finished_report_becomes_a_valid_cloud_body() {
        use razdor_wire::Validate;
        let cell = |id: &str, name: &str| CellResult {
            id: id.into(),
            name: name.into(),
            view: "25d",
            stats: stats(&[(1.0, 10.0), (2.0, 20.0), (3.0, 30.0)], 0.5, 12.0, None),
            stages: None,
            motion: None,
        };
        let r = Report {
            choice: Choice::Mine,
            device: fake_device(),
            map: "demo".into(),
            cells: vec![cell("mine.near", "Мои настройки, ближе"), cell("mine.far", "Мои настройки, дальше")],
            file: None,
            copied: false,
            error: None,
            setup: vec!["Vsync: device".into(), "Экран: 1920x1080".into()],
        };
        let body = cloud_report(&r, 1_700_000_000);
        assert!(body.validate().is_ok(), "the game's own report passes the server's checks");
        assert_eq!((body.razdor.as_str(), body.at), (r.device.version.as_str(), 1_700_000_000));
        assert_eq!(body.settings.get("setup1").map(String::as_str), Some("Экран: 1920x1080"));
        assert_eq!(body.scenes.len(), 2);
        assert_eq!((body.scenes[0].name.as_str(), body.scenes[0].median_ms, body.scenes[0].p99_ms), ("Мои настройки, ближе", 20.0, 30.0));
        assert_eq!(body.scenes[0].frames, 3);
        let mut broken = body.clone();
        broken.scenes[0].median_ms = f32::NAN;
        assert!(broken.validate().is_err(), "a NaN frame time is refused before it is sent");
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        let v: Vec<f32> = (1..=100).map(|n| n as f32).collect();
        assert_eq!(percentile(&v, 0.5), 50.0);
        assert_eq!(percentile(&v, 0.9), 90.0);
        assert_eq!(percentile(&v, 0.99), 99.0);
        assert_eq!(percentile(&v, 1.0), 100.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
        assert_eq!(percentile(&[7.0], 0.99), 7.0);
    }

    #[test]
    fn stats_count_fps_and_the_slow_frames() {
        // 120 frames of 16 ms and 6 of 40 ms over 12 s.
        let mut frames: Vec<(f32, f32)> = vec![(5.0, 16.0); 120];
        frames.extend(vec![(9.0, 40.0); 6]);
        let s = stats(&frames, 12.0, 800.0, Some(410.0));
        assert_eq!(s.frames, 126);
        assert!((s.fps - 10.5).abs() < 1e-9);
        assert_eq!(s.frame_med, 16.0);
        assert_eq!(s.frame_p99, 40.0);
        assert_eq!(s.cpu_med, 5.0);
        assert_eq!(s.slow, 6, "only frames over 33 ms are slow");
        assert_eq!((s.load_ms, s.rss_mb), (800.0, Some(410.0)));
    }

    #[test]
    fn the_result_line_is_one_key_value_line_with_the_device() {
        let dev = Device { version: "1.5.0".into(), screen: (1920, 1080), gpu: "NVIDIA \"X\"".into(), os: "Fedora".into(), vsync: "device".into() };
        let s = stats(&[(4.0, 16.0), (4.0, 17.0)], 12.0, 900.0, None);
        let line = result_line(&CellResult { id: "mine.far".into(), name: "Мои настройки · вся карта".into(), view: "25d", stats: s, stages: None, motion: None }, &dev);
        assert!(line.starts_with("RazdorBench cell=mine.far name=\"Мои настройки · вся карта\" view=25d pos=far "), "{line}");
        assert!(line.contains(" rss_mb=na "));
        assert!(line.contains(" gpu=\"NVIDIA 'X'\" "), "quotes become apostrophes: {line}");
        assert!(line.ends_with(" version=1.5.0"));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn the_file_name_is_the_utc_time() {
        assert_eq!(file_name(0), "razdor-bench-1970-01-01-000000.txt");
        // 1_000_000_000 s is 2001-09-09 01:46:40 UTC.
        assert_eq!(file_name(1_000_000_000), "razdor-bench-2001-09-09-014640.txt");
    }

    #[test]
    fn the_report_text_ends_with_the_done_line_and_has_every_cell() {
        let dev = Device { version: "1.5.0".into(), screen: (1, 2), gpu: "g".into(), os: "o".into(), vsync: "device".into() };
        let r = Report {
            choice: Choice::Mine,
            device: dev,
            map: "Другой берег".into(),
            cells: vec![CellResult { id: "mine.near".into(), name: "n".into(), view: "flat", stats: stats(&[(1.0, 16.0)], 1.0, 1.0, None), stages: None, motion: None }],
            file: None,
            copied: false,
            error: None,
            setup: Vec::new(),
        };
        let text = report_text(&r);
        assert!(text.contains("\nRazdorBench cell=mine.near "));
        assert!(text.ends_with("RazdorBench DONE\n"));
    }

    #[test]
    fn a_profiled_cell_has_its_stage_line_after_its_cell_line_and_others_have_none() {
        let dev = Device { version: "1.5.0".into(), screen: (1, 2), gpu: "g".into(), os: "o".into(), vsync: "device".into() };
        let cell = |id: &str, stages: Option<&str>| CellResult {
            id: id.into(),
            name: "n".into(),
            view: "flat",
            stats: stats(&[(1.0, 16.0)], 1.0, 1.0, None),
            stages: stages.map(str::to_string),
            motion: None,
        };
        assert_eq!(stages_line(&cell("mine.near", None)), None);
        let r = Report {
            choice: Choice::Mine,
            device: dev,
            map: "m".into(),
            cells: vec![cell("mine.near", Some("cell=mine.near frames=1 gpu_frames=0 tick_p50=0.10 tick_p90=0.20")), cell("mine.far", None)],
            file: None,
            copied: false,
            error: None,
            setup: Vec::new(),
        };
        let text = report_text(&r);
        let cell_line = text.find("\nRazdorBench cell=mine.near ").expect("the cell line");
        let stage_line = text.find("\nRazdorBenchStages cell=mine.near frames=1 gpu_frames=0 tick_p50=0.10 tick_p90=0.20\n").expect("the stage line");
        let far = text.find("\nRazdorBench cell=mine.far ").expect("the next cell");
        assert!(cell_line < stage_line && stage_line < far, "the stage line follows its cell's line");
        assert_eq!(text.matches("RazdorBenchStages").count(), 1, "only the profiled cell has one");
        assert!(text.ends_with("RazdorBench DONE\n"));
    }

    #[test]
    fn the_report_header_carries_the_device_display_render_and_settings_lines() {
        let dev = Device { version: "1.5.0".into(), screen: (1, 2), gpu: "g".into(), os: "o".into(), vsync: "device".into() };
        let setup = vec!["Device: os=android model=\"moto g32\"".to_string(), "Settings: quality=none".to_string()];
        let r = Report { choice: Choice::Mine, device: dev, map: "m".into(), cells: Vec::new(), file: None, copied: false, error: None, setup };
        let text = report_text(&r);
        let vsync = text.find("Vertical sync: device\n").expect("the vertical sync line");
        let device = text.find("\nDevice: os=android model=\"moto g32\"\n").expect("the device line in the header");
        let settings = text.find("\nSettings: quality=none\n").expect("the settings line in the header");
        assert!(vsync < device && device < settings, "the setup lines follow the vertical sync line");
        assert!(text.contains("\n\nCell | FPS"), "a blank line still ends the header");
    }

    #[test]
    fn the_pan_cells_are_the_two_originals_after_the_choice() {
        assert_eq!(pan_cells().iter().map(Slot::id).collect::<Vec<_>>(), ["original-25d.pan", "original-flat.pan"]);
        assert_eq!(cells_for(Choice::Mine, false), plan(Choice::Mine), "no pan line: the choice's cells alone");
        let ids: Vec<String> = cells_for(Choice::Mine, true).iter().map(Slot::id).collect();
        assert_eq!(ids, ["mine.near", "mine.far", "original-25d.pan", "original-flat.pan"]);
        assert!(!plan(Choice::All).iter().any(|c| c.pos == Pos::Pan), "the player's menu has no pan cell");
        assert_eq!(Slot { config: ALL[4], pos: Pos::Pan }.name(), format!("{} · {}", tr("Original, flat"), tr("panning")));
    }

    #[test]
    fn the_pan_moves_three_seconds_and_holds_half_a_second() {
        assert_eq!(pan_moved_secs(0.0), 0.0);
        assert!((pan_moved_secs(1.5) - 1.5).abs() < 1e-12, "it moves with the time");
        assert_eq!(pan_moved_secs(3.0), 3.0);
        assert_eq!(pan_moved_secs(3.2), pan_moved_secs(3.4), "the hold stands still");
        assert_eq!(pan_moved_secs(3.0), pan_moved_secs(3.5), "a hold ends where its move ended");
        assert!((pan_moved_secs(4.0) - 3.5).abs() < 1e-12, "the next move goes on from there");
        assert_eq!(pan_moved_secs(7.0), 6.0, "two moves done");
        assert_eq!(pan_moved_secs(10.2), pan_moved_secs(10.4), "the third hold stands still too");
        assert_eq!(pan_moved_secs(10.2), 9.0);
    }

    #[test]
    fn the_pan_goes_to_the_end_and_back_at_its_speed() {
        // A range of 10 cells at 4 cells a second: 2.5 s a leg.
        assert_eq!(pan_offset(0.0, 4.0, 10.0), 0.0);
        assert!((pan_offset(1.0, 4.0, 10.0) - 4.0).abs() < 1e-9);
        assert!((pan_offset(2.5, 4.0, 10.0) - 10.0).abs() < 1e-9, "the right end after one leg");
        assert!((pan_offset(3.75, 4.0, 10.0) - 5.0).abs() < 1e-9, "back half way");
        assert!(pan_offset(5.0, 4.0, 10.0).abs() < 1e-9, "back at the left end");
        assert_eq!(pan_offset(9.0, 4.0, 0.0), 0.0, "no range, no motion");
        for i in 0..1000 {
            let v = pan_offset(f64::from(i) * 0.037, 4.0, 10.0);
            assert!((0.0..=10.0).contains(&v), "{v}");
        }
    }

    #[test]
    fn a_pan_cell_line_has_its_motion_counts_and_the_others_do_not() {
        let dev = Device { version: "1.5.1".into(), screen: (1, 2), gpu: "g".into(), os: "o".into(), vsync: "device".into() };
        let s = stats(&[(4.0, 16.0)], 12.0, 900.0, None);
        let near = result_line(&CellResult { id: "mine.near".into(), name: "n".into(), view: "flat", stats: s.clone(), stages: None, motion: None }, &dev);
        assert!(!near.contains("fills="), "{near}");
        let pan = CellResult {
            id: "original-flat.pan".into(),
            name: "n".into(),
            view: "flat",
            stats: s,
            stages: None,
            motion: Some(Motion { fills: 2, scrolls: 5, moving: 40, cam_moves: 40 }),
        };
        let line = result_line(&pan, &dev);
        assert!(line.contains(" pos=pan ") && line.contains(" rss_mb=na fills=2 scrolls=5 moving=40 cam_moves=40 vsync="), "{line}");
    }
}
