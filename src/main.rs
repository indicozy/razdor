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
fn single_instance() {
    use std::os::unix::io::AsRawFd;
    static LOCK: std::sync::OnceLock<std::fs::File> = std::sync::OnceLock::new();
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
    let (window_width, window_height) = ui::snapshot::size().unwrap_or((1280, 800));
    razdor::diag::step(&format!("opening the window ({window_width}x{window_height}, OpenGL)"));
    Conf {
        window_title: "Razdor".to_owned(),
        window_width,
        window_height,
        high_dpi: true,
        ..Default::default()
    }
}

/// Debug: `RAZDOR_QUIT_AFTER=<frames>` asks to quit after that many frames, to test the exit
/// path without a window manager.
fn quit_after() -> Option<u64> {
    std::env::var("RAZDOR_QUIT_AFTER").ok()?.trim().parse().ok()
}

/// Ends the process without running the C `atexit` handlers.
///
/// A normal return from `main` crashed with SIGSEGV every time: `exit` runs the exit handlers
/// of the native libraries (GL / X11 / ALSA under miniquad), and one of them calls into code
/// that is no longer mapped. Everything Razdor writes (saves, `audio.json`) is written and
/// closed synchronously before this, so skipping the handlers loses nothing.
fn exit_now(app: &mut App) -> ! {
    razdor::diag::step("quitting");
    app.shutdown();
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
    {
        let gl = unsafe { get_internal_gl() };
        let info = gl.quad_context.info();
        razdor::diag::step(&format!("window open: {:?}, {}", info.backend, info.gl_version_string));
    }
    ui::widgets::load_font().await;
    razdor::diag::step("fonts loaded");
    ui::language::init();
    let content = Arc::new(Content::builtin());
    razdor::diag::step("loading the art and sounds");
    let mut app = App::new(Assets::load(content.clone()).await, content);
    razdor::diag::step("started");
    // `--editor`: start in the map editor.
    if std::env::args().skip(1).any(|a| a == "--editor") {
        app.open_editor();
    }
    ui::snapshot::stage(&mut app);
    let snapshot = ui::snapshot::target();
    let quiet = ui::snapshot::quiet();
    let quit_after = quit_after();
    let mut frames = 0u64;
    // RAZDOR_PROFILE=1: frames whose work takes over 40 ms are logged with their screen.
    let profile = std::env::var_os("RAZDOR_PROFILE").is_some();
    loop {
        let started = std::time::Instant::now();
        let before = app.screen_name();
        app.frame();
        app.draw_fps();
        // The original draws its pointer last, over everything (0x474a81).
        ui::cursor::draw();
        if quiet {
            app.dialogs.clear();
        }
        let work = started.elapsed();
        frames += 1;
        if let Some((path, n)) = &snapshot {
            if frames >= *n {
                get_screen_data().export_png(path);
                exit_now(&mut app);
            }
        }
        if is_quit_requested() || app.quit || quit_after.is_some_and(|n| frames >= n) {
            exit_now(&mut app);
        }
        next_frame().await;
        if frames == 1 {
            razdor::diag::step("first frame shown");
        }
        if profile {
            // The whole frame: the game's work, then drawing and the GPU (textures and font
            // atlases go up there), vsync included (~16 ms). Over 100 ms is a visible hitch.
            let total = started.elapsed();
            if total.as_millis() > 100 {
                let (w, r) = (work.as_secs_f32() * 1000.0, (total - work).as_secs_f32() * 1000.0);
                razdor::diag!("slow frame: {:.0} ms = work {w:.0} + render {r:.0} ({before} -> {})", w + r, app.screen_name());
            }
        }
    }
}
