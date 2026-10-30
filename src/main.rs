// Release builds for Windows open no console window next to the game's.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ui;

use std::sync::Arc;

use macroquad::prelude::*;
use razdor::rules::content::Content;

use ui::assets::Assets;
use ui::App;

/// Only one Razdor at a time: an exclusive lock on `razdor.lock` in the runtime folder, held
/// until the process ends (the OS drops it on exit or crash). A second copy says so and quits
/// before opening a window. `conf` runs before the window exists, so the check lives there.
#[cfg(unix)]
static LOCK: std::sync::OnceLock<std::fs::File> = std::sync::OnceLock::new();

#[cfg(unix)]
fn single_instance() {
    use std::os::unix::io::AsRawFd;
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let path = dir.join("razdor.lock");
    let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path) else {
        return; // No lock file possible: do not stand in the way.
    };
    // SAFETY: flock on a descriptor we own; it only takes an advisory lock.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        razdor::diag!("Razdor is already running ({}).", path.display());
        std::process::exit(0);
    }
    let _ = LOCK.set(file);
}

#[cfg(not(unix))]
fn single_instance() {}

/// Lets the next Razdor take the lock (the updated program started again as this one ends).
fn release_instance() {
    #[cfg(unix)]
    if let Some(file) = LOCK.get() {
        use std::os::unix::io::AsRawFd;
        // SAFETY: flock on a descriptor we own; it only drops the advisory lock.
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn conf() -> Conf {
    // `--replay <actions.jsonl>`: the diff test's script mode, played without a window
    // (`razdor::difftest`). `conf` runs before the window opens, so it ends here.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = razdor::difftest::cli(&args) {
        std::process::exit(code);
    }
    // The log of this start (`razdor.log`, see `razdor::diag`), before anything can fail.
    razdor::diag::init();
    // `RAZDOR_DT_DIR` and the other settings may come from a `.env` file.
    razdor::dt::install::load_dotenv();
    single_instance();
    // Updates: the program's file, and what an update left behind.
    razdor::update::init();
    let (window_width, window_height) = ui::snapshot::size().unwrap_or((1280, 800));
    razdor::diag::step(&format!("opening the window ({window_width}x{window_height}, OpenGL)"));
    // On Linux the WM_CLASS is how `ui::display` finds the game's own X11 window.
    #[cfg(target_os = "linux")]
    let platform = macroquad::miniquad::conf::Platform { linux_wm_class: ui::display::WM_CLASS, ..Default::default() };
    #[cfg(not(target_os = "linux"))]
    let platform = macroquad::miniquad::conf::Platform::default();
    Conf {
        window_title: "Razdor".to_owned(),
        window_width,
        window_height,
        high_dpi: true,
        platform,
        icon: Some(window_icon()),
        ..Default::default()
    }
}

/// The window's and the taskbar's icon: `data/icon/window-*.rgba`, made by `scripts/icon.py`.
fn window_icon() -> macroquad::miniquad::conf::Icon {
    macroquad::miniquad::conf::Icon {
        small: *include_bytes!("../data/icon/window-16.rgba"),
        medium: *include_bytes!("../data/icon/window-32.rgba"),
        big: *include_bytes!("../data/icon/window-64.rgba"),
    }
}

/// Debug: `RAZDOR_QUIT_AFTER=<frames>` asks to quit after that many frames, to test the exit
/// path without a window manager.
fn quit_after() -> Option<u64> {
    std::env::var("RAZDOR_QUIT_AFTER").ok()?.trim().parse().ok()
}

/// Makes every pixel of an RGBA buffer opaque. The window shows the frame without its alpha
/// channel; a PNG keeps it, and viewers then blend the transparent pixels with their
/// background, so the snapshot looks washed out.
fn opaque_rgba(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        px[3] = 255;
    }
}

/// Ends the process without running the C `atexit` handlers.
///
/// A normal return from `main` crashed with SIGSEGV every time: `exit` runs the exit handlers
/// of the native libraries (GL / X11 / ALSA under miniquad), and one of them calls into code
/// that is no longer mapped. Everything Razdor writes (saves, `audio.json`) is written and
/// closed synchronously before this, so skipping the handlers loses nothing.
fn exit_now(app: &mut App) -> ! {
    razdor::diag::step("quitting");
    razdor::frame_probe::flush();
    // The noon autosave still being written on its thread is finished first (the file is whole).
    razdor::rules::save::finish_pending_writes();
    app.shutdown();
    // "Restart now" after an update: the new program, once the lock is free.
    release_instance();
    razdor::update::restart();
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: `_exit` only ends the process; no Rust state is used after it.
    #[cfg(unix)]
    unsafe {
        libc::_exit(0)
    }
    #[cfg(not(unix))]
    std::process::exit(0)
}

#[macroquad::main(conf)]
async fn main() {
    // Closing the window sets a flag instead of leaving the loop, so the exit goes through
    // `exit_now`.
    prevent_quit();
    // An import («Обновить игру из архива…») interrupted while it swapped the game's folders puts the
    // old game back, before the install is looked for (`dt::game_import::recover`).
    if let Some(dir) = razdor::platform::unpack_dir() {
        razdor::dt::game_import::recover(&dir);
    }
    // The picture cache's folder on Android (the app's private cache, not the shared storage).
    #[cfg(target_os = "android")]
    razdor::platform::set_private_cache_dir(ui::android::private_cache_dir());
    // The phones' folders are tidied off the main thread (the old picture cache goes, `.nomedia`).
    std::thread::spawn(razdor::platform::tidy_app_dir);
    // The picture cache's folders of other releases, its old flat files and stale temporary files
    // go too (off the main thread; the current folder's entries are never touched).
    std::thread::spawn(razdor::dt::picture_cache::prune);
    {
        let gl = unsafe { get_internal_gl() };
        let info = gl.quad_context.info();
        razdor::diag::step(&format!("window open: {:?}, {}", info.backend, info.gl_version_string));
    }
    // One black frame before the loading: the loading below runs inside a single frame and
    // handles none of the system's messages, so without it the window just shown would wait
    // seconds for its first messages and its first picture (on Windows it may be taken as not
    // responding meanwhile; players' reports of a black window at start).
    clear_background(BLACK);
    next_frame().await;
    razdor::diag::step("first black frame shown");
    ui::widgets::load_font().await;
    razdor::diag::step("fonts loaded");
    ui::language::init();
    let content = Arc::new(Content::builtin());
    razdor::diag::step("loading the art and sounds");
    let mut app = App::new(Assets::load(content.clone()).await, content);
    razdor::diag::step("started");
    // The session slot is written when the game is ended by a signal (the desktop) or when the
    // app goes to the background (iOS; Android's pause is a JNI call, `ui::android`).
    #[cfg(all(unix, not(any(target_os = "android", target_os = "ios"))))]
    ui::session::install_exit_hooks();
    #[cfg(target_os = "ios")]
    ui::ios::observe_app_state();
    // `--editor`: start in the map editor.
    if std::env::args().skip(1).any(|a| a == "--editor") {
        app.open_editor();
    }
    ui::snapshot::stage(&mut app);
    let snapshot = ui::snapshot::target();
    let quiet = ui::snapshot::quiet();
    let quit_after = quit_after();
    let mut frames = 0u64;
    // Profiling (`diag::profiling`): frames over `diag::slow_frame_ms` (100 ms by default) are
    // logged with their screen, and the frame probe takes every frame.
    let profile = razdor::diag::profiling();
    // With RAZDOR_PROFILE, the frames after a window opens are logged whatever they take.
    let mut opened = 0u32;
    loop {
        let started = std::time::Instant::now();
        let before = app.screen_name();
        if ui::snapshot::open_later(&mut app, frames) {
            opened = 6;
        }
        // The text-size probe lists the texts of this frame (`RAZDOR_TEXT_PROBE`, off otherwise).
        ui::text_probe::begin_frame();
        app.frame();
        // The FPS counter, the keyboard strip and the pointer stay where the screen is.
        ui::pan::end_frame();
        app.draw_fps();
        ui::snapshot::draw_overlay();
        // On mobile, the text being typed, over the frame (the keyboard covers its field).
        ui::widgets::draw_keyboard_strip();
        // The original draws its pointer last, over everything (0x474a81).
        ui::cursor::draw();
        if quiet {
            app.dialogs.clear();
        }
        let work = started.elapsed();
        frames += 1;
        // The snapshot clock moves one frame on (live runs: no-op).
        ui::clock::tick();
        if let Some((path, n)) = &snapshot {
            if frames >= *n {
                ui::text_probe::write();
                let mut shot = get_screen_data();
                opaque_rgba(&mut shot.bytes);
                shot.export_png(path);
                exit_now(&mut app);
            }
        }
        if is_quit_requested() || app.quit || quit_after.is_some_and(|n| frames >= n) {
            exit_now(&mut app);
        }
        ui::widgets::sync_keyboard();
        next_frame().await;
        if frames == 1 {
            razdor::diag::step("first frame shown");
        }
        if profile {
            // The whole frame: the game's work, then drawing and the GPU (textures and font
            // atlases go up there), vsync included (~16 ms). Over the threshold is a hitch.
            let total = started.elapsed();
            razdor::frame_probe::end_frame(work, total);
            if total.as_millis() > razdor::diag::slow_frame_ms() || opened > 0 {
                opened = opened.saturating_sub(1);
                let (w, r) = (work.as_secs_f32() * 1000.0, (total - work).as_secs_f32() * 1000.0);
                razdor::diag!("slow frame: {:.0} ms = work {w:.0} + render {r:.0} ({before} -> {})", w + r, app.screen_name());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::opaque_rgba;

    #[test]
    fn opaque_rgba_sets_every_alpha_and_keeps_rgb() {
        let mut px = vec![1, 2, 3, 0, 4, 5, 6, 128, 7, 8, 9, 255];
        opaque_rgba(&mut px);
        assert_eq!(px, vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255]);
    }
}
