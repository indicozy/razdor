//! The replay floor (`RAZDOR_REPLAY_FLOOR=1`, the performance test's `replay-floor` line; `lab`): a lab measure of what
//! macroquad and the driver cost on a still frame, without the sprite pass.
//!
//! On a frame where the hero does not move and the camera is where the last frame's was, the world's standing
//! sprites are not drawn again: the batch that last still frame drew for them is issued again
//! (`QuadGl::batch_replay`, a copy of its draw calls and vertices). The frame then pays macroquad's submission and
//! the driver's, and the sprite pass's own time is gone (the `replay` stage stands in for it). The pictures are the
//! last frame's, stale but the same on a still frame (the sprites' animation does not move); a replayed frame
//! records no picks. Lab only: nothing here runs with the switch off.

use std::cell::RefCell;

use macroquad::window::get_internal_gl;
use macroquad::BatchSave;
use razdor::frame_probe::{self, Stage};
use razdor::lab;
use razdor::local;

local! {
    /// The last frame's sprite batch, and the view key it was drawn for (None before any frame has one).
    static LAST: RefCell<Option<(u64, BatchSave)>> = const { RefCell::new(None) };
}

/// The key of a camera's view (`parts`: its origin, zoom and the windows it is drawn in, as floats; `tilted`: the
/// 2.5D view): a still frame replays only when its key is the last drawn frame's.
pub fn view_key(parts: &[f32], tilted: bool) -> u64 {
    parts.iter().map(|p| p.to_bits()).chain([u32::from(tilted)]).fold(0xcbf2_9ce4_8422_2325_u64, |h, p| (h ^ u64::from(p)).wrapping_mul(0x0100_0000_01b3))
}

/// Runs the world's sprite pass `pass`, or issues the last drawn still frame's batch again instead (see the module).
/// `still` is true when the hero is not moving. With the switch off, `pass` runs as it is.
pub fn sprites(key: u64, still: bool, pass: impl FnOnce()) {
    if !lab::replay_floor() {
        return pass();
    }
    // SAFETY: the frame's draws run on the main thread while macroquad runs (as every draw of the world).
    let replayed = LAST.with_borrow(|last| match last {
        Some((k, save)) if still && *k == key => {
            frame_probe::time(Stage::Replay, || unsafe { get_internal_gl() }.quad_gl.batch_replay(save));
            true
        }
        _ => false,
    });
    if replayed {
        frame_probe::replayed();
        return;
    }
    let mark = unsafe { get_internal_gl() }.quad_gl.batch_mark();
    pass();
    // A pass that submitted the batch on its way (a flush inside it) leaves no batch to keep: the next still frame
    // then runs the pass as well.
    let save = unsafe { get_internal_gl() }.quad_gl.batch_save(mark);
    LAST.with_borrow_mut(|last| *last = save.map(|s| (key, s)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_key_follows_the_camera_and_not_the_frame() {
        let view = [10.0, 20.0, 2.0, 0.0, 0.0, 800.0, 600.0];
        let a = view_key(&view, false);
        assert_eq!(a, view_key(&view, false), "the same view, the same key");
        assert_ne!(a, view_key(&[10.5, 20.0, 2.0, 0.0, 0.0, 800.0, 600.0], false), "a scroll is another view");
        assert_ne!(a, view_key(&[10.0, 20.0, 2.5, 0.0, 0.0, 800.0, 600.0], false), "a zoom is another view");
        assert_ne!(a, view_key(&view, true), "the 2.5D view is another view");
    }
}
