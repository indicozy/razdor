//! Profiling only (Android): the GPU waits of the frame probe. A wait flushes macroquad's
//! batched draws and waits for the GL queue to finish, so the GPU's time for the frame so far
//! is timed as a stage. It serialises the GPU, so it runs only with the frame probe on;
//! elsewhere it does nothing (the stage stays zero).

use razdor::frame_probe::Stage;

/// Flushes the batch and waits for the GL queue, timed as `stage`. The flush keeps the camera
/// and the material state, so the draws after it are as before.
#[cfg(target_os = "android")]
pub fn gpu_wait(stage: Stage) {
    if !razdor::frame_probe::on() {
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
