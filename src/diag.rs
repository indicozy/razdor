//! Diagnostics: a log of every start, and a report when the game fails.
//!
//! [`init`] opens `razdor.log` in the data folder (`%APPDATA%\razdor` on Windows,
//! `~/.local/share/razdor` on Linux; the one before is kept as `razdor.previous.log`) and
//! writes what is needed to tell why a start failed: version, system, program and working
//! folders, the `RAZDOR_*` variables, then each step of the start ([`step`]). Started without
//! a terminal (a double click, or a Windows release build), everything the program and its
//! libraries print goes into the log too. A panic is written with its backtrace and, on
//! Windows, shown in a message box naming the log; so is a crash (an unhandled exception).
//!
//! `RAZDOR_LOG=<file>` puts the log elsewhere. `RAZDOR_CRASH_TEST=panic` or `=crash` fails on
//! purpose right after the start is logged, to see the report on a player's machine.

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

struct Log {
    file: Option<File>,
    /// Standard error is the log file itself: [`log`] writes only once.
    stderr_is_file: bool,
}

static LOG: Mutex<Log> = Mutex::new(Log { file: None, stderr_is_file: false });
static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
static PATH: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

thread_local! {
    /// Panics expected and caught by the caller ([`quiet`]): logged, never shown.
    static QUIET: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Writes a line to the log (and to the terminal when there is one).
#[macro_export]
macro_rules! diag {
    ($($arg:tt)*) => { $crate::diag::log(&format!($($arg)*)) };
}

/// The log file: `RAZDOR_LOG`, else `razdor.log` in the platform's data folder.
pub fn log_path() -> Option<PathBuf> {
    PATH.get_or_init(|| {
        std::env::var_os("RAZDOR_LOG")
            .map(PathBuf::from)
            .or_else(|| crate::platform::app_dir().map(|d| d.join("razdor.log")))
    })
    .clone()
}

fn elapsed() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Writes `msg` to the log, one line per line, with the time since the start.
pub fn log(msg: &str) {
    let t = elapsed();
    let mut text = String::new();
    for line in msg.lines() {
        text.push_str(&format!("[{t:8.3}] {line}\n"));
    }
    let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(f) = log.file.as_mut() {
        let _ = f.write_all(text.as_bytes());
        let _ = f.flush();
    }
    if !log.stderr_is_file {
        let _ = std::io::stderr().write_all(text.as_bytes());
    }
}

/// `RAZDOR_PROFILE` is set: slow frames and slow loads are logged.
pub fn profiling() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RAZDOR_PROFILE").is_some())
}

/// Runs `f`; with [`profiling`] on, logs `what` when it took over 2 ms.
pub fn timed<T>(what: impl FnOnce() -> String, f: impl FnOnce() -> T) -> T {
    if !profiling() {
        return f();
    }
    let started = Instant::now();
    let r = f();
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    if ms > 2.0 {
        log(&format!("slow load: {} {ms:.1} ms", what()));
    }
    r
}

/// A step of the start or of the game: if the log ends here, this is where it stopped.
pub fn step(what: &str) {
    log(&format!("-- {what}"));
}

/// Runs `f`, whose panics the caller catches and handles: they are logged, not reported.
pub fn quiet<T>(f: impl FnOnce() -> T) -> T {
    QUIET.with(|q| q.set(q.get() + 1));
    let r = f();
    QUIET.with(|q| q.set(q.get() - 1));
    r
}

/// Opens the log, redirects the output there when there is no terminal, installs the panic
/// and crash reports and writes the system summary. Call first, once.
pub fn init() {
    elapsed();
    PLAY_ON.store(true, std::sync::atomic::Ordering::Relaxed);
    let path = log_path();
    let file = path.as_ref().and_then(|p| {
        let _ = p.parent().map(std::fs::create_dir_all);
        let _ = std::fs::rename(p, p.with_file_name("razdor.previous.log"));
        File::create(p).ok()
    });
    let stderr_is_file = file.as_ref().is_some_and(redirect_output);
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Log { file, stderr_is_file };
    install_panic_hook();
    platform::install_crash_handler();
    summary(path.as_deref());
    match std::env::var("RAZDOR_CRASH_TEST").as_deref() {
        Ok("panic") => panic!("RAZDOR_CRASH_TEST=panic"),
        // SAFETY: not safe at all: a deliberate access violation, to test the crash report.
        Ok("crash") => unsafe { std::ptr::null_mut::<u8>().write_volatile(1) },
        _ => {}
    }
}

/// The author's credit, from Cargo.toml's `authors`. The start of the log prints it, so it
/// stays in every build of the program as one string (`strings Razdor.exe` finds it).
pub const CREDIT: &str = concat!("made by ", env!("CARGO_PKG_AUTHORS"));

fn summary(path: Option<&std::path::Path>) {
    let mut s = format!(
        "Razdor {} ({}), {}, {} {}",
        env!("CARGO_PKG_VERSION"),
        option_env!("RAZDOR_GIT").unwrap_or("unknown commit"),
        CREDIT,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    if let Some(v) = platform::os_version() {
        s.push_str(&format!(", {v}"));
    }
    s.push_str(&format!("\nlog: {}", path.map_or("(none)".into(), |p| p.display().to_string())));
    s.push_str(&format!("\nprogram: {:?}", std::env::current_exe().ok()));
    s.push_str(&format!("\nworking folder: {:?}", std::env::current_dir().ok()));
    s.push_str(&format!("\narguments: {:?}", std::env::args().skip(1).collect::<Vec<_>>()));
    for (k, v) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("RAZDOR_") {
            s.push_str(&format!("\n{}={:?}", k.to_string_lossy(), v));
        }
    }
    log(&s);
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed").to_string();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let place = info.location().map_or(String::new(), |l| format!(" at {}:{}", l.file(), l.line()));
        let quiet = QUIET.with(|q| q.get() > 0);
        if quiet {
            log(&format!("caught panic in thread '{name}'{place}: {msg}"));
            return;
        }
        let bt = std::backtrace::Backtrace::force_capture();
        log(&format!("PANIC in thread '{name}'{place}: {msg}\nbacktrace:\n{bt}"));
        // Only the main thread's panic ends the game (the audio thread's does not).
        if name == "main" {
            match known_cause(&msg) {
                Some(help) => {
                    log(help);
                    fatal(&format!("{help}\n\n({msg})"));
                }
                None => fatal(&format!("{msg}{place}")),
            }
        }
    }));
}

/// What a player can do about a failure that has a known cause outside the game, in Russian
/// and English (it can come before the interface language is known).
fn known_cause(msg: &str) -> Option<&'static str> {
    // miniquad needs WGL_ARB_pixel_format (and a real OpenGL 2 context) from the Windows
    // driver. Windows' own fallback, OpenGL 1.1 "GDI Generic", has neither: no graphics
    // driver, or a Remote Desktop session, which hides the GPU from OpenGL programs.
    (msg.contains("WGL_ARB_pixel_format") || msg.starts_with("WGL:")).then_some(
        "Не найден драйвер OpenGL. Так бывает в сеансе удалённого рабочего стола (RDP) или без драйвера видеокарты.\n\
         Что сделать: положите opengl32.dll из Mesa (github.com/pal1000/mesa-dist-win, папка x64) рядом с Razdor.exe; \
         или подключитесь не через RDP (Parsec, VNC, консоль ВМ); или установите драйвер видеокарты.\n\n\
         No OpenGL driver was found. This happens in a Remote Desktop (RDP) session or without a graphics driver.\n\
         What to do: put Mesa's opengl32.dll (github.com/pal1000/mesa-dist-win, x64 folder) next to Razdor.exe; \
         or connect another way than RDP (Parsec, VNC, the VM's console); or install the graphics driver.",
    )
}

/// The play log, `razdor-play.log` next to [`log_path`] (the one before kept as
/// `razdor-play.previous.log`): what the player did and what the game did, with the in-game
/// time, for reading back when something played wrong. Opened on the first line.
static PLAY: Mutex<Option<File>> = Mutex::new(None);
/// The play log runs only in the game ([`init`]); tests and tools write none.
static PLAY_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Where the play log is written.
pub fn play_log_path() -> Option<PathBuf> {
    log_path().map(|p| p.with_file_name("razdor-play.log"))
}

/// Writes `msg` (one or more lines) to the play log, each line after `when` (the in-game date
/// and time, or a label such as "menu").
pub fn play(when: &str, msg: &str) {
    if !PLAY_ON.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let mut f = PLAY.lock().unwrap_or_else(|e| e.into_inner());
    if f.is_none() {
        let Some(path) = play_log_path() else { return };
        let _ = path.parent().map(std::fs::create_dir_all);
        let _ = std::fs::rename(&path, path.with_file_name("razdor-play.previous.log"));
        *f = File::create(&path).ok();
    }
    if let Some(file) = f.as_mut() {
        let t = elapsed();
        let mut text = String::new();
        for line in msg.lines() {
            text.push_str(&format!("[{t:8.1}] {when} | {line}\n"));
        }
        let _ = file.write_all(text.as_bytes());
        let _ = file.flush();
    }
}

/// Tells the player the game has failed and where the log is.
pub fn fatal(what: &str) {
    let log = log_path().map_or(String::new(), |p| format!("\n\nDetails: {}", p.display()));
    platform::message_box("Razdor has failed", &format!("Razdor has failed to run:\n\n{what}{log}"));
}

/// Without a terminal, standard output and error go into the log file. True when they do.
fn redirect_output(file: &File) -> bool {
    platform::redirect_output(file)
}

#[cfg(test)]
mod tests {
    use super::known_cause;

    #[test]
    fn a_missing_opengl_driver_is_explained() {
        assert!(known_cause("WGL_ARB_pixel_format is required").is_some_and(|h| h.contains("opengl32.dll")));
        assert!(known_cause("WGL: Failed to create dummy context").is_some());
        assert!(known_cause("index out of bounds: the len is 3 but the index is 7").is_none());
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::fs::File;
    use std::os::windows::io::AsRawHandle;

    type Handle = *mut c_void;
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    const MB_ICONERROR: u32 = 0x10;
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

    #[repr(C)]
    struct ExceptionRecord {
        code: u32,
        flags: u32,
        record: *mut ExceptionRecord,
        address: *mut c_void,
        parameters: u32,
        information: [usize; 15],
    }

    #[repr(C)]
    struct ExceptionPointers {
        record: *mut ExceptionRecord,
        context: *mut c_void,
    }

    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }

    type Filter = unsafe extern "system" fn(*mut ExceptionPointers) -> i32;

    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(pid: u32) -> i32;
        fn GetConsoleWindow() -> Handle;
        fn SetStdHandle(which: u32, handle: Handle) -> i32;
        fn SetUnhandledExceptionFilter(filter: Option<Filter>) -> Option<Filter>;
    }

    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(wnd: Handle, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }

    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfo) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn message_box(caption: &str, text: &str) {
        let (caption, text) = (wide(caption), wide(text));
        // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call.
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_ICONERROR) };
    }

    /// Started from a console (`cmd`, PowerShell), the output goes there; otherwise into the
    /// log file.
    pub fn redirect_output(file: &File) -> bool {
        // SAFETY: plain Win32 calls; the handle is the log file's, kept open for the whole run
        // by the log.
        unsafe {
            if !GetConsoleWindow().is_null() || AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
                return false;
            }
            let h = file.as_raw_handle() as Handle;
            SetStdHandle(STD_OUTPUT_HANDLE, h) != 0 && SetStdHandle(STD_ERROR_HANDLE, h) != 0
        }
    }

    unsafe extern "system" fn on_crash(info: *mut ExceptionPointers) -> i32 {
        // SAFETY: Windows passes valid exception pointers to the filter.
        let (code, address) = unsafe {
            let r = &*(*info).record;
            (r.code, r.address)
        };
        let what = format!("crash: exception 0x{code:08X} at {address:?}{}", name_of(code));
        super::log(&format!("{what}\nbacktrace:\n{}", std::backtrace::Backtrace::force_capture()));
        super::fatal(&what);
        EXCEPTION_CONTINUE_SEARCH
    }

    fn name_of(code: u32) -> &'static str {
        match code {
            0xC000_0005 => " (access violation)",
            0xC000_001D => " (illegal instruction: the processor lacks an instruction)",
            0xC000_00FD => " (stack overflow)",
            0xC000_0135 => " (a DLL was not found)",
            0xC000_0139 => " (an entry point was not found in a DLL)",
            0xC000_0409 => " (stack buffer overrun / fast fail)",
            _ => "",
        }
    }

    pub fn install_crash_handler() {
        // SAFETY: installs a process-wide filter; `on_crash` has the required signature.
        unsafe { SetUnhandledExceptionFilter(Some(on_crash)) };
    }

    pub fn os_version() -> Option<String> {
        let mut v = OsVersionInfo { size: 0, major: 0, minor: 0, build: 0, platform: 0, service_pack: [0; 128] };
        v.size = std::mem::size_of::<OsVersionInfo>() as u32;
        // SAFETY: `v` is a properly sized OSVERSIONINFOW.
        (unsafe { RtlGetVersion(&mut v) } == 0).then(|| format!("Windows {}.{} build {}", v.major, v.minor, v.build))
    }
}

#[cfg(unix)]
mod platform {
    use std::fs::File;
    use std::os::unix::io::AsRawFd;

    pub fn message_box(_caption: &str, _text: &str) {}

    /// Without a terminal on standard error (a double click in a file manager), the output
    /// goes into the log file.
    pub fn redirect_output(file: &File) -> bool {
        // SAFETY: isatty and dup2 on descriptors we own; the log file stays open.
        unsafe {
            if libc::isatty(2) == 1 {
                return false;
            }
            libc::dup2(file.as_raw_fd(), 1) >= 0 && libc::dup2(file.as_raw_fd(), 2) >= 0
        }
    }

    pub fn install_crash_handler() {}

    pub fn os_version() -> Option<String> {
        let text = std::fs::read_to_string("/etc/os-release").ok()?;
        let name = text.lines().find_map(|l| l.strip_prefix("PRETTY_NAME="))?;
        Some(name.trim_matches('"').to_string())
    }
}

#[cfg(not(any(windows, unix)))]
mod platform {
    pub fn message_box(_caption: &str, _text: &str) {}
    pub fn redirect_output(_file: &std::fs::File) -> bool {
        false
    }
    pub fn install_crash_handler() {}
    pub fn os_version() -> Option<String> {
        None
    }
}
