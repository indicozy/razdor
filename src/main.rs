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
    // `--map-picture <map.DTm> <out.png>`: the catalog pack's export of a map's own picture, no window
    // (`razdor::hub::export_cli`).
    if let Some(code) = razdor::hub::export_cli(&args) {
        std::process::exit(code);
    }
    // `--trailer <shot> [--plan | --preview] [--out <mp4>] [--frames <dir>] [--hashes <file>]`: the
    // trailer's shot, rendered frame-exact (dev-only, `ui::trailer`; `docs/trailer-shots.md`).
    if let Some(code) = ui::trailer::configure(&args) {
        std::process::exit(code);
    }
    // Android: the app's own folder, and whether the data has moved to it. Before the log opens: the
    // log's folder is fixed at its first use (`diag::log_path`), and the folders come from this.
    #[cfg(target_os = "android")]
    razdor::platform::set_android_files_dir(ui::android::files_dir());
    // The log of this start (`razdor.log`, see `razdor::diag`), before anything can fail.
    razdor::diag::init();
    // `RAZDOR_DT_DIR` and the other settings may come from a `.env` file.
    razdor::dt::install::load_dotenv();
    single_instance();
    // Updates: the program's file, and what an update left behind.
    razdor::update::init();
    let (window_width, window_height) = ui::trailer::window_size().map(|(w, h)| (w as i32, h as i32)).or_else(ui::snapshot::size).unwrap_or((1280, 800));
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
    // Android: the app leaves the screen first, so the slow writes below happen off the screen
    // (on a phone the window would stay frozen on it until the process is gone). The activity's
    // onPause then flushes the session slot on its UI thread (`ui::android`, `razdorOnPause`),
    // which is harmless: `session::flush_now` writes nothing that is already written.
    #[cfg(target_os = "android")]
    ui::display::hide_for_exit();
    let timed = razdor::diag::profiling();
    let t0 = std::time::Instant::now();
    razdor::frame_probe::flush();
    let t1 = std::time::Instant::now();
    // The noon autosave still being written on its thread is finished first (the file is whole).
    razdor::rules::save::finish_pending_writes();
    let t2 = std::time::Instant::now();
    app.shutdown();
    let t3 = std::time::Instant::now();
    // The saves are written: the window goes now, so the memory the process frees below is
    // freed off the screen (the kernel frees it before the X connection closes, and the window
    // would stay up meanwhile). Android hid the app before the saves (above).
    #[cfg(not(target_os = "android"))]
    ui::display::hide_for_exit();
    let t4 = std::time::Instant::now();
    // "Restart now" after an update: the new program, once the lock is free.
    release_instance();
    razdor::update::restart();
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    if timed {
        let ms = |a: std::time::Instant, b: std::time::Instant| (b - a).as_secs_f64() * 1000.0;
        razdor::diag!(
            "exit timing: frame probe {:.1} ms, autosave join {:.1} ms, app shutdown {:.1} ms, hide {:.1} ms, restart {:.1} ms",
            ms(t0, t1),
            ms(t1, t2),
            ms(t2, t3),
            ms(t3, t4),
            ms(t4, std::time::Instant::now())
        );
    }
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
    // Whether this install is an older one (its private cache holds files already), before the
    // threads below write to the cache: the popup of the saves of an older release depends on it.
    #[cfg(target_os = "android")]
    ui::saves_move::decide_at_start(razdor::platform::cache_dir().as_deref());
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
    let assets = Assets::load(content.clone()).await;
    // With no install, the first screen's art is Razdor's own copy (`ui::menu_art`); an install
    // replaces it when it is loaded (`ui::assets::announce_install`).
    if assets.dt.is_none() {
        if let Some(dir) = ui::menu_art::root_or_extract().await {
            razdor::diag!("menu art: {}", dir.display());
            ui::chrome::set_install(dir);
        }
    }
    let mut app = App::new(assets, content);
    // The hub's catalog, read once for the launch (the promo ad and the game check; `ui::launch`).
    ui::launch::start();
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
    // The frame rate cap (`ui::frame_cap`): the frames start a period apart. Snapshots and the trailer never sleep.
    let mut pacer = ui::frame_cap::Pacer::default();
    // Profiling (`diag::profiling`): frames over `diag::slow_frame_ms` (100 ms by default) are
    // logged with their screen, and the frame probe takes every frame.
    let profile = razdor::diag::profiling();
    // With RAZDOR_PROFILE, the frames after a window opens are logged whatever they take.
    let mut opened = 0u32;
    loop {
        // The window's size and DPI for this frame, read once here (`ui::frame_state`).
        ui::frame_state::refresh();
        let started = std::time::Instant::now();
        let before = app.screen_name();
        if ui::snapshot::open_later(&mut app, frames) {
            opened = 6;
        }
        // The text-size and touch probes list the texts and the touch targets of this frame
        // (`RAZDOR_TEXT_PROBE`, `RAZDOR_TOUCH_PROBE`, off otherwise).
        ui::text_probe::begin_frame();
        ui::touch_probe::begin_frame();
        // A trailer run sets its camera and timed actions for this frame (`ui::trailer`).
        ui::trailer::before_frame(&mut app);
        app.frame();
        // The FPS counter, the keyboard strip and the pointer stay where the screen is.
        ui::pan::end_frame();
        app.draw_fps();
        ui::snapshot::draw_overlay();
        // On mobile, the text being typed, over the frame (the keyboard covers its field).
        ui::widgets::draw_keyboard_strip();
        // The original draws its pointer last, over everything (0x474a81).
        // A trailer run has no pointer in its pictures.
        if !ui::trailer::active() {
            ui::cursor::draw();
        }
        // A performance test's mid-run picture (`RAZDOR_BENCH_MID=<png>`, `RAZDOR_BENCH_MID_CELL=<n>`): one frame of
        // the cell being measured, then the run goes on.
        if ui::bench::take_mid_shot() {
            if let Ok(path) = std::env::var("RAZDOR_BENCH_MID") {
                let mut shot = get_screen_data();
                opaque_rgba(&mut shot.bytes);
                shot.export_png(&path);
            }
        }
        // A performance test's end picture (`RAZDOR_BENCH_END=<png>`, a lab's or a test's run): the results are drawn, then the process ends.
        if ui::bench::take_end_shot() {
            if let Ok(path) = std::env::var("RAZDOR_BENCH_END") {
                let mut shot = get_screen_data();
                opaque_rgba(&mut shot.bytes);
                shot.export_png(&path);
            }
            exit_now(&mut app);
        }
        // The frame is drawn: a trailer run records it, and ends when its last frame is written.
        if ui::trailer::after_frame(&mut app) {
            exit_now(&mut app);
        }
        if quiet {
            app.dialogs.clear();
            // `RAZDOR_SCENE_QUIET`: the map's opening windows are not shown. Each frame one that is open closes as
            // its OK closes it (`event_window_closed`), so a window that opens a frame late does not stay.
            if let Some(game) = app.game.as_mut() {
                if game.event_window().is_some() {
                    let _ = game.event_window_closed();
                    game.drain_events();
                }
            }
        }
        let work = started.elapsed();
        frames += 1;
        // The snapshot clock moves one frame on (live runs: no-op).
        ui::clock::tick();
        if let Some((path, n)) = &snapshot {
            // `RAZDOR_SNAPSHOT_SERIES=<count>:<every>`: a picture every `every` frames from `n` (numbered), and the
            // run ends with the last of them. Without it, the one picture at `n`, as before.
            let series = ui::snapshot::series();
            let at = frames.saturating_sub(*n);
            let save = match series {
                None => frames >= *n,
                Some((_, every)) => frames >= *n && at % every == 0,
            };
            if save {
                ui::text_probe::write();
                ui::touch_probe::write();
                let mut shot = get_screen_data();
                opaque_rgba(&mut shot.bytes);
                match series {
                    None => shot.export_png(path),
                    Some(_) => shot.export_png(&ui::snapshot::series_path(path, at)),
                }
            }
            let last = match series {
                None => *n,
                Some((count, every)) => *n + (count - 1) * every,
            };
            if frames >= last {
                exit_now(&mut app);
            }
        }
        if is_quit_requested() || app.quit || quit_after.is_some_and(|n| frames >= n) {
            exit_now(&mut app);
        }
        ui::widgets::sync_keyboard();
        // The map frame's last GPU wait (profiling, Android): the frame's draws, cursor included.
        ui::gpu_probe::gpu_wait_ui();
        next_frame().await;
        pacer.pace(ui::frame_cap::current(), snapshot.is_none() && !ui::trailer::active());
        // The performance test's frame: its work and its whole time (`ui::bench`, a no-op when no test runs).
        ui::bench::sample(&mut app, work, started.elapsed());
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
