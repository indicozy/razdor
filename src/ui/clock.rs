//! The interface's clock. Live, it is macroquad's (`get_time`, `get_frame_time`). In a snapshot
//! (`RAZDOR_SNAPSHOT` set) it is a fixed one: it starts at 0 and advances 1/60 s per frame
//! (`tick`, from the main loop), so the cursor's blink, the sways, the fades and every timed
//! window are the same on every run. The UI regression check (tools/ui-regress) relies on that.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

/// The frames drawn so far, in a snapshot.
static FRAMES: AtomicU64 = AtomicU64::new(0);

/// The snapshot's fixed frame step, in seconds (one frame at 60 Hz). A trailer shot sets its own
/// rate (`set_shot`, from its frames per second).
const FRAME_STEP: f64 = 1.0 / 60.0;

/// The frame step in use, as the bits of an `f64` (a shot's `fps` changes it before the run).
static STEP: AtomicU64 = AtomicU64::new(FRAME_STEP.to_bits());
/// Whether a trailer shot runs: its clock is the frame count too, whatever the environment.
static SHOT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether this run is a snapshot (decided once: the variable does not change), or a trailer shot.
fn fixed() -> bool {
    static FIXED: OnceLock<bool> = OnceLock::new();
    SHOT.load(Ordering::Relaxed) || *FIXED.get_or_init(|| std::env::var_os("RAZDOR_SNAPSHOT").is_some())
}

/// The step of one frame in seconds.
fn step() -> f64 {
    f64::from_bits(STEP.load(Ordering::Relaxed))
}

/// A trailer shot runs on this clock: one frame is `1 / fps` seconds, from frame 0.
pub fn set_shot(fps: u32) {
    STEP.store((1.0 / fps as f64).to_bits(), Ordering::Relaxed);
    FRAMES.store(0, Ordering::Relaxed);
    SHOT.store(true, Ordering::Relaxed);
}

/// The frames drawn so far (the snapshot clock's count).
pub fn frame_index() -> u64 {
    FRAMES.load(Ordering::Relaxed)
}

/// Whether this run is a snapshot: its clocks and dates are fixed.
pub fn is_snapshot() -> bool {
    fixed()
}

/// The snapshot clock after `frames` frames.
fn after(frames: u64) -> f64 {
    frames as f64 * step()
}

/// Seconds since the start of the run (macroquad's `get_time`).
pub fn now() -> f64 {
    if fixed() {
        after(FRAMES.load(Ordering::Relaxed))
    } else {
        macroquad::time::get_time()
    }
}

/// The length of the last frame in seconds (macroquad's `get_frame_time`).
pub fn frame_dt() -> f32 {
    if fixed() {
        step() as f32
    } else {
        macroquad::time::get_frame_time()
    }
}

/// Advances the snapshot clock by one frame. The main loop calls it once per frame; live runs
/// ignore it.
pub fn tick() {
    if fixed() {
        FRAMES.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snapshot_clock_runs_at_sixty_frames_a_second() {
        assert_eq!(after(0), 0.0);
        assert!((after(60) - 1.0).abs() < 1e-12);
        assert!((after(90) - 1.5).abs() < 1e-12);
    }
}
