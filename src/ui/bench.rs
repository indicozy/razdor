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
use std::time::{Duration, Instant};

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
        };
        format!("{}.{pos}", self.config.key)
    }

    /// The cell's name for the window and the log (translated).
    pub fn name(&self) -> String {
        let pos = match self.pos {
            Pos::Near => tr("near"),
            Pos::Far => tr("whole map"),
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

/// One finished cell.
#[derive(Clone, Debug, PartialEq)]
pub struct CellResult {
    pub id: String,
    pub name: String,
    /// The view the cell ran in (`25d` or `flat`).
    pub view: &'static str,
    pub stats: Stats,
}

/// A quoted text value of a log line (its quotes become apostrophes).
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "'"))
}

/// The log line of one finished cell: `RazdorBench key=value…`, the values with spaces quoted.
pub fn result_line(r: &CellResult, dev: &Device) -> String {
    let s = &r.stats;
    let rss = s.rss_mb.map_or("na".to_string(), |v| format!("{v:.0}"));
    format!(
        "RazdorBench cell={} name={} view={} pos={} fps={:.1} frames={} secs={:.1} frame_ms_med={:.2} frame_ms_p90={:.2} frame_ms_p99={:.2} cpu_ms_med={:.2} slow={} load_ms={:.0} rss_mb={} vsync={} screen={}x{} gpu={} os={} version={}",
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
    out.push_str(&format!("Vertical sync: {}\n\n", r.device.vsync));
    out.push_str("Cell | FPS | frame ms median | frame ms p99 | slow frames\n");
    for c in &r.cells {
        out.push_str(&format!("{} | {:.1} | {:.1} | {:.1} | {}\n", c.name, c.stats.fps, c.stats.frame_med, c.stats.frame_p99, c.stats.slow));
    }
    out.push('\n');
    for c in &r.cells {
        out.push_str(&result_line(c, &r.device));
        out.push('\n');
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
    // SAFETY: called on the main thread once the context exists (the title screen has drawn).
    let p = unsafe { macroquad::miniquad::gl::glGetString(0x1F01) };
    if p.is_null() {
        return "unknown".into();
    }
    // SAFETY: GL returns a NUL-terminated string that lives as long as the context.
    unsafe { std::ffi::CStr::from_ptr(p as *const std::ffi::c_char) }.to_string_lossy().into_owned()
}

/// The operating system's name: the release's pretty name on Linux, else the target's.
fn os_name() -> String {
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
    let Some(_) = app.dt_content.clone() else {
        return Err(tr("The test needs the game: the Discord Times folder.").to_string());
    };
    let Some(map_index) = map_index(app) else {
        return Err(tr("No map of the game was found.").to_string());
    };
    let cells = plan(choice);
    let map = app.scenarios[map_index].file.clone();
    let saved = Saved {
        settings: app.audio.settings.clone(),
        game: app.game.take(),
        dialogs: std::mem::take(&mut app.dialogs),
        message: app.message.take(),
    };
    keep_awake(true);
    log_line(&format!("RazdorBench START choice={} cells={} map={}", if choice == Choice::Mine { "mine" } else { "all" }, cells.len(), quoted(&map)));
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
    }
    put_run(run);
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
        run.stage = stage;
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
    let result = CellResult { id: cell.id(), name: cell.name(), view: view_name(&app.audio.settings), stats: st };
    let dev = run.device.get_or_insert_with(device);
    log_line(&result_line(&result, dev));
    run.results.push(result);
    run.index += 1;
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
    };
    // `RAZDOR_BENCH_SAVE` (a lab's or a test's run, with no one to press Save): the file is saved at once.
    if std::env::var_os("RAZDOR_BENCH_SAVE").is_some() {
        let file = save(&report);
        report.copied = false;
        match file {
            Ok(path) => report.file = Some(path),
            Err(e) => report.error = Some(e),
        }
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
    let epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(file_name(epoch));
    std::fs::write(&path, report_text(report)).map_err(|e| e.to_string())?;
    Ok(path)
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
                    CellResult { id: c.id(), name: c.name(), view: if c.config.tilt == Some(false) { "flat" } else { "25d" }, stats: stats(&frames, MEASURE_SECS, 1200.0 + i as f32 * 90.0, Some(540.0)) }
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
        let mut s = Settings::default();
        s.map_first_person = true;
        s.frame_cap = 60;
        s.battery_saver = true;
        s.record_game = true;
        configure(&mut s, &Slot { config: ALL[4], pos: Pos::Near });
        assert!(!s.map_tilt, "the flat view");
        assert_eq!((s.frame_cap, s.battery_saver, s.record_game), (0, false, false));
        configure(&mut s, &Slot { config: ALL[3], pos: Pos::Far });
        assert!(s.map_tilt && !s.map_first_person, "2.5D is the third-person view");
        assert_eq!(quality_preset::current(&s), Some(Preset::High));
    }

    #[test]
    fn my_settings_keep_the_players_look() {
        let mut s = Settings::default();
        s.map_tilt = true;
        s.map_first_person = true;
        s.frame_cap = 30;
        configure(&mut s, &Slot { config: MINE, pos: Pos::Far });
        assert!(s.map_tilt && s.map_first_person, "the player's own view");
        assert_eq!(s.frame_cap, 0);
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
        let line = result_line(&CellResult { id: "mine.far".into(), name: "Мои настройки · вся карта".into(), view: "25d", stats: s }, &dev);
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
            cells: vec![CellResult { id: "mine.near".into(), name: "n".into(), view: "flat", stats: stats(&[(1.0, 16.0)], 1.0, 1.0, None) }],
            file: None,
            copied: false,
            error: None,
        };
        let text = report_text(&r);
        assert!(text.contains("\nRazdorBench cell=mine.near "));
        assert!(text.ends_with("RazdorBench DONE\n"));
    }
}
