//! Profiling only (Android): the GPU waits of the frame probe. A wait flushes macroquad's
//! batched draws and waits for the GL queue to finish, so the GPU's time for the frame so far
//! is timed as a stage. It serialises the GPU and slows every frame, so it runs only with the
//! frame probe on and the app folder's `profile-gpu` file (`frame_probe::gpu_waits_on`); elsewhere,
//! a plain `profile` included, it does nothing (the stage stays zero).

use razdor::frame_probe::Stage;

/// Flushes the batch and waits for the GL queue, timed as `stage`. The flush keeps the camera
/// and the material state, so the draws after it are as before.
#[cfg(target_os = "android")]
pub fn gpu_wait(stage: Stage) {
    if !razdor::frame_probe::gpu_waits_on() {
        return;
    }
    razdor::frame_probe::time(stage, || unsafe {
        macroquad::window::get_internal_gl().flush();
        macroquad::miniquad::gl::glFinish();
    });
}

/// Desktop (`RAZDOR_GPU_STAGES=1` with the probe): a stage marker. The batch is flushed, so the GPU
/// runs the draws before the marker, and the GPU time since the previous marker is the stage's. With
/// GL timer queries (`GL_TIME_ELAPSED`, desktop GL 3 and up) the time is read back a frame or two later
/// and does not stall the frame; on GL 2 and GLES it is `glFinish`-bracketed CPU time (the GPU's share
/// is what the frame waits for). Either way the result goes to the probe's `gpu_<stage>_ms` column.
#[cfg(not(target_os = "android"))]
pub fn gpu_wait(stage: Stage) {
    if !razdor::frame_probe::gpu_timers_on() {
        return;
    }
    let frame = razdor::frame_probe::current_frame();
    timers::marker(frame, stage);
}

/// The desktop stage timers of [`gpu_wait`].
#[cfg(not(target_os = "android"))]
mod timers {
    use macroquad::miniquad::gl;
    use razdor::frame_probe::{self, Stage};
    use std::time::Instant;

    /// Queries not read back yet are kept this long before they are dropped (a lost result must
    /// not grow the list without end).
    const MAX_PENDING: usize = 256;

    razdor::local! {
        /// The query open since the last marker (its segment ends at the next one).
        static ACTIVE: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
        /// Queries ended, waiting for their results: the frame and the stage of the segment each measured.
        static PENDING: std::cell::RefCell<Vec<(u64, Stage, u32)>> = const { std::cell::RefCell::new(Vec::new()) };
        /// Queries whose results were read, kept for the next segment.
        static FREE: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
        /// The last marker's time (the fallback's CPU segments).
        static LAST: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    }

    /// Whether the GL context has timer queries: desktop GL 3 and up (GL 2 and GLES have none here).
    fn queries_supported() -> bool {
        // SAFETY: called on the main thread while macroquad runs, with the GL context current.
        !unsafe { gl::is_gl2() }
    }

    /// Ends the segment since the last marker as `stage` of `frame` and starts the next one.
    pub fn marker(frame: u64, stage: Stage) {
        // SAFETY: as `queries_supported`; the GL calls take the ids this module made.
        unsafe {
            macroquad::window::get_internal_gl().flush();
            if !queries_supported() {
                gl::glFinish();
                let now = Instant::now();
                if let Some(since) = LAST.with(std::cell::Cell::get) {
                    frame_probe::record_gpu_stage(frame, stage, (now - since).as_secs_f32() * 1000.0);
                }
                LAST.with(|l| l.set(Some(now)));
                return;
            }
            if let Some(q) = ACTIVE.with(std::cell::Cell::take) {
                gl::glEndQuery(gl::GL_TIME_ELAPSED);
                PENDING.with(|p| p.borrow_mut().push((frame, stage, q)));
            }
            read_ready();
            let q = FREE.with(|f| f.borrow_mut().pop()).unwrap_or_else(|| {
                let mut id = 0u32;
                gl::glGenQueries(1, &mut id);
                id
            });
            gl::glBeginQuery(gl::GL_TIME_ELAPSED, q);
            ACTIVE.with(|a| a.set(Some(q)));
        }
    }

    /// Reads the results that are ready (`GL_QUERY_RESULT_AVAILABLE`) and recycles their queries; the
    /// oldest are dropped past [`MAX_PENDING`].
    ///
    /// # Safety
    /// The GL context must be current; the queries are ones this module made.
    unsafe fn read_ready() {
        let mut ready = Vec::new();
        PENDING.with(|p| {
            p.borrow_mut().retain(|&(frame, stage, q)| {
                let mut avail = 0i32;
                gl::glGetQueryObjectiv(q, gl::GL_QUERY_RESULT_AVAILABLE, &mut avail);
                if avail == 0 {
                    return true;
                }
                let mut ns = 0u64;
                gl::glGetQueryObjectui64v(q, gl::GL_QUERY_RESULT, &mut ns);
                ready.push((frame, stage, ns as f32 / 1.0e6, q));
                false
            });
            while p.borrow().len() > MAX_PENDING {
                let (_, _, q) = p.borrow_mut().remove(0);
                FREE.with(|f| f.borrow_mut().push(q));
            }
        });
        for (frame, stage, ms, q) in ready {
            frame_probe::record_gpu_stage(frame, stage, ms);
            FREE.with(|f| f.borrow_mut().push(q));
        }
    }
}

/// The map frame's last wait (`gpu-ui`), at the frame's end before the next frame: only in a
/// map frame (see `frame_probe::map_frame_now`), so the other screens are not serialised.
pub fn gpu_wait_ui() {
    if razdor::frame_probe::map_frame_now() {
        gpu_wait(Stage::GpuUi);
    }
}

/// Runs `f` (the terrain's draws of the frame) timed on the GPU when the probe's GPU option is on
/// (desktop: `RAZDOR_GPU_STAGES=1` with `RAZDOR_PROFILE`): the queue is flushed and finished
/// before and after `f`, so the time between is the GPU's time for `f`'s draws (the batch is
/// flushed inside). It serialises the GPU, so it is for A/B runs only. Elsewhere `f` runs as it is.
/// (GL timer queries read back zero on this driver's path, so they are not used.)
#[cfg(not(target_os = "android"))]
pub fn timed<R>(f: impl FnOnce() -> R) -> R {
    if !razdor::frame_probe::gpu_timers_on() {
        return f();
    }
    let kind = razdor::frame_probe::current_kind();
    // SAFETY: called on the main thread while macroquad runs, as every draw; `glFinish` takes no
    // arguments and is loaded by miniquad.
    unsafe {
        macroquad::window::get_internal_gl().flush();
        macroquad::miniquad::gl::glFinish();
        let started = std::time::Instant::now();
        let r = f();
        macroquad::window::get_internal_gl().flush();
        macroquad::miniquad::gl::glFinish();
        razdor::frame_probe::record_gpu(kind, started.elapsed().as_secs_f32() * 1000.0);
        r
    }
}

#[cfg(target_os = "android")]
pub fn timed<R>(f: impl FnOnce() -> R) -> R {
    f()
}
