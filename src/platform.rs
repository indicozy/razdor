//! Where Razdor keeps its own files and looks for the install, per platform.
//!
//! On desktop these are the platform's folders (`dirs`). Android has no such folders for
//! a program that is not a game (`dirs` gives nothing useful there), so Razdor uses its app's own
//! folder (`getExternalFilesDir`, see [`android_paths`]). Older releases kept the data on the
//! shared storage (`/storage/emulated/0`, which needed the all-files permission): until the copy
//! into the app's folder, those legacy paths stay in use (see [`set_android_files_dir`]).
//!
//! iOS keeps everything in the app's own Documents folder (`$HOME/Documents`, the app's
//! sandbox home). The Files app shows that folder as «На iPhone → Раздор» because Info.plist
//! turns file sharing on (`UIFileSharingEnabled`), so the player copies the game there with no
//! permission to ask for (see `ui::mobile`).

use std::path::{Path, PathBuf};

/// Whether the build is for a phone or a tablet (Android or iOS): no browser opener, no
/// self-update, a touch screen, the not-found screen with its buttons, the on-screen keyboard.
pub const MOBILE: bool = cfg!(any(target_os = "android", target_os = "ios"));

/// The margins as f32 bits: left, top, right, bottom (see [`safe_insets`]).
static INSETS: [std::sync::atomic::AtomicU32; 4] = [const { std::sync::atomic::AtomicU32::new(0) }; 4];

/// The screen's unsafe margins, in screen pixels (left, top, right, bottom): the rounded corners,
/// the display cutout and the system bars, which the interface keeps its text and its
/// indicators away from. Zeros on desktop. The UI measures them and stores them with
/// [`set_safe_insets`] (on a phone: at start, on a turn, and now and then).
pub fn safe_insets() -> (f32, f32, f32, f32) {
    use std::sync::atomic::Ordering::Relaxed;
    let v = |i: usize| f32::from_bits(INSETS[i].load(Relaxed));
    (v(0), v(1), v(2), v(3))
}

/// The safe margins from the system's insets: each side is at least `corner` (the rounded
/// corners' allowance, which not every system reports).
pub fn safe_margins(system: (f32, f32, f32, f32), corner: f32) -> (f32, f32, f32, f32) {
    (system.0.max(corner), system.1.max(corner), system.2.max(corner), system.3.max(corner))
}

/// The iOS safe area in screen pixels for a screen of the given orientation: 44 points at the
/// top and bottom in portrait (the notch and the home indicator), at the sides in landscape.
/// `dpi` is the screen's pixels per point.
pub fn ios_safe_margins(portrait: bool, dpi: f32) -> (f32, f32, f32, f32) {
    let m = (44.0 * dpi).round();
    if portrait { (0.0, m, 0.0, m) } else { (m, 0.0, m, 0.0) }
}

/// Stores the margins [`safe_insets`] returns.
pub fn set_safe_insets(insets: (f32, f32, f32, f32)) {
    use std::sync::atomic::Ordering::Relaxed;
    let (l, t, r, b) = insets;
    for (slot, value) in INSETS.iter().zip([l, t, r, b]) {
        slot.store(value.to_bits(), Relaxed);
    }
}

/// Android: Razdor's folder on the shared internal storage, the one older releases used. It stays
/// the source of the copy into the app's own folder and the place of the data until the move
/// (see [`android_paths`]).
pub const LEGACY_ANDROID_ROOT: &str = "/storage/emulated/0/Razdor";

/// Android: the game's folder on the shared internal storage (`dt::install`), the legacy one
/// as [`LEGACY_ANDROID_ROOT`].
pub const LEGACY_ANDROID_GAME: &str = "/storage/emulated/0/DiscordTimes";

/// Where Android keeps Razdor's folders: Razdor's own folder ([`AndroidPaths::root`]) and the game's
/// ([`AndroidPaths::game`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidPaths {
    pub root: PathBuf,
    pub game: PathBuf,
}

/// The paths of the data once it is in the app's own folder `files` (`getExternalFilesDir`, see
/// `ui::android::files_dir`): `<files>/Razdor` and `<files>/DiscordTimes`. Before the move
/// (`migrated` false) the legacy shared folders.
pub fn android_paths(files: &Path, migrated: bool) -> AndroidPaths {
    if migrated {
        AndroidPaths { root: files.join("Razdor"), game: files.join("DiscordTimes") }
    } else {
        legacy_android_paths()
    }
}

/// The legacy shared folders ([`LEGACY_ANDROID_ROOT`], [`LEGACY_ANDROID_GAME`]).
pub fn legacy_android_paths() -> AndroidPaths {
    AndroidPaths { root: PathBuf::from(LEGACY_ANDROID_ROOT), game: PathBuf::from(LEGACY_ANDROID_GAME) }
}

/// The marker that says the data is in the app's own folder: `<files>/Razdor/migrated-from-shared`.
pub fn migrated_marker(files: &Path) -> PathBuf {
    files.join("Razdor").join("migrated-from-shared")
}

/// Writes the marker ([`migrated_marker`]): a line with the time (seconds since the epoch) and
/// `how` (`fresh` for an install with no legacy data, else how the data came). Errors are ignored:
/// the next start asks again.
pub fn write_migrated_marker(files: &Path, how: &str) {
    let marker = migrated_marker(files);
    if let Some(dir) = marker.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let _ = std::fs::write(marker, format!("{secs} {how}\n"));
}

/// The shared storage's root (the legacy install search reads it, see [`install_roots`]).
pub const LEGACY_ANDROID_STORAGE: &str = "/storage/emulated/0";

/// What a legacy folder holds, for the start-up decision ([`start_state`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyState {
    /// Not there.
    Missing,
    /// There, with nothing in it but `.nomedia` (see [`write_nomedia`]).
    Empty,
    /// There, but it cannot be read (no permission, or another read error).
    Unreadable,
    /// There is something in it besides `.nomedia`.
    Data,
}

/// The start-up decision about the data (see [`start_state`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartState {
    /// The marker is there: the data is in the app's own folder.
    Migrated,
    /// No legacy data in either folder: a fresh install. The data is in the app's own folder, and
    /// the marker is written.
    Fresh,
    /// The legacy folders cannot be read: the app's own folder for this run (without the grant
    /// nothing can be written to the legacy root), and no marker, so the next start asks again.
    Unreadable,
    /// Legacy data is there and not copied yet: the legacy paths stay.
    Legacy,
}

impl StartState {
    /// Whether the app's own folder is in use for this run.
    pub fn migrated(self) -> bool {
        !matches!(self, StartState::Legacy)
    }
}

/// What `dir` holds (see [`LegacyState`]). A missing folder is [`LegacyState::Missing`], any other
/// read error is [`LegacyState::Unreadable`].
pub fn legacy_state(dir: &Path) -> LegacyState {
    match std::fs::read_dir(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LegacyState::Missing,
        Err(_) => LegacyState::Unreadable,
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry) if entry.file_name() == ".nomedia" => {}
                    Ok(_) => return LegacyState::Data,
                    Err(_) => return LegacyState::Unreadable,
                }
            }
            LegacyState::Empty
        }
    }
}

/// The start-up decision: the marker is there ([`StartState::Migrated`]); else any legacy folder
/// holds data ([`StartState::Legacy`], the data is copied later); else a legacy folder cannot be
/// read, or the shared storage root `storage` (see [`LEGACY_ANDROID_STORAGE`]) cannot be read to
/// tell whether there is legacy data at all ([`StartState::Unreadable`]); else nothing is there
/// ([`StartState::Fresh`]). Only a readable root with both folders missing or empty is a fresh install:
/// the marker is written then, so a later start never looks at the shared folders again.
pub fn start_state(marker: bool, storage: LegacyState, legacy_root: LegacyState, legacy_game: LegacyState) -> StartState {
    use LegacyState::{Data, Missing, Unreadable};
    if marker {
        StartState::Migrated
    } else if legacy_root == Data || legacy_game == Data {
        StartState::Legacy
    } else if storage == Unreadable || storage == Missing || legacy_root == Unreadable || legacy_game == Unreadable {
        StartState::Unreadable
    } else {
        StartState::Fresh
    }
}

/// The install folders on Android (see [`install_roots`]). Not migrated: the legacy search list.
/// Migrated: the app's game folder `game` first, then the legacy search, which only runs while the
/// shared storage `storage` is readable (`storage_readable`), so a game in `Download` or `Games`
/// is still found in the release that moves the data.
pub fn android_install_roots(migrated: bool, storage: &Path, storage_readable: bool, game: PathBuf) -> Vec<PathBuf> {
    let legacy: Vec<PathBuf> = ["DiscordTimes", "Download", "Games", ""].iter().map(|d| storage.join(d)).collect();
    if !migrated {
        return legacy;
    }
    let mut roots = vec![game];
    if storage_readable {
        for root in legacy {
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    roots
}

/// Android: the app's own folder, given by the binary at start ([`set_android_files_dir`]); `None`
/// when the call failed.
#[cfg(target_os = "android")]
static ANDROID_FILES: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

/// Android: whether the data is in the app's own folder (the release N gate). Off until
/// [`set_android_files_dir`] finds it migrated, or [`set_android_migrated`] after the copy.
#[cfg(target_os = "android")]
static ANDROID_MIGRATED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Android: stores the app's own folder (`getExternalFilesDir(null)`, or `getFilesDir()`) and
/// decides whether the data is in it ([`start_state`]). A fresh install gets its marker here.
/// Called once, before anything reads a folder (the log's folder included); a second call does
/// nothing. `None` (the call failed) keeps the legacy folders.
#[cfg(target_os = "android")]
pub fn set_android_files_dir(files: Option<PathBuf>) {
    use std::sync::atomic::Ordering::Relaxed;
    if ANDROID_FILES.get().is_some() {
        return;
    }
    let start = match &files {
        Some(files) => {
            let state = start_state(
                migrated_marker(files).exists(),
                legacy_state(Path::new(LEGACY_ANDROID_STORAGE)),
                legacy_state(Path::new(LEGACY_ANDROID_ROOT)),
                legacy_state(Path::new(LEGACY_ANDROID_GAME)),
            );
            if state == StartState::Fresh {
                write_migrated_marker(files, "fresh");
            }
            state
        }
        None => {
            crate::diag!("the app's folder could not be read: the data stays in the shared storage");
            StartState::Legacy
        }
    };
    let _ = ANDROID_FILES.set(files);
    ANDROID_MIGRATED.store(start.migrated(), Relaxed);
}

/// Android: the app's own folder as [`set_android_files_dir`] found it; `None` when the call failed.
#[cfg(target_os = "android")]
pub fn android_files_dir() -> Option<&'static Path> {
    ANDROID_FILES.get().and_then(|files| files.as_deref())
}

/// Android: whether the data is in the app's own folder (see [`set_android_files_dir`]).
#[cfg(target_os = "android")]
pub fn android_migrated() -> bool {
    ANDROID_MIGRATED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Android: records that the data was copied into the app's own folder; from now on the paths are
/// the app's own ones. The caller writes the marker first ([`write_migrated_marker`] with
/// [`android_files_dir`]), then calls this.
#[cfg(target_os = "android")]
pub fn set_android_migrated() {
    ANDROID_MIGRATED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Android: the paths as they are now: the app's own folder once migrated, else the legacy ones.
#[cfg(target_os = "android")]
fn android_current() -> AndroidPaths {
    match android_files_dir() {
        Some(files) => android_paths(files, android_migrated()),
        None => legacy_android_paths(),
    }
}

/// iOS: the app's Documents folder (the sandbox home is `$HOME`).
#[cfg(target_os = "ios")]
fn ios_documents() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Documents"))
}

/// Razdor's own folder: saves, the log, the editor's maps, the translation
/// jobs. Desktop: `razdor` in the platform's data folder. Android: [`android_paths`] (the app's
/// own folder, or the legacy one until the move). iOS: `Documents/Razdor`.
pub fn app_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        Some(android_current().root)
    }
    #[cfg(target_os = "ios")]
    {
        ios_documents().map(|docs| docs.join("Razdor"))
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        dirs::data_dir().map(|d| d.join("razdor"))
    }
}

/// iOS: the app's Caches folder (`$HOME/Library/Caches/Razdor`). The Files app does not show it.
#[cfg(target_os = "ios")]
fn ios_caches() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library").join("Caches").join("Razdor"))
}

/// Android: the app's private cache folder, given by the binary at start (see
/// [`set_private_cache_dir`]).
#[cfg(target_os = "android")]
static PRIVATE_CACHE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

/// Android: stores the app's private cache folder (`Context.getCacheDir()`, read by the binary
/// through `ui::android::private_cache_dir`: the JNI calls live there, not in this library).
/// Called once at start; `None` (the call failed) turns the picture cache off.
#[cfg(target_os = "android")]
pub fn set_private_cache_dir(dir: Option<PathBuf>) {
    let _ = PRIVATE_CACHE.set(dir);
}

/// Razdor's cache folder: the decoded pictures, which the player never needs by hand. Android:
/// the app's private cache folder ([`set_private_cache_dir`]), which the Files app's Recent tab
/// and the media scanner do not see. iOS: `Library/Caches/Razdor` (not `Documents`, which the
/// Files app shows). Desktop: `cache` in [`app_dir`]. `None` where there is no such folder.
pub fn cache_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        PRIVATE_CACHE.get().cloned().flatten()
    }
    #[cfg(target_os = "ios")]
    {
        ios_caches()
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        app_dir().map(|d| d.join("cache"))
    }
}

/// Removes the picture cache that older releases kept under `root` (`cache/pictures`, where
/// `root` is [`app_dir`]; on Android that was the shared storage the Files app indexes), then
/// `cache` itself if it is empty by now. Only that folder is touched: the saves, the log and the
/// rest of `root` stay. Errors are ignored (no permission, nothing there).
pub fn retire_old_cache(root: &Path) {
    let cache = root.join("cache");
    let _ = std::fs::remove_dir_all(cache.join("pictures"));
    // Fails, and leaves it, unless it is empty.
    let _ = std::fs::remove_dir(&cache);
}

/// Writes an empty `.nomedia` in `root` (if there is none), so the media scanner skips the
/// folder: the saves and the log are not offered as media. Errors are ignored.
pub fn write_nomedia(root: &Path) {
    let marker = root.join(".nomedia");
    if !marker.exists() && std::fs::create_dir_all(root).is_ok() {
        let _ = std::fs::write(marker, b"");
    }
}

/// Start-up housekeeping on the phones (`main` runs it on a thread): the old picture cache is
/// removed from [`app_dir`], and on Android `.nomedia` is written there. Desktop: nothing.
/// Android before the move ([`android_migrated`] false): nothing, since [`app_dir`] is then the
/// legacy shared folder, which is only read until its files are copied (`migration`).
pub fn tidy_app_dir() {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        #[cfg(target_os = "android")]
        if !android_migrated() {
            return;
        }
        if let Some(root) = app_dir() {
            retire_old_cache(&root);
            #[cfg(target_os = "android")]
            write_nomedia(&root);
        }
    }
}

/// The folder of the settings Razdor remembers (the install's folder): `razdor` in the
/// platform's config folder on desktop; on Android, the same as [`app_dir`]; on iOS, the same
/// `Documents/Razdor` as [`app_dir`].
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        Some(android_current().root)
    }
    #[cfg(target_os = "ios")]
    {
        app_dir()
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        dirs::config_dir().map(|d| d.join("razdor"))
    }
}

/// Folders searched for an install when none is set or remembered. Desktop: `~/Games`,
/// `~/Downloads` and the home folder. Android: the game folder ([`unpack_dir`]) first once the data
/// is in the app's own folder, then the usual legacy search (`DiscordTimes`, the download and game
/// folders, then the whole internal storage three levels deep, see `dt::install`; the legacy part
/// only while the storage is readable, see [`android_install_roots`]). iOS: the folder the
/// player is told to use (`Documents/DiscordTimes`), then `Documents` itself.
pub fn install_roots() -> Vec<PathBuf> {
    #[cfg(target_os = "android")]
    {
        let storage = Path::new(LEGACY_ANDROID_STORAGE);
        let readable = std::fs::read_dir(storage).is_ok();
        android_install_roots(android_migrated(), storage, readable, android_current().game)
    }
    #[cfg(target_os = "ios")]
    {
        let Some(docs) = ios_documents() else { return Vec::new() };
        vec![docs.join("DiscordTimes"), docs]
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let mut roots: Vec<PathBuf> = dirs::home_dir().map(|home| vec![home.join("Games"), home.join("Downloads"), home]).unwrap_or_default();
        roots.extend(unpack_dir());
        roots
    }
}

/// The folder an unpacked game goes into (`dt::unpack`), and the last install folder the search
/// checks: Android `DiscordTimes` in the app's own folder ([`android_paths`]), iOS `Documents/DiscordTimes`, macOS
/// `DiscordTimes` in [`app_dir`] (the app bundle is not written to), elsewhere `DiscordTimes`
/// next to the program.
pub fn unpack_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        Some(android_current().game)
    }
    #[cfg(target_os = "ios")]
    {
        ios_documents().map(|docs| docs.join("DiscordTimes"))
    }
    #[cfg(target_os = "macos")]
    {
        app_dir().map(|dir| dir.join("DiscordTimes"))
    }
    #[cfg(not(any(target_os = "android", target_os = "ios", target_os = "macos")))]
    {
        std::env::current_exe().ok()?.parent().map(|dir| dir.join("DiscordTimes"))
    }
}

/// The folders scanned for a game archive (`dt::unpack::find_archives`), without repeats. The
/// phones: their install folders ([`install_roots`]). The desktop: the program's own folder
/// (not on macOS, where it is inside the bundle), the Downloads folder, then the install folders.
pub fn archive_dirs() -> Vec<PathBuf> {
    let mut dirs = desktop_archive_dirs();
    dirs.extend(install_roots());
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn desktop_archive_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(not(target_os = "macos"))]
    dirs.extend(std::env::current_exe().ok().and_then(|exe| exe.parent().map(PathBuf::from)));
    dirs.extend(downloads_dir());
    dirs
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn desktop_archive_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// The Downloads folder: `XDG_DOWNLOAD_DIR` on Linux when set, else the platform's
/// ([`dirs::download_dir`]), else `Downloads` in the home folder.
pub fn downloads_dir() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        if let Some(dir) = std::env::var_os("XDG_DOWNLOAD_DIR").filter(|d| !d.is_empty()) {
            return Some(PathBuf::from(dir));
        }
    }
    dirs::download_dir().or_else(|| dirs::home_dir().map(|home| home.join("Downloads")))
}

/// iOS: the `shareddocuments://` link that opens the Files app at the absolute path `dir`
/// (percent-encoded). Razdor's own (a player's request), not the original's.
pub fn ios_files_url(dir: &str) -> String {
    let mut out = String::from("shareddocuments://");
    for b in dir.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_paths_are_legacy_until_migrated() {
        let files = Path::new("/data/data/io.github.indicozy.razdor/files");
        assert_eq!(android_paths(files, false), legacy_android_paths());
        assert_eq!(legacy_android_paths().root, PathBuf::from("/storage/emulated/0/Razdor"));
        assert_eq!(legacy_android_paths().game, PathBuf::from("/storage/emulated/0/DiscordTimes"));
        assert_eq!(
            android_paths(files, true),
            AndroidPaths { root: files.join("Razdor"), game: files.join("DiscordTimes") }
        );
    }

    /// A scratch folder for one test (removed by the test that makes it).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_marker_wins_over_any_legacy_state() {
        use LegacyState::*;
        for storage in [Missing, Empty, Unreadable, Data] {
            for root in [Missing, Empty, Unreadable, Data] {
                for game in [Missing, Empty, Unreadable, Data] {
                    assert_eq!(start_state(true, storage, root, game), StartState::Migrated, "{storage:?} {root:?} {game:?}");
                }
            }
        }
    }

    #[test]
    fn legacy_data_without_a_marker_stays_legacy() {
        use LegacyState::*;
        for other in [Missing, Empty, Unreadable, Data] {
            assert_eq!(start_state(false, Data, Data, other), StartState::Legacy, "root data, game {other:?}");
            assert_eq!(start_state(false, Data, other, Data), StartState::Legacy, "game data, root {other:?}");
            assert_eq!(start_state(false, Unreadable, Data, other), StartState::Legacy, "root data, storage unreadable, game {other:?}");
        }
    }

    #[test]
    fn unreadable_legacy_is_migrated_for_this_run_only() {
        use LegacyState::*;
        for other in [Missing, Empty, Unreadable] {
            assert_eq!(start_state(false, Data, Unreadable, other), StartState::Unreadable);
            assert_eq!(start_state(false, Data, other, Unreadable), StartState::Unreadable);
        }
        assert!(StartState::Unreadable.migrated());
        assert!(!StartState::Legacy.migrated());
    }

    #[test]
    fn no_legacy_data_is_a_fresh_install_only_when_the_storage_root_is_readable() {
        use LegacyState::*;
        for storage in [Data, Empty] {
            assert_eq!(start_state(false, storage, Missing, Missing), StartState::Fresh, "storage {storage:?}");
            assert_eq!(start_state(false, storage, Empty, Missing), StartState::Fresh, "storage {storage:?}");
            assert_eq!(start_state(false, storage, Missing, Empty), StartState::Fresh, "storage {storage:?}");
            assert_eq!(start_state(false, storage, Empty, Empty), StartState::Fresh, "storage {storage:?}");
        }
    }

    #[test]
    fn an_unreadable_or_missing_storage_root_is_never_fresh() {
        use LegacyState::*;
        // Without the root, the legacy folders may still hold data: no marker, the move asks again.
        for storage in [Unreadable, Missing] {
            for (root, game) in [(Missing, Missing), (Empty, Empty), (Missing, Empty), (Empty, Missing)] {
                assert_eq!(start_state(false, storage, root, game), StartState::Unreadable, "storage {storage:?}, root {root:?}, game {game:?}");
            }
        }
        assert_eq!(start_state(true, Unreadable, Missing, Missing), StartState::Migrated, "the marker wins");
    }

    #[test]
    fn the_folders_are_read_as_missing_empty_or_data() {
        let base = scratch("legacy-state");
        assert_eq!(legacy_state(&base.join("missing")), LegacyState::Missing);
        std::fs::create_dir_all(base.join("empty")).unwrap();
        std::fs::write(base.join("empty").join(".nomedia"), b"").unwrap();
        assert_eq!(legacy_state(&base.join("empty")), LegacyState::Empty);
        std::fs::create_dir_all(base.join("saves")).unwrap();
        std::fs::write(base.join("saves").join("game.rzsave"), b"save").unwrap();
        assert_eq!(legacy_state(&base.join("saves")), LegacyState::Data);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_marker_says_how_it_came_to_be() {
        let files = scratch("marker-how");
        write_migrated_marker(&files, "fresh");
        let text = std::fs::read_to_string(migrated_marker(&files)).unwrap();
        let (secs, how) = text.trim_end().split_once(' ').unwrap();
        assert!(secs.parse::<u64>().is_ok(), "{text:?}");
        assert_eq!(how, "fresh");
        let _ = std::fs::remove_dir_all(&files);
    }

    #[test]
    fn install_roots_before_the_move_are_the_legacy_search() {
        let storage = Path::new(LEGACY_ANDROID_STORAGE);
        let game = PathBuf::from("/data/files/DiscordTimes");
        let legacy: Vec<PathBuf> = ["DiscordTimes", "Download", "Games", ""].iter().map(|d| storage.join(d)).collect();
        assert_eq!(android_install_roots(false, storage, true, game.clone()), legacy);
        assert_eq!(android_install_roots(false, storage, false, game), legacy);
    }

    #[test]
    fn install_roots_after_the_move_add_the_legacy_search_only_when_readable() {
        let storage = Path::new(LEGACY_ANDROID_STORAGE);
        let legacy: Vec<PathBuf> = ["DiscordTimes", "Download", "Games", ""].iter().map(|d| storage.join(d)).collect();
        // The app's game folder first; the legacy search follows, all of it.
        let game = PathBuf::from("/data/files/DiscordTimes");
        let mut expected = vec![game.clone()];
        expected.extend(legacy.clone());
        assert_eq!(android_install_roots(true, storage, true, game.clone()), expected);
        // A game already in the legacy DiscordTimes is listed once.
        let roots = android_install_roots(true, storage, true, storage.join("DiscordTimes"));
        assert_eq!(roots, vec![storage.join("DiscordTimes"), storage.join("Download"), storage.join("Games"), storage.join("")]);
        // The shared storage unreadable: the app's game folder only.
        assert_eq!(android_install_roots(true, storage, false, game.clone()), vec![game]);
    }

    #[test]
    fn ios_folder_is_a_percent_encoded_files_link() {
        assert_eq!(
            ios_files_url("/var/mobile/Containers/Documents/Razdor"),
            "shareddocuments:///var/mobile/Containers/Documents/Razdor"
        );
        assert_eq!(ios_files_url("/a b/ю"), "shareddocuments:///a%20b/%D1%8E");
    }

    #[test]
    fn safe_margins_keep_the_corner_allowance_on_every_side() {
        // A system that reports no insets: the corner allowance on each side.
        assert_eq!(safe_margins((0.0, 0.0, 0.0, 0.0), 48.0), (48.0, 48.0, 48.0, 48.0));
        // A notch and a navigation bar on the bottom: they win over the allowance.
        assert_eq!(safe_margins((0.0, 120.0, 0.0, 150.0), 48.0), (48.0, 120.0, 48.0, 150.0));
        // Desktop has no margins at all.
        assert_eq!(safe_margins((0.0, 0.0, 0.0, 0.0), 0.0), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn ios_margins_follow_the_orientation() {
        assert_eq!(ios_safe_margins(true, 3.0), (0.0, 132.0, 0.0, 132.0));
        assert_eq!(ios_safe_margins(false, 2.0), (88.0, 0.0, 88.0, 0.0));
    }

    #[test]
    fn desktop_cache_is_the_old_picture_cache_folder() {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        assert_eq!(cache_dir(), dirs::data_dir().map(|d| d.join("razdor").join("cache")));
    }

    #[test]
    fn retiring_the_old_cache_leaves_the_saves() {
        let root = std::env::temp_dir().join(format!("razdor-retire-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pictures = root.join("cache").join("pictures");
        std::fs::create_dir_all(&pictures).unwrap();
        std::fs::write(pictures.join("0123456789abcdef.rzpc"), b"picture").unwrap();
        std::fs::create_dir_all(root.join("saves")).unwrap();
        std::fs::write(root.join("saves").join("game.rzsave"), b"save").unwrap();
        std::fs::write(root.join("razdor.log"), b"log").unwrap();

        retire_old_cache(&root);

        assert!(!root.join("cache").exists(), "the emptied cache folder goes too");
        assert_eq!(std::fs::read(root.join("saves").join("game.rzsave")).unwrap(), b"save");
        assert_eq!(std::fs::read(root.join("razdor.log")).unwrap(), b"log");

        // Something else in `cache` stays, and so does the folder.
        std::fs::create_dir_all(&pictures).unwrap();
        std::fs::write(root.join("cache").join("other.bin"), b"x").unwrap();
        retire_old_cache(&root);
        assert!(!pictures.exists());
        assert!(root.join("cache").join("other.bin").exists());
        // A missing folder is nothing to do.
        retire_old_cache(&root.join("missing"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nomedia_is_written_once() {
        let root = std::env::temp_dir().join(format!("razdor-nomedia-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_nomedia(&root);
        assert!(root.join(".nomedia").exists());
        write_nomedia(&root);
        assert!(root.join(".nomedia").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stored_insets_read_back() {
        set_safe_insets((12.0, 34.0, 56.0, 78.0));
        assert_eq!(safe_insets(), (12.0, 34.0, 56.0, 78.0));
        set_safe_insets((0.0, 0.0, 0.0, 0.0));
    }
}
