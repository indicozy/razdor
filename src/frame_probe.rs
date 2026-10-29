//! The frame probe: where a frame's time goes, summarised as percentiles, for the 8.3 ms
//! (120 Hz phone) target. Dev-only: on when [`crate::diag::profiling`] is, that is
//! `RAZDOR_PROFILE` set or the app folder's `profile` file present (Android has no env vars,
//! so there the file turns it on).
//!
//! Each frame of the main loop ends with [`end_frame`], which stores a [`Sample`]: the frame's
//! [`Kind`] (set by the map, see [`set_kind`]), its work (the game's frame, `main.rs`), its
//! whole time (vsync included), the CPU time of each [`Stage`] ([`time`]) and the texture
//! rebuilds ([`tex_rebuilt`]). Every 5 s it logs one summary line per kind that had frames
//! ([`summary_lines`]); [`flush`] logs the pending ones at exit.
//!
//! The stage times are CPU time spent building the frame. Macroquad batches draws and issues
//! the GL calls in `next_frame`, so GL submission and the wait for vsync (`eglSwapBuffers`)
//! land in "present" (total minus work). GPU time itself is not visible here.

use crate::local;
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// The parts of a frame that are timed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    /// The game's tick (`Game::tick_shown`): the hero's walk, the armies, the AI.
    Tick,
    /// The terrain layer of the map (inside [`Stage::World`]).
    Terrain,
    /// The whole world map: terrain, objects and sprites (`draw_world`).
    World,
    /// The fog layer over the map.
    Fog,
    /// The minimap window.
    Minimap,
}

impl Stage {
    pub const COUNT: usize = 5;
}

/// What the frame was: the map with the hero walking, or not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// The map, the hero not walking.
    Idle,
    /// The hero walking, no step this frame.
    Walk,
    /// A frame in which the hero moved to another cell.
    WalkStep,
    /// Any other screen.
    Other,
}

/// One frame's numbers, in milliseconds.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub kind: Kind,
    pub work_ms: f32,
    pub total_ms: f32,
    pub stage_ms: [f32; Stage::COUNT],
    pub rebuilds: u32,
}

struct State {
    /// This frame's time per stage (calls of one stage add up).
    stage: [Duration; Stage::COUNT],
    rebuilds: u32,
    kind: Kind,
    samples: Vec<Sample>,
    /// When the samples since the last summary began.
    since: Option<Instant>,
}

impl State {
    const fn new() -> State {
        State { stage: [Duration::ZERO; Stage::COUNT], rebuilds: 0, kind: Kind::Other, samples: Vec::new(), since: None }
    }
}

local! {
    static STATE: RefCell<State> = const { RefCell::new(State::new()) };
}

/// The time between two summaries.
const PERIOD: Duration = Duration::from_secs(5);

/// The probe is on (the same switch as the profiling log, [`crate::diag::profiling`]).
pub fn on() -> bool {
    crate::diag::profiling()
}

/// Runs `f`; with the probe on, adds its time to this frame's slot for `stage`.
pub fn time<T>(stage: Stage, f: impl FnOnce() -> T) -> T {
    if !on() {
        return f();
    }
    let started = Instant::now();
    let r = f();
    let spent = started.elapsed();
    STATE.with_borrow_mut(|s| s.stage[stage as usize] += spent);
    r
}

/// A texture was rebuilt this frame (the minimap's and the fog's caches).
pub fn tex_rebuilt() {
    if on() {
        STATE.with_borrow_mut(|s| s.rebuilds += 1);
    }
}

/// Sets this frame's kind; the default each frame is [`Kind::Other`].
pub fn set_kind(kind: Kind) {
    if on() {
        STATE.with_borrow_mut(|s| s.kind = kind);
    }
}

/// Stores this frame's sample and resets the per-frame slots and the kind. Once 5 s have
/// passed since the last summary, logs the summary and clears the samples.
pub fn end_frame(work: Duration, total: Duration) {
    if !on() {
        return;
    }
    let now = Instant::now();
    let due = STATE.with_borrow_mut(|s| {
        let since = *s.since.get_or_insert(now);
        s.samples.push(Sample {
            kind: s.kind,
            work_ms: ms(work),
            total_ms: ms(total),
            stage_ms: s.stage.map(ms),
            rebuilds: s.rebuilds,
        });
        s.stage = [Duration::ZERO; Stage::COUNT];
        s.rebuilds = 0;
        s.kind = Kind::Other;
        let elapsed = now.duration_since(since);
        if elapsed < PERIOD {
            return None;
        }
        s.since = Some(now);
        Some((elapsed.as_secs_f32(), std::mem::take(&mut s.samples)))
    });
    if let Some((secs, samples)) = due {
        log_lines(secs, &samples);
    }
}

/// Logs the pending samples' summary now (at exit). Does nothing when there are none.
pub fn flush() {
    if !on() {
        return;
    }
    let (secs, samples) = STATE.with_borrow_mut(|s| {
        let secs = s.since.map_or(0.0, |t| t.elapsed().as_secs_f32());
        s.since = None;
        (secs, std::mem::take(&mut s.samples))
    });
    log_lines(secs, &samples);
}

fn log_lines(secs: f32, samples: &[Sample]) {
    for line in summary_lines(secs, samples) {
        crate::diag::log(&line);
    }
}

fn ms(d: Duration) -> f32 {
    d.as_secs_f32() * 1000.0
}

/// The summary of `samples`, taken over `secs` seconds: one line per kind that has frames, in
/// the order walk-step, walk, idle, other. Empty when there are no frames.
pub fn summary_lines(secs: f32, samples: &[Sample]) -> Vec<String> {
    [Kind::WalkStep, Kind::Walk, Kind::Idle, Kind::Other]
        .into_iter()
        .filter_map(|kind| {
            let of: Vec<&Sample> = samples.iter().filter(|s| s.kind == kind).collect();
            (!of.is_empty()).then(|| kind_line(secs, kind, &of))
        })
        .collect()
}

fn kind_line(secs: f32, kind: Kind, of: &[&Sample]) -> String {
    let name = match kind {
        Kind::WalkStep => "walk-step",
        Kind::Walk => "walk",
        Kind::Idle => "idle",
        Kind::Other => "other",
    };
    let rebuilds: u32 = of.iter().map(|s| s.rebuilds).sum();
    let stage = |idx: usize| dist(of.iter().map(|s| s.stage_ms[idx])).narrow();
    format!(
        "frame probe {secs:.1} s {name} n={} | total {} | work {} | present {} | tick {} | terrain {} | objects {} | fog {} | minimap {} | rest {} | tex rebuilds {rebuilds}",
        of.len(),
        dist(of.iter().map(|s| s.total_ms)).wide(),
        dist(of.iter().map(|s| s.work_ms)).wide(),
        dist(of.iter().map(|s| present_ms(s))).wide(),
        stage(Stage::Tick as usize),
        stage(Stage::Terrain as usize),
        dist(of.iter().map(|s| objects_ms(s))).narrow(),
        stage(Stage::Fog as usize),
        stage(Stage::Minimap as usize),
        dist(of.iter().map(|s| rest_ms(s))).narrow(),
    )
}

/// Time outside the game's work: the vsync wait and the GL submission, `total - work`.
fn present_ms(s: &Sample) -> f32 {
    s.total_ms - s.work_ms
}

/// The map's objects (sprites, markers): the world minus its terrain.
fn objects_ms(s: &Sample) -> f32 {
    (s.stage_ms[Stage::World as usize] - s.stage_ms[Stage::Terrain as usize]).max(0.0)
}

/// What the work spends outside the timed stages, clamped at zero.
fn rest_ms(s: &Sample) -> f32 {
    let timed = s.stage_ms[Stage::Tick as usize]
        + s.stage_ms[Stage::World as usize]
        + s.stage_ms[Stage::Fog as usize]
        + s.stage_ms[Stage::Minimap as usize];
    (s.work_ms - timed).max(0.0)
}

/// Percentiles and the maximum of one column of values.
struct Dist {
    p50: f32,
    p90: f32,
    p99: f32,
    max: f32,
}

fn dist(values: impl Iterator<Item = f32>) -> Dist {
    let mut v: Vec<f32> = values.collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Dist {
        p50: percentile(&v, 50.0),
        p90: percentile(&v, 90.0),
        p99: percentile(&v, 99.0),
        max: v.last().copied().unwrap_or(0.0),
    }
}

impl Dist {
    /// "p50 a p90 b p99 c max d".
    fn wide(&self) -> String {
        format!("p50 {:.1} p90 {:.1} p99 {:.1} max {:.1}", self.p50, self.p90, self.p99, self.max)
    }

    /// "p50 a p99 b max c".
    fn narrow(&self) -> String {
        format!("p50 {:.1} p99 {:.1} max {:.1}", self.p50, self.p99, self.max)
    }
}

/// The nearest-rank percentile `p` (0 to 100) of `sorted` (ascending); 0 when it is empty.
pub fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p * sorted.len() as f32 / 100.0).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(kind: Kind, work_ms: f32, total_ms: f32, stage_ms: [f32; Stage::COUNT]) -> Sample {
        Sample { kind, work_ms, total_ms, stage_ms, rebuilds: 0 }
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        let data: Vec<f32> = (1..=10).map(|n| n as f32).collect();
        assert_eq!(percentile(&data, 50.0), 5.0);
        assert_eq!(percentile(&data, 90.0), 9.0);
        assert_eq!(percentile(&data, 99.0), 10.0);
        assert_eq!(percentile(&data, 100.0), 10.0);
        assert_eq!(percentile(&data, 0.0), 1.0);
        assert_eq!(percentile(&[7.5], 50.0), 7.5);
        assert_eq!(percentile(&[], 50.0), 0.0);
    }

    #[test]
    fn objects_and_rest_are_worked_out_per_frame() {
        // Stages: tick 1, terrain 2, world 5 (terrain inside it), fog 1, minimap 0.5.
        let s = sample(Kind::Walk, 10.0, 12.0, [1.0, 2.0, 5.0, 1.0, 0.5]);
        assert_eq!(objects_ms(&s), 3.0);
        assert_eq!(rest_ms(&s), 2.5);
        assert_eq!(present_ms(&s), 2.0);
        // Stages that add up to more than the work: the rest is clamped at zero.
        let over = sample(Kind::Walk, 5.0, 6.0, [2.0, 0.0, 4.0, 2.0, 0.0]);
        assert_eq!(rest_ms(&over), 0.0);
    }

    #[test]
    fn a_kind_without_frames_has_no_line() {
        let samples = [sample(Kind::Walk, 4.0, 5.0, [0.0; Stage::COUNT]), sample(Kind::Walk, 6.0, 7.0, [0.0; Stage::COUNT])];
        let lines = summary_lines(5.0, &samples);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("frame probe 5.0 s walk n=2 "), "{}", lines[0]);
        assert!(summary_lines(5.0, &[]).is_empty());
    }

    #[test]
    fn the_lines_follow_the_order_and_carry_the_rebuilds() {
        let mut step = sample(Kind::WalkStep, 3.0, 4.0, [1.0, 1.0, 2.0, 0.0, 0.0]);
        step.rebuilds = 2;
        let idle = sample(Kind::Idle, 2.0, 3.0, [0.0; Stage::COUNT]);
        let lines = summary_lines(5.0, &[idle, step]);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(" walk-step n=1 "), "{}", lines[0]);
        assert!(lines[0].ends_with("tex rebuilds 2"), "{}", lines[0]);
        assert!(lines[1].contains(" idle n=1 "), "{}", lines[1]);
    }
}
