//! «Обновить игру из архива…» in the settings window of the main menu: the platform's file picker
//! (as «Добавить карту…», `ui::map_import`), the archive examined, a question before a whole game
//! replaces the install, and the import on a thread with its progress (`dt::game_import`).
//!
//! Only the phones' unpack folder is written (`dt::game_import::target`): a player's own copy
//! elsewhere, a `RAZDOR_DT_DIR` or a Steam or GOG folder is never replaced, and the row says so.
//! The row is not offered over a map (the game under way reads the install).
//!
//! The pick: the desktop's dialog (`rfd`) on a thread, or on the main thread on macOS; Android's
//! document picker through the JNI (`ui::android`), which the Java side copies into the app's
//! cache by streaming; iOS has no picker, so the row says so.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use macroquad::prelude::*;
use razdor::dt::game_import::{self as import, Done, Error};
use razdor::dt::unpack::{Found, Kind, UnpackError};
use razdor::gdrive;
use razdor::hub::{self, Hub, Source};
use razdor::i18n::tr;
use razdor::local;
use razdor::platform;
use razdor::trf;

use super::audio::{cue, Cue};
use super::chrome;
use super::map_import;
use super::widgets::*;

/// The window's title.
fn title() -> &'static str {
    tr("Update the game")
}

/// The picked file: its name, where its bytes are, and whether the copy there is Razdor's to delete.
struct Picked {
    name: String,
    path: PathBuf,
    cached: bool,
}

/// Where the pick stands.
#[derive(Default)]
enum Picker {
    #[default]
    Idle,
    /// The desktop dialog's answer (`None`: cancelled), sent by its thread.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    Desktop(std::sync::mpsc::Receiver<Option<PathBuf>>),
    /// The Android picker is open: its answer comes through `ui::android::pick_outcome`.
    #[cfg(target_os = "android")]
    Android,
}

/// An archive examined and waiting for the player's answer: the install folder it goes to, and
/// the cache copy to delete when it is done with.
struct Pending {
    found: Found,
    target: PathBuf,
    cleanup: Option<PathBuf>,
}

/// A window of this module, shown over the settings until the player answers it.
enum Modal {
    /// The whole game is to be replaced: Replace or Cancel.
    Confirm(Pending),
    /// What happened, or why nothing did: OK.
    Notice { title: String, lines: Vec<String> },
}

/// The import running on a thread: its handle and the bytes done and in all.
struct Job {
    name: String,
    handle: Option<JoinHandle<Result<Done, Error>>>,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    /// The game folder's download (`gdrive`), when this job is one: its files and cancel.
    progress: Option<Arc<gdrive::Progress>>,
    /// The window's title: the import's, the download's, or the repair's.
    title: &'static str,
    /// The automatic download at start (`auto_tick`): its end shows no notice.
    auto: bool,
    /// The archive's size, for the download's window (0 for an unpacking).
    size: u64,
}

/// The automatic start with no install (`auto_tick`): the hub's catalog is read on a thread
/// (`Checking`), and the game it offers is then downloaded as a job of its own. `Done` once the
/// start is decided (a game downloads, or not: the no-install screen stays as it is).
enum Auto {
    Idle,
    /// The catalog is being read; `None` is the snapshot's frozen check (`RAZDOR_SCENE_CATALOG`).
    Checking(Option<std::sync::mpsc::Receiver<Result<Option<gdrive::Archive>, String>>>),
    Done,
}

local! {
    static REQUESTED: Cell<bool> = const { Cell::new(false) };
    static PICKER: RefCell<Picker> = const { RefCell::new(Picker::Idle) };
    static MODAL: RefCell<Option<Modal>> = const { RefCell::new(None) };
    static JOB: RefCell<Option<Job>> = const { RefCell::new(None) };
    /// The folder an import writes to this frame (`dt::game_import::target`), None when not offered.
    static TARGET: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static DOWNLOAD_REQUESTED: Cell<bool> = const { Cell::new(false) };
    static REPAIR_REQUESTED: Cell<bool> = const { Cell::new(false) };
    /// The last game download's failure, one line, for the no-install screen (None: none).
    static DOWNLOAD_FAILURE: RefCell<Option<String>> = const { RefCell::new(None) };
    static AUTO: RefCell<Auto> = const { RefCell::new(Auto::Idle) };
}

/// «Скачать игру» was pressed: the download starts at the next frame (`tick`), unless an import
/// or a download runs.
pub(super) fn request_download() {
    DOWNLOAD_REQUESTED.with(|r| r.set(true));
}

/// Whether the no-install screen offers the game download (not on iPhone: its import is not there yet).
pub fn download_available() -> bool {
    !cfg!(target_os = "ios")
}

/// Whether the no-install screen offers picking a game archive by hand: the desktop's dialog and
/// Android's document picker (iPhone has no picker here).
pub fn pick_available() -> bool {
    !cfg!(target_os = "ios")
}

/// «Восстановить файлы игры» was pressed: the missing game files come back from the archive
/// (`dt::integrity`), at the next frame (`tick`), unless an import or a download runs.
pub(super) fn request_repair() {
    REPAIR_REQUESTED.with(|r| r.set(true));
}

/// The last game download's failure, in one line, while the screen shows the manual steps.
pub fn download_failure() -> Option<String> {
    // The snapshot's `RAZDOR_SCENE_DOWNLOAD_FAILED` shows the manual path after a failed download.
    if std::env::var_os("RAZDOR_SNAPSHOT").is_some() && std::env::var_os("RAZDOR_SCENE_DOWNLOAD_FAILED").is_some() {
        return Some(trf!("Download failed: {why}.", why = "could not resolve the host"));
    }
    DOWNLOAD_FAILURE.with_borrow(|f| f.clone())
}

/// The row's button was pressed: the picker opens at the next frame (`tick`).
pub(super) fn request() {
    REQUESTED.with(|r| r.set(true));
}

/// True while the import has a window or a job of its own: the settings under it take no input.
pub fn busy() -> bool {
    MODAL.with_borrow(|m| m.is_some()) || JOB.with_borrow(|j| j.is_some()) || PICKER.with_borrow(|p| !matches!(p, Picker::Idle))
}

/// Whether the row offers the import: the install is the phones' unpack folder (or there is none
/// yet). The snapshot's `RAZDOR_SCENE_IMPORT` shows the offer without an install.
fn offered() -> bool {
    TARGET.with_borrow(|t| t.is_some()) || snapshot_offer()
}

fn snapshot_offer() -> bool {
    std::env::var_os("RAZDOR_SNAPSHOT").is_some() && std::env::var_os("RAZDOR_SCENE_IMPORT").is_some()
}

/// Each frame: sets the folder an import writes to (`install` is the current install's folder),
/// opens a requested picker, takes its answer and finishes a job. True when the install may have
/// changed (the caller reloads it).
pub fn tick(install: Option<&Path>) -> bool {
    let unpack = platform::unpack_dir();
    let target = import::target(install, unpack.as_deref());
    TARGET.with_borrow_mut(|t| *t = target);
    if REQUESTED.with(|r| r.replace(false)) && !busy() && offered() {
        start_picker();
    }
    if DOWNLOAD_REQUESTED.with(|r| r.replace(false)) && !busy() {
        start_download();
    }
    if REPAIR_REQUESTED.with(|r| r.replace(false)) && !busy() {
        start_repair();
    }
    let changed = finish_job();
    auto_tick(install.is_some());
    match poll_picker() {
        Poll::Nothing | Poll::Cancelled => {}
        #[cfg(target_os = "android")]
        Poll::Failed(why) => notice(vec![why]),
        Poll::Got(picked) => picked_archive(picked),
    }
    changed
}

/// The picked archive: examined (nothing is written), then either started, or asked about when it
/// replaces a game that is there.
fn picked_archive(picked: Picked) {
    let cleanup = picked.cached.then(|| picked.path.clone());
    let discard = |cleanup: &Option<PathBuf>| {
        if let Some(copy) = cleanup {
            let _ = std::fs::remove_file(copy);
        }
    };
    let Some(target) = TARGET.with_borrow(|t| t.clone()) else {
        discard(&cleanup);
        notice(vec![tr("Only the game in Razdor's own folder can be updated here.").to_string()]);
        return;
    };
    let found = match import::examine(&picked.path, &picked.name) {
        Ok(found) => found,
        Err(e) => {
            discard(&cleanup);
            notice(vec![error_text(&e)]);
            return;
        }
    };
    let installed = target.is_dir();
    if found.kind == Kind::Patch && !installed {
        discard(&cleanup);
        notice(vec![error_text(&Error::NoInstall)]);
        return;
    }
    let pending = Pending { found, target, cleanup };
    if pending.found.kind == Kind::Game && installed {
        MODAL.with_borrow_mut(|m| *m = Some(Modal::Confirm(pending)));
    } else {
        start(pending);
    }
}

/// Starts the import on a thread; its end is seen by [`finish_job`].
fn start(pending: Pending) {
    let name = pending.found.name.clone();
    let total = Arc::new(AtomicU64::new(pending.found.size));
    let done = Arc::new(AtomicU64::new(0));
    let (d, t) = (done.clone(), total.clone());
    let spawned = std::thread::Builder::new().name("game-import".into()).spawn(move || {
        let Pending { found, target, cleanup } = pending;
        let result = import::import(&found, &target, &|bytes, all| {
            d.store(bytes, Ordering::Relaxed);
            t.store(all, Ordering::Relaxed);
        });
        if let Some(copy) = cleanup {
            let _ = std::fs::remove_file(copy);
        }
        result
    });
    match spawned {
        Ok(handle) => JOB.with_borrow_mut(|j| *j = Some(Job { name, handle: Some(handle), done, total, progress: None, title: title(), auto: false, size: 0 })),
        Err(e) => notice(vec![trf!("The import cannot start: {e}.", e = e)]),
    }
}

/// «Скачать игру»: the game on Drive (`gdrive::GAME`) is downloaded and imported (`start_archive`).
fn start_download() {
    start_archive(gdrive::GAME, false);
}

/// The game archive `archive` is downloaded into the cache folder (`gdrive`), verified, examined and
/// imported into the target (the same import as «Обновить игру из архива…») on a thread, with its
/// progress. The file is removed after the import; a failed download keeps its partial copy for the
/// next press. Only with no install (the screen offers it there). `auto`: the start's own download
/// (`auto_tick`), whose end shows no notice.
fn start_archive(archive: gdrive::Archive, auto: bool) {
    let Some(target) = TARGET.with_borrow(|t| t.clone()) else {
        notice(vec![tr("Only the game in Razdor's own folder can be updated here.").to_string()]);
        return;
    };
    DOWNLOAD_FAILURE.with_borrow_mut(|f| *f = None);
    let Some(dest) = archive_file() else {
        DOWNLOAD_FAILURE.with_borrow_mut(|f| *f = Some(trf!("The download cannot start: {e}.", e = "no folder for it")));
        return;
    };
    let size = archive.size;
    let (progress, done, total) = (Arc::new(gdrive::Progress::default()), Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    let (p, d, t) = (progress.clone(), done.clone(), total.clone());
    let spawned = std::thread::Builder::new().name("game-download".into()).spawn(move || download_and_import(&archive, &dest, &target, &p, &d, &t));
    match spawned {
        Ok(handle) => JOB.with_borrow_mut(|j| {
            *j = Some(Job { name: "Discord Times".into(), handle: Some(handle), done, total, progress: Some(progress), title: tr("Loading the game"), auto, size })
        }),
        Err(e) => DOWNLOAD_FAILURE.with_borrow_mut(|f| *f = Some(trf!("The download cannot start: {e}.", e = e))),
    }
}

/// Whether the automatic start is reading the catalog: the no-install screen says so, in the block's place.
pub fn checking() -> bool {
    AUTO.with_borrow(|a| matches!(a, Auto::Checking(_)))
}

/// Whether the automatic download runs: its window is what the no-install screen shows.
pub fn automatic() -> bool {
    checking() || JOB.with_borrow(|j| j.as_ref().is_some_and(|j| j.auto))
}

/// The automatic start, each frame (`tick`): with no install, the hub's catalog is read once per
/// launch on a thread, and a game it offers is downloaded and installed at once (`start_auto`).
/// Anything else (no game in it, the catalog unreadable, the download or the import failing) leaves
/// the no-install screen as it is, with the failure's one line where there is one.
fn auto_tick(installed: bool) {
    let state = AUTO.with_borrow_mut(|a| std::mem::replace(a, Auto::Done));
    let next = match state {
        Auto::Done => Auto::Done,
        _ if installed => Auto::Done,
        Auto::Idle if snapshot_checking() => Auto::Checking(None),
        Auto::Idle if !auto_allowed() => Auto::Done,
        Auto::Idle => start_check(),
        Auto::Checking(None) => Auto::Checking(None),
        Auto::Checking(Some(rx)) => match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => Auto::Checking(Some(rx)),
            Ok(Ok(Some(archive))) => {
                start_auto(archive);
                Auto::Done
            }
            Ok(Ok(None)) => Auto::Done,
            Ok(Err(why)) => {
                razdor::diag!("game catalog: {why}");
                Auto::Done
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Auto::Done,
        },
    };
    AUTO.with_borrow_mut(|a| *a = next);
}

/// Whether the automatic start may run in this process: a download is offered here (not on iPhone),
/// it is not a snapshot (its fakes stand in for the download), and no folder was set by hand
/// (`RAZDOR_DT_DIR`: a folder that is not an install is the player's to fix, never written over).
fn auto_allowed() -> bool {
    download_available() && std::env::var_os("RAZDOR_SNAPSHOT").is_none() && std::env::var_os(razdor::dt::install::ENV_VAR).is_none_or(|v| v.is_empty())
}

/// The snapshot's frozen check (`RAZDOR_SCENE_CATALOG=checking`): the screen shows the check's text.
fn snapshot_checking() -> bool {
    std::env::var_os("RAZDOR_SNAPSHOT").is_some() && std::env::var("RAZDOR_SCENE_CATALOG").is_ok_and(|v| v == "checking")
}

/// Starts the check's thread: the catalog is read, and its game (if any) sent back.
fn start_check() -> Auto {
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("game-catalog".into()).spawn(move || {
        let _ = tx.send(catalog_game());
    });
    match spawned {
        Ok(_) => Auto::Checking(Some(rx)),
        Err(e) => {
            razdor::diag!("game catalog: the check cannot start: {e}");
            Auto::Done
        }
    }
}

/// The game the hub's catalog offers, read as the hub window reads it (`Hub::fetch_catalog`, which
/// keeps the catalog in the cache as the window does). None: no game in it, or no address to fetch
/// it from (a folder or the mock hub). A game's path is resolved against the hub's address.
fn catalog_game() -> Result<Option<gdrive::Archive>, String> {
    let source = Source::from_env();
    let Source::Url(base) = &source else { return Ok(None) };
    let local = hub::local_dir().ok_or("no folder for the hub")?;
    let catalog = Hub { source: source.clone(), local }.fetch_catalog()?;
    Ok(catalog.game.and_then(|g| {
        let url = hub::resolve(base, &g.url);
        url.starts_with("https://").then(|| gdrive::Archive::at_url(url, g.size, g.sha256, g.name))
    }))
}

/// Starts the game the catalog offered, when the target is there and nothing else runs.
fn start_auto(archive: gdrive::Archive) {
    if TARGET.with_borrow(|t| t.is_none()) || busy() {
        razdor::diag!("game download: not started (no folder for it, or something runs)");
        return;
    }
    razdor::diag!("game download: the catalog offers {} ({} bytes)", archive.name, archive.size);
    start_archive(archive, true);
}

/// Where the game archive is downloaded to: the cache folder (not a folder the archive search
/// scans), made if missing. None where there is no cache folder.
fn archive_file() -> Option<PathBuf> {
    let dir = platform::cache_dir()?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("game-download.zip"))
}

/// «Восстановить файлы игры» (the no-install screen, when files are missing): the archive is got
/// (cached, or downloaded and verified), and only the missing game files are written from it
/// (`dt::integrity::repair`). Changed files are never overwritten here. Same job and progress
/// as the download; the install is loaded again afterwards.
fn start_repair() {
    let Some(base) = razdor::dt::install::base_dir() else {
        DOWNLOAD_FAILURE.with_borrow_mut(|f| *f = Some(trf!("The download cannot start: {e}.", e = "no install folder")));
        return;
    };
    DOWNLOAD_FAILURE.with_borrow_mut(|f| *f = None);
    let Some(dest) = archive_file() else {
        DOWNLOAD_FAILURE.with_borrow_mut(|f| *f = Some(trf!("The download cannot start: {e}.", e = "no folder for it")));
        return;
    };
    let (progress, done, total) = (Arc::new(gdrive::Progress::default()), Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    let (p, d, t) = (progress.clone(), done.clone(), total.clone());
    let spawned = std::thread::Builder::new().name("game-repair".into()).spawn(move || repair_from_archive(&base, &dest, &p, &d, &t));
    match spawned {
        Ok(handle) => JOB.with_borrow_mut(|j| {
            *j = Some(Job { name: "Discord Times".into(), handle: Some(handle), done, total, progress: Some(progress), title: tr("Restoring the game files"), auto: false, size: gdrive::GAME.size })
        }),
        Err(e) => DOWNLOAD_FAILURE.with_borrow_mut(|f| *f = Some(trf!("The download cannot start: {e}.", e = e))),
    }
}

/// The repair thread's work (see [`start_repair`]). Its result is `Done::Patched` with the files
/// written; the job's end reloads the install.
fn repair_from_archive(base: &Path, dest: &Path, progress: &gdrive::Progress, done: &AtomicU64, total: &AtomicU64) -> Result<Done, Error> {
    let manifest = razdor::dt::integrity::builtin().map_err(Error::Io)?;
    let missing = razdor::dt::integrity::missing_files(base, &manifest);
    if missing.is_empty() {
        return Ok(Done::Patched(0));
    }
    total.store(missing.len() as u64, Ordering::Relaxed);
    gdrive::download_game(&gdrive::Http, &gdrive::GAME, dest, progress).map_err(|f| match f {
        gdrive::Failure::Cancelled => Error::Cancelled,
        gdrive::Failure::Failed(why) => Error::Download(why),
    })?;
    progress.set_downloaded();
    let result = razdor::dt::integrity::repair(base, &manifest, dest, &missing).map(Done::Patched).map_err(Error::Io);
    done.store(missing.len() as u64, Ordering::Relaxed);
    let _ = std::fs::remove_file(dest);
    result
}

/// The download thread's work: the archive into `dest` (verified), then the archive import into
/// `target`, whose unpacking reports into `done` and `total`. `dest` is removed at the end.
fn download_and_import(archive: &gdrive::Archive, dest: &Path, target: &Path, progress: &gdrive::Progress, done: &AtomicU64, total: &AtomicU64) -> Result<Done, Error> {
    gdrive::download_game(&gdrive::Http, archive, dest, progress).map_err(|f| match f {
        gdrive::Failure::Cancelled => Error::Cancelled,
        gdrive::Failure::Failed(why) => Error::Download(why),
    })?;
    progress.set_downloaded();
    let result = import::examine(dest, &archive.name).and_then(|found| {
        import::import(&found, target, &|bytes, all| {
            done.store(bytes, Ordering::Relaxed);
            total.store(all, Ordering::Relaxed);
        })
    });
    let _ = std::fs::remove_file(dest);
    result
}

/// Each frame: when the import's thread has ended, its outcome is shown. Always asks for a reload
/// of the install (a failed one may have moved folders, and the reload is then the right state).
/// A download's failure is kept for the screen (one line) instead of a window; a cancel is silent.
fn finish_job() -> bool {
    let ended = JOB.with_borrow(|j| j.as_ref().and_then(|j| j.handle.as_ref()).is_some_and(|h| h.is_finished()));
    if !ended {
        return false;
    }
    let Some(mut job) = JOB.with_borrow_mut(Option::take) else { return false };
    let downloading = job.progress.is_some();
    let result = job.handle.take().map(|h| h.join());
    if downloading {
        match result {
            Some(Ok(Ok(Done::Replaced))) if job.auto => {}
            Some(Ok(Ok(Done::Replaced))) => notice(vec![trf!("The game is updated: «{name}». The saves stay.", name = job.name.clone())]),
            Some(Ok(Ok(Done::Patched(n)))) => notice(vec![trf!("The game files are restored: {n} files.", n = n)]),
            Some(Ok(Err(Error::Cancelled))) => {}
            Some(Ok(Err(e))) => DOWNLOAD_FAILURE.with_borrow_mut(|f| *f = Some(error_text(&e))),
            _ => DOWNLOAD_FAILURE.with_borrow_mut(|f| *f = Some(tr("The download stopped unexpectedly.").to_string())),
        }
        return true;
    }
    let text = match result {
        Some(Ok(Ok(Done::Replaced))) => trf!("The game is updated: «{name}». The saves stay.", name = job.name.clone()),
        Some(Ok(Ok(Done::Patched(n)))) => trf!("Written over the game: {n} files.", n = n),
        Some(Ok(Err(e))) => error_text(&e),
        _ => tr("The update stopped unexpectedly.").to_string(),
    };
    notice(vec![text]);
    true
}

/// A notice of this module (its title is [`title`]).
fn notice(lines: Vec<String>) {
    MODAL.with_borrow_mut(|m| *m = Some(Modal::Notice { title: title().to_string(), lines }));
}

/// The words for a failed import, as the player reads them.
fn error_text(e: &Error) -> String {
    match e {
        Error::NotGame(name) => trf!("«{name}» holds neither the game nor a patch for it.", name = name.clone()),
        Error::Unreadable(why) => trf!("The archive cannot be read: {why}.", why = why.clone()),
        Error::TooBig(name) => trf!("«{name}» is too big for the game.", name = name.clone()),
        Error::NoInstall => tr("A patch goes over the game, and there is no game yet: import the whole game archive first.").to_string(),
        Error::NotInstall => tr("The archive does not hold a Discord Times install: the game is not changed.").to_string(),
        Error::Unpack(UnpackError::NoRoomOrAccess(dir)) => trf!("Could not unpack: no room or no access to the folder {dir}", dir = dir.display()),
        Error::Unpack(other) => trf!("Could not unpack: {error}", error = other.to_string()),
        Error::Io(why) => trf!("The update failed: {why}.", why = why.clone()),
        Error::Download(why) => trf!("Download failed: {why}.", why = why.clone()),
        Error::Cancelled => tr("The download was cancelled.").to_string(),
    }
}

/// The window of this module, drawn over the settings (after them, so it is on top).
pub fn modal() {
    let modal = MODAL.with_borrow_mut(Option::take);
    match modal {
        Some(Modal::Confirm(pending)) => match confirm_window(&pending.found.name) {
            None => MODAL.with_borrow_mut(|m| *m = Some(Modal::Confirm(pending))),
            Some(true) => start(pending),
            Some(false) => {
                if let Some(copy) = &pending.cleanup {
                    let _ = std::fs::remove_file(copy);
                }
            }
        },
        Some(Modal::Notice { title, lines }) => {
            if !map_import::notice_window(&title, &lines) {
                MODAL.with_borrow_mut(|m| *m = Some(Modal::Notice { title, lines }));
            }
        }
        None => progress_window(),
    }
}

/// The question before a whole game is replaced: Replace (`Some(true)`), Cancel (`Some(false)`),
/// or no answer yet.
fn confirm_window(name: &str) -> Option<bool> {
    let text = trf!("The game will be replaced with «{name}»: the game files now in the folder are removed. Your saves stay. Replace?", name = name.to_string());
    let (inner, closed) = map_import::frame(title(), &[text], true);
    match map_import::buttons(inner, &[tr("Replace"), tr("Cancel")]) {
        Some(0) => Some(true),
        Some(_) => Some(false),
        None if closed || esc_pressed() => Some(false),
        None => None,
    }
}

/// The import's progress while its thread runs: the download's bar (with Cancel) while the file
/// comes, then the unpacking's percent and bar. The snapshot's `RAZDOR_SCENE_DOWNLOADING` shows
/// the download's bar without a job.
fn progress_window() {
    let Some((done, total, progress, title, size)) = JOB.with_borrow(|j| j.as_ref().map(|j| (j.done.load(Ordering::Relaxed), j.total.load(Ordering::Relaxed), j.progress.clone(), j.title, j.size))) else {
        if checking() {
            checking_window();
        } else if let Some(bytes) = snapshot_download_bytes() {
            download_window(bytes, gdrive::GAME.size, None, tr("Loading the game"));
        } else if let Some(pct) = snapshot_unpacking() {
            unpack_window(pct, 100, tr("Loading the game"));
        }
        return;
    };
    match progress {
        Some(p) if !p.downloaded() => download_window(p.bytes(), size, Some(p.as_ref()), title),
        _ => unpack_window(done, total, title),
    }
}

/// The snapshot's fake download: `RAZDOR_SCENE_DOWNLOADING=<percent>` (with `RAZDOR_SNAPSHOT`).
fn snapshot_download_bytes() -> Option<u64> {
    std::env::var_os("RAZDOR_SNAPSHOT")?;
    let pct: u64 = std::env::var("RAZDOR_SCENE_DOWNLOADING").ok()?.parse().ok()?;
    Some(gdrive::GAME.size * pct.min(100) / 100)
}

/// The snapshot's fake unpacking: `RAZDOR_SCENE_UNPACKING=<percent>` (with `RAZDOR_SNAPSHOT`).
fn snapshot_unpacking() -> Option<u64> {
    std::env::var_os("RAZDOR_SNAPSHOT")?;
    let pct: u64 = std::env::var("RAZDOR_SCENE_UNPACKING").ok()?.parse().ok()?;
    Some(pct.min(100))
}

/// The unpacking's percent and bar.
fn unpack_window(done: u64, total: u64, title: &str) {
    let pct = if total == 0 { 0 } else { (done * 100 / total).min(100) };
    let share = pct as f32 / 100.0;
    let (_, _, bar) = map_import::frame_bar(title, &[trf!("Unpacking… {pct}%", pct = pct)], Some(share), false);
    if let Some(bar) = bar {
        map_import::draw_bar(bar, share);
    }
}

/// The settings window's row (`main_menu::options_window`), `r` its full width: the button, or
/// the reason it is not offered, in its place.
pub fn settings_row(r: Rect, over_map: bool) {
    let k = chrome::k();
    let reason = if over_map {
        Some(tr("The game is updated from the main menu."))
    } else if cfg!(target_os = "ios") {
        Some(tr("The update from an archive is not on iPhone yet."))
    } else if !offered() {
        Some(tr("Only the game in Razdor's own folder can be updated here."))
    } else {
        None
    };
    if let Some(text) = reason {
        chrome::shadow_text(text, r.x + 4.0 * k, r.y + 19.0 * k, 13.0 * k, Color::new(0.93, 0.87, 0.75, 0.75));
        return;
    }
    let enabled = !busy();
    let (_, down, pressed) = button_state(r, enabled);
    chrome::marble_button(r, tr("Update the game from an archive…"), enabled, down);
    if pressed {
        cue(Cue::Button);
        request();
    }
}

/// The words of the picker's failure (Android) or of the cancel, for the picker's answer.
enum Poll {
    Nothing,
    Cancelled,
    /// The Android picker reported an error (its reason).
    #[cfg(target_os = "android")]
    Failed(String),
    Got(Picked),
}

/// Opens the platform's picker (the state is then `Desktop` or `Android`).
#[cfg(target_os = "ios")]
fn start_picker() {}

#[cfg(target_os = "android")]
fn start_picker() {
    let state = if super::android::start_pick() {
        Picker::Android
    } else {
        notice(vec![tr("The file picker cannot be opened on this phone.").to_string()]);
        Picker::Idle
    };
    PICKER.with_borrow_mut(|p| *p = state);
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn start_picker() {
    let (tx, rx) = std::sync::mpsc::channel();
    if cfg!(target_os = "macos") {
        // macOS: the dialog runs here, on the main thread, as its dialogs must.
        let _ = tx.send(desktop_dialog());
        PICKER.with_borrow_mut(|p| *p = Picker::Desktop(rx));
    } else {
        let spawned = std::thread::Builder::new().name("game-picker".into()).spawn(move || {
            let _ = tx.send(desktop_dialog());
        });
        match spawned {
            Ok(_) => PICKER.with_borrow_mut(|p| *p = Picker::Desktop(rx)),
            Err(e) => notice(vec![trf!("The file picker cannot be opened: {e}.", e = e)]),
        }
    }
}

/// The desktop's open-file dialog: a game archive (.zip or .7z), or any file.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn desktop_dialog() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr("Update the game from an archive…"))
        .add_filter(tr("Game archives"), &["zip", "7z"])
        .add_filter(tr("All files"), &["*"])
        .pick_file()
}

/// The picker's answer this frame, if it has one.
fn poll_picker() -> Poll {
    let state = PICKER.with_borrow_mut(std::mem::take);
    match state {
        Picker::Idle => Poll::Nothing,
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        Picker::Desktop(rx) => match rx.try_recv() {
            Ok(None) => Poll::Cancelled,
            Ok(Some(path)) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                Poll::Got(Picked { name, path, cached: false })
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                PICKER.with_borrow_mut(|p| *p = Picker::Desktop(rx));
                Poll::Nothing
            }
            // The dialog's thread ended without an answer: taken as cancelled.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Poll::Cancelled,
        },
        #[cfg(target_os = "android")]
        Picker::Android => {
            let outcome = super::android::pick_outcome();
            match outcome.as_str() {
                "" => {
                    PICKER.with_borrow_mut(|p| *p = Picker::Android);
                    Poll::Nothing
                }
                "cancel" => Poll::Cancelled,
                text if text.starts_with("error: ") => Poll::Failed(text["error: ".len()..].to_string()),
                text => match text.strip_prefix("ok\n").and_then(|rest| rest.split_once('\n')) {
                    Some((name, path)) => Poll::Got(Picked { name: name.to_string(), path: PathBuf::from(path), cached: true }),
                    None => Poll::Failed(trf!("The picked file cannot be read: {why}.", why = text.to_string())),
                },
            }
        }
    }
}

/// The automatic start's check, in the same window as the download and the unpacking («Loading the
/// game»): the line «Checking the catalog…» over an empty bar, until the game's download starts.
fn checking_window() {
    let (_, _, bar) = map_import::frame_bar(tr("Loading the game"), &[tr("Checking the catalog…").to_string()], Some(0.0), true);
    if let Some(bar) = bar {
        map_import::draw_bar(bar, 0.0);
    }
}

/// The game download's window: the megabytes of the archive, its percent and a bar, and Cancel (the
/// close box too). A cancel keeps the partial copy for the next press; `cancel` is None in the snapshot.
fn download_window(bytes: u64, all: u64, cancel: Option<&gdrive::Progress>, title: &str) {
    let mb = |b: u64| format!("{:.1}", b as f64 / 1_048_576.0);
    let pct = if all == 0 { 0 } else { (bytes * 100 / all).min(100) };
    let text = trf!("Downloading the game: {mb} / {all} MB, {pct}%.", mb = mb(bytes), all = mb(all), pct = pct);
    let share = if all == 0 { 0.0 } else { (bytes as f32 / all as f32).min(1.0) };
    // The button row is always reserved (the Cancel is always drawn, the snapshot's too), so the bar
    // never runs under it.
    let (inner, closed, bar) = map_import::frame_bar(title, &[text], Some(share), true);
    if let Some(bar) = bar {
        map_import::draw_bar(bar, share);
    }
    let answer = map_import::buttons(inner, &[tr("Cancel")]);
    if let Some(progress) = cancel {
        if answer.is_some() || closed {
            progress.cancel();
        }
    }
}
