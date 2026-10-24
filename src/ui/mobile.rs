//! The phones' not-found screen and its install poll, for Android and iOS (the platform-neutral
//! part; the Android JNI calls are in `ui::android`). The title screen shows a block with
//! «Check again» while no install is found; the player's press, or a poll every
//! [`POLL_SECONDS`] that finds the install in place, loads it without a restart.
//!
//! The storage is the only difference: Android needs the all-files access (see
//! `ui::android::storage_granted`), iOS has no permission at all (the app's Documents folder).

use razdor::dt::unpack::{self, Found, Kind, UnpackError};
use razdor::local;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

/// How often the not-found screen looks for the install while the storage is granted.
const POLL_SECONDS: f64 = 5.0;
/// How often the not-found screen looks for game archives in the folders (`dt::unpack`).
const SCAN_SECONDS: f64 = 5.0;

/// The unpacking of the game archives on a thread: its handle (None for a snapshot's fake
/// progress) and the bytes done and in all.
pub struct Job {
    handle: Option<JoinHandle<Result<(), UnpackError>>>,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
}

/// The unpacking's state on the not-found screen.
enum Unpack {
    Idle,
    Running(Job),
    /// The last try failed: the error, for the player.
    Failed(UnpackError),
}

local! {
    /// The player asked to look again ("Check again" on the not-found screen).
    static RECHECK: Cell<bool> = const { Cell::new(false) };
    /// The next time the not-found screen polls for the install (macroquad's clock).
    static NEXT_POLL: Cell<f64> = const { Cell::new(0.0) };
    /// The next time the not-found screen looks for game archives.
    static NEXT_SCAN: Cell<f64> = const { Cell::new(0.0) };
    /// The archives of the last look (game archives and patches), for the not-found block.
    static ARCHIVES: RefCell<Vec<Found>> = const { RefCell::new(Vec::new()) };
    /// Set by a snapshot's fake archives: the looks do not replace them.
    static FAKED: Cell<bool> = const { Cell::new(false) };
    static UNPACK: RefCell<Unpack> = const { RefCell::new(Unpack::Idle) };
    /// The screen size the safe area was last read for, and the frames since the last read.
    static INSETS_SEEN: Cell<(f32, f32, u32)> = const { Cell::new((0.0, 0.0, 0)) };
}

/// Frames between two reads of the safe area: the system's insets settle a little after a turn,
/// so they are read again now and then (a few JNI calls, twice a second at 60 fps).
#[cfg(any(target_os = "android", target_os = "ios"))]
const INSETS_EVERY: u32 = 30;

/// Reads the phone's safe area into `platform::safe_insets` when the screen turns, and every
/// [`INSETS_EVERY`] frames. A turn the system has not laid out yet is read again on the next
/// frame. Called on Android and iOS only (desktop keeps its margins at zero).
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn refresh_safe_insets() {
    use macroquad::window::{screen_height, screen_width};
    let (w, h) = (screen_width(), screen_height());
    let (seen_w, seen_h, frames) = INSETS_SEEN.with(|c| c.get());
    let turned = (w, h) != (seen_w, seen_h);
    if !turned && frames < INSETS_EVERY {
        INSETS_SEEN.with(|c| c.set((seen_w, seen_h, frames + 1)));
        return;
    }
    #[cfg(target_os = "android")]
    let read = super::android::read_safe_insets();
    #[cfg(target_os = "ios")]
    let read = Some(razdor::platform::ios_safe_margins(h >= w, macroquad::window::screen_dpi_scale()));
    if let Some(insets) = read {
        razdor::platform::set_safe_insets(insets);
        INSETS_SEEN.with(|c| c.set((w, h, 0)));
    } else {
        // Not laid out yet: the old size is kept, so a turn is read again on the next frame.
        INSETS_SEEN.with(|c| c.set((seen_w, seen_h, 0)));
    }
}

/// Whether Razdor may read the install's folder: Android's storage access; iOS has no
/// permission (the app's own Documents folder), so it is always granted there.
pub fn storage_granted() -> bool {
    #[cfg(target_os = "android")]
    {
        super::android::storage_granted()
    }
    #[cfg(not(target_os = "android"))]
    {
        true
    }
}

/// The first frame without an install asks for the storage access, once in the run (Android:
/// the all-files settings page, `ui::android::ask_once`). iOS has no permission to ask for.
pub fn ask_access_once() {
    #[cfg(target_os = "android")]
    super::android::ask_once();
}

/// The player pressed "Check again" on the not-found screen.
pub fn ask_recheck() {
    RECHECK.with(|r| r.set(true));
}

/// True once after "Check again" was pressed.
pub fn take_recheck() -> bool {
    RECHECK.with(|r| r.replace(false))
}

/// True at most every [`POLL_SECONDS`]: the not-found screen's own look for the install. Phones
/// only: the desktop has no poll (its not-found block is reloaded by the unpacking's end).
pub fn poll_due(now: f64) -> bool {
    if !cfg!(any(target_os = "android", target_os = "ios")) {
        return false;
    }
    NEXT_POLL.with(|next| {
        if now >= next.get() {
            next.set(now + POLL_SECONDS);
            true
        } else {
            false
        }
    })
}

/// A cheap look for an install (its folder, or a folder of the search roots' children): no
/// deep search, so it can run every few seconds. Finds what the full search finds there.
pub fn install_listed() -> bool {
    razdor::platform::install_roots().iter().any(|root| {
        razdor::dt::install::is_install(root)
            || std::fs::read_dir(root).into_iter().flatten().flatten().any(|e| razdor::dt::install::is_install(&e.path()))
    })
}

/// The not-found screen's look for game archives (`dt::unpack`) in [`platform::archive_dirs`]:
/// at once when `asked` (after "Check again" or the unpacking's end), else every [`SCAN_SECONDS`].
/// Does nothing while a snapshot's fake archives are in place.
pub fn look_for_archives(now: f64, asked: bool) {
    if FAKED.with(|f| f.get()) {
        return;
    }
    let due = NEXT_SCAN.with(|next| {
        if asked || now >= next.get() {
            next.set(now + SCAN_SECONDS);
            true
        } else {
            false
        }
    });
    if due {
        let found = unpack::find_archives(&razdor::platform::archive_dirs());
        ARCHIVES.with_borrow_mut(|archives| *archives = found);
    }
}

/// The names of the game archives found by the last look, for the not-found block.
pub fn game_archive_names() -> Vec<String> {
    ARCHIVES.with_borrow(|archives| archives.iter().filter(|f| f.kind == Kind::Game).map(|f| f.name.clone()).collect())
}

/// True while the game archives are being unpacked.
pub fn unpacking() -> bool {
    UNPACK.with_borrow(|state| matches!(state, Unpack::Running(_)))
}

/// The "Unpack the game archive" button is offered: a game archive was found and nothing runs.
pub fn unpack_offered() -> bool {
    !unpacking() && !game_archive_names().is_empty()
}

/// The unpacking's progress in percent, while it runs.
pub fn unpack_progress() -> Option<u64> {
    UNPACK.with_borrow(|state| match state {
        Unpack::Running(job) => {
            let total = job.total.load(Ordering::Relaxed);
            Some(if total == 0 { 0 } else { job.done.load(Ordering::Relaxed) * 100 / total })
        }
        _ => None,
    })
}

/// The error of the last unpacking that failed, for the player.
pub fn unpack_error() -> Option<UnpackError> {
    UNPACK.with_borrow(|state| match state {
        Unpack::Failed(error) => Some(error.clone()),
        _ => None,
    })
}

/// The player pressed "Unpack the game archive": the archives are unpacked on a thread into the
/// unpack folder ([`platform::unpack_dir`]). Its end is seen by [`unpack_tick`].
pub fn start_unpack() {
    let archives = ARCHIVES.with_borrow(|archives| archives.clone());
    let state = match razdor::platform::unpack_dir() {
        None => Unpack::Failed(UnpackError::Other("no folder to unpack into".into())),
        Some(dest) => {
            let done = Arc::new(AtomicU64::new(0));
            let total = Arc::new(AtomicU64::new(0));
            let (d, t) = (done.clone(), total.clone());
            let spawned = std::thread::Builder::new().name("unpack".into()).spawn(move || {
                unpack::unpack(&archives, &dest, &|bytes, all| {
                    d.store(bytes, Ordering::Relaxed);
                    t.store(all, Ordering::Relaxed);
                })
            });
            match spawned {
                Ok(handle) => Unpack::Running(Job { handle: Some(handle), done, total }),
                Err(e) => Unpack::Failed(UnpackError::Other(e.to_string())),
            }
        }
    };
    UNPACK.with_borrow_mut(|s| *s = state);
}

/// Each frame: when the unpacking thread has ended, its outcome. Success asks for the install
/// ([`ask_recheck`]), which the poll loads; a failure is kept for the block to show.
pub fn unpack_tick() {
    let ended = UNPACK.with_borrow_mut(|state| {
        let finished = matches!(state, Unpack::Running(job) if job.handle.as_ref().is_some_and(|h| h.is_finished()));
        if !finished {
            return None;
        }
        match std::mem::replace(state, Unpack::Idle) {
            Unpack::Running(job) => job.handle.map(|h| h.join()),
            _ => None,
        }
    });
    match ended {
        Some(Ok(Ok(()))) => ask_recheck(),
        Some(Ok(Err(error))) => UNPACK.with_borrow_mut(|s| *s = Unpack::Failed(error)),
        Some(Err(_)) => UNPACK.with_borrow_mut(|s| *s = Unpack::Failed(UnpackError::Other("the unpacking stopped unexpectedly".into()))),
        None => {}
    }
}

/// The snapshot's fake state (`RAZDOR_SCENE_ARCHIVES=a.7z,b.zip` and `RAZDOR_SCENE_UNPACK=<pct>`):
/// those game archives are found, and the unpacking shows that progress. Nothing is read.
pub fn snapshot_archives(names: &str, pct: Option<u64>) {
    FAKED.with(|f| f.set(true));
    let found = names
        .split(',')
        .filter(|n| !n.is_empty())
        .map(|n| Found { path: PathBuf::from(n), name: n.to_string(), kind: Kind::Game, strip: String::new(), size: 0 })
        .collect();
    ARCHIVES.with_borrow_mut(|archives| *archives = found);
    if let Some(pct) = pct {
        let job = Job { handle: None, done: Arc::new(AtomicU64::new(pct)), total: Arc::new(AtomicU64::new(100)) };
        UNPACK.with_borrow_mut(|s| *s = Unpack::Running(job));
    }
}

/// iOS: opens `url` in the browser through `UIApplication`'s `openURL:options:completionHandler:`
/// (no completion handler, no options). A link that does not parse is logged, not sent.
#[cfg(target_os = "ios")]
pub fn open_url(url: &str) {
    use macroquad::miniquad::native::apple::frameworks::{class, msg_send, nil, sel, sel_impl, ObjcId};
    let Ok(text) = std::ffi::CString::new(url) else { return };
    // SAFETY: the Foundation and UIKit classes are the app's own; every object is autoreleased
    // by its class factory, and the strings and the URL live through the call.
    unsafe {
        let ns_string: ObjcId = msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()];
        let ns_url: ObjcId = msg_send![class!(NSURL), URLWithString: ns_string];
        if ns_url == nil {
            razdor::diag!("could not open {url}");
            return;
        }
        let options: ObjcId = msg_send![class!(NSDictionary), dictionary];
        let app: ObjcId = msg_send![class!(UIApplication), sharedApplication];
        let () = msg_send![app, openURL: ns_url options: options completionHandler: nil];
    }
}
