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

#[cfg(not(target_os = "android"))]
pub fn gpu_wait(_stage: Stage) {}

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
