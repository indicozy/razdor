//! The performance lab's switches (dev-only, all off by default: no look change for players).
//!
//! Each switch is on for a run when its environment variable says so on desktop (`RAZDOR_REPLAY_FLOOR=1`,
//! `RAZDOR_GPU_FINISH=1`, `RAZDOR_SWAP_INTERVAL=0|1`, `RAZDOR_CLEAR_TARGETS=1`, `RAZDOR_STATIC_DRAW=1`), and when the
//! performance test's trigger file asks for it (`ui/bench_autorun.rs`: the lines `replay-floor`, `gpu-finish`,
//! `swap-interval 0`, `clear-targets`, `static-draw`; Android has no env vars, so its runs use the file). A trigger run sets
//! all of them ([`set`]), and the run's end sets the environment's back ([`set`] with [`Flags::from_env`]).
//!
//! What each one does, and how to read it: `docs/flows/testing-and-tools.md` (the lab's switches).

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// The switches of a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// On a still frame the world's sprite pass is not run: the batch of the last still frame is issued again.
    pub replay_floor: bool,
    /// Before the swap the frame's draws are submitted and the GL queue waited for, timed as the `finish` stage.
    pub gpu_finish: bool,
    /// Each render target's fill starts from a cleared target (the whole-target fills only, see `render_cache`).
    pub clear_targets: bool,
    /// The swap interval the run asks for (0 or 1); None for the driver's default (1 on EGL).
    pub swap_interval: Option<i32>,
    /// The flat view's static decorations are drawn with retained draw calls (`static_draw`, A′).
    pub static_draw: bool,
}

/// The swap interval's value a flag string gives (`0` or `1`); None for anything else.
fn swap_from(value: Option<&str>) -> Option<i32> {
    match value.map(str::trim) {
        Some("0") => Some(0),
        Some("1") => Some(1),
        _ => None,
    }
}

/// A switch's environment value is on: any value but empty, `0`, `off` or `false`.
fn on_from(value: Option<&str>) -> bool {
    matches!(value.map(|v| v.trim().to_ascii_lowercase()), Some(v) if !v.is_empty() && !matches!(v.as_str(), "0" | "off" | "false"))
}

impl Flags {
    /// The switches the environment asks for (`RAZDOR_REPLAY_FLOOR`, `RAZDOR_GPU_FINISH`, `RAZDOR_SWAP_INTERVAL`,
    /// `RAZDOR_CLEAR_TARGETS`).
    pub fn from_env() -> Flags {
        Flags {
            replay_floor: on_from(std::env::var("RAZDOR_REPLAY_FLOOR").ok().as_deref()),
            gpu_finish: on_from(std::env::var("RAZDOR_GPU_FINISH").ok().as_deref()),
            clear_targets: on_from(std::env::var("RAZDOR_CLEAR_TARGETS").ok().as_deref()),
            swap_interval: swap_from(std::env::var("RAZDOR_SWAP_INTERVAL").ok().as_deref()),
            static_draw: on_from(std::env::var("RAZDOR_STATIC_DRAW").ok().as_deref()),
        }
    }

    /// The names of the switches that are on, as the trigger file's lines name them, comma-separated (empty for none).
    pub fn describe(&self) -> String {
        let mut names = Vec::new();
        if self.replay_floor {
            names.push("replay-floor".to_string());
        }
        if self.gpu_finish {
            names.push("gpu-finish".to_string());
        }
        if let Some(i) = self.swap_interval {
            names.push(format!("swap-interval{i}"));
        }
        if self.clear_targets {
            names.push("clear-targets".to_string());
        }
        if self.static_draw {
            names.push("static-draw".to_string());
        }
        names.join(",")
    }
}

/// The swap interval when nothing asks for another (EGL's default).
const DEFAULT_SWAP: i32 = 1;

static REPLAY_FLOOR: AtomicBool = AtomicBool::new(false);
static GPU_FINISH: AtomicBool = AtomicBool::new(false);
static CLEAR_TARGETS: AtomicBool = AtomicBool::new(false);
static STATIC_DRAW: AtomicBool = AtomicBool::new(false);
/// The swap interval the switches ask for, and whether it changed since the window last took it ([`take_swap`]).
static SWAP: AtomicI32 = AtomicI32::new(DEFAULT_SWAP);
static SWAP_CHANGED: AtomicBool = AtomicBool::new(false);

/// Sets the switches (a run's start, or the environment's again at its end). A swap interval that changed is
/// reported by [`take_swap`]; the default (no switch) is the interval 1.
pub fn set(flags: Flags) {
    REPLAY_FLOOR.store(flags.replay_floor, Ordering::Relaxed);
    GPU_FINISH.store(flags.gpu_finish, Ordering::Relaxed);
    CLEAR_TARGETS.store(flags.clear_targets, Ordering::Relaxed);
    STATIC_DRAW.store(flags.static_draw, Ordering::Relaxed);
    let want = flags.swap_interval.unwrap_or(DEFAULT_SWAP);
    if SWAP.swap(want, Ordering::Relaxed) != want {
        SWAP_CHANGED.store(true, Ordering::Relaxed);
    }
}

/// The switches as they are now.
pub fn current() -> Flags {
    Flags {
        replay_floor: replay_floor(),
        gpu_finish: gpu_finish(),
        clear_targets: clear_targets(),
        swap_interval: Some(SWAP.load(Ordering::Relaxed)).filter(|&i| i != DEFAULT_SWAP),
        static_draw: static_draw(),
    }
}

/// The replay floor is on (see [`Flags::replay_floor`]).
pub fn replay_floor() -> bool {
    REPLAY_FLOOR.load(Ordering::Relaxed)
}

/// The GPU finish is on (see [`Flags::gpu_finish`]).
pub fn gpu_finish() -> bool {
    GPU_FINISH.load(Ordering::Relaxed)
}

/// The render targets are cleared before their fills (see [`Flags::clear_targets`]).
pub fn clear_targets() -> bool {
    CLEAR_TARGETS.load(Ordering::Relaxed)
}

/// The flat view's static decorations are drawn with retained draw calls (see [`Flags::static_draw`]).
pub fn static_draw() -> bool {
    STATIC_DRAW.load(Ordering::Relaxed)
}

/// The swap interval changed since the last call: the new value, once (the window applies it).
pub fn take_swap() -> Option<i32> {
    SWAP_CHANGED.swap(false, Ordering::Relaxed).then(|| SWAP.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_values_are_read_as_on_or_off() {
        assert!(on_from(Some("1")) && on_from(Some(" yes ")) && on_from(Some("on")));
        assert!(!on_from(None) && !on_from(Some("")) && !on_from(Some("0")) && !on_from(Some("off")) && !on_from(Some("FALSE")));
        assert_eq!(swap_from(Some("0")), Some(0));
        assert_eq!(swap_from(Some(" 1 ")), Some(1));
        assert_eq!(swap_from(Some("2")), None, "only the two intervals the lab compares");
        assert_eq!(swap_from(None), None);
    }

    #[test]
    fn the_names_of_the_switches_are_the_trigger_lines() {
        let all = Flags { replay_floor: true, gpu_finish: true, clear_targets: true, swap_interval: Some(0), static_draw: true };
        assert_eq!(all.describe(), "replay-floor,gpu-finish,swap-interval0,clear-targets,static-draw");
        assert_eq!(Flags { static_draw: true, ..Flags::default() }.describe(), "static-draw");
        assert_eq!(Flags::default().describe(), "");
        assert_eq!(Flags { swap_interval: Some(1), ..Flags::default() }.describe(), "swap-interval1");
    }

    #[test]
    fn a_swap_change_is_taken_once_and_the_default_restores_it() {
        set(Flags { swap_interval: Some(0), ..Flags::default() });
        assert!(!replay_floor() && !gpu_finish() && !clear_targets());
        assert_eq!(take_swap(), Some(0));
        assert_eq!(take_swap(), None, "taken once");
        set(Flags::default());
        assert_eq!(take_swap(), Some(1), "the run's end gives the default back");
        assert_eq!(current(), Flags::default(), "the default is no switch");
    }
}
