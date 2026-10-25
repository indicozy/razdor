//! Where Razdor keeps its own files and looks for the install, per platform.
//!
//! On desktop these are the platform's folders (`dirs`). Android has no such folders for
//! a program that is not a game (`dirs` gives nothing useful there), so Razdor uses a
//! `Razdor` folder on the shared storage: it survives an uninstall, and the player reaches
//! it with a file manager. Reading and writing it needs the all-files permission (see
//! `ui::android`).
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

/// Android: Razdor's folder on the shared internal storage.
#[cfg(target_os = "android")]
pub const ANDROID_ROOT: &str = "/storage/emulated/0/Razdor";

/// iOS: the app's Documents folder (the sandbox home is `$HOME`).
#[cfg(target_os = "ios")]
fn ios_documents() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Documents"))
}

/// Razdor's own folder: saves, the log, the editor's maps, the translation
/// jobs. Desktop: `razdor` in the platform's data folder. Android: [`ANDROID_ROOT`]. iOS:
/// `Documents/Razdor`.
pub fn app_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        Some(PathBuf::from(ANDROID_ROOT))
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
pub fn tidy_app_dir() {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        if let Some(root) = app_dir() {
            retire_old_cache(&root);
            #[cfg(target_os = "android")]
            write_nomedia(&root);
        }
    }
}

/// The folder of the settings Razdor remembers (the install's folder): `razdor` in the
/// platform's config folder on desktop; on Android, [`ANDROID_ROOT`] too; on iOS, the same
/// `Documents/Razdor` as [`app_dir`].
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        Some(PathBuf::from(ANDROID_ROOT))
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
/// `~/Downloads` and the home folder. Android: the folder the player is told to use
/// (`DiscordTimes`, in internal storage), then the usual download and game folders, then
/// the whole internal storage (three levels deep, see `dt::install`). iOS: the folder the
/// player is told to use (`Documents/DiscordTimes`), then `Documents` itself.
pub fn install_roots() -> Vec<PathBuf> {
    #[cfg(target_os = "android")]
    {
        ["DiscordTimes", "Download", "Games", ""].iter().map(|d| PathBuf::from("/storage/emulated/0").join(d)).collect()
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
/// checks: Android `DiscordTimes` in the shared storage, iOS `Documents/DiscordTimes`, macOS
/// `DiscordTimes` in [`app_dir`] (the app bundle is not written to), elsewhere `DiscordTimes`
/// next to the program.
pub fn unpack_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        Some(PathBuf::from("/storage/emulated/0/DiscordTimes"))
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

#[cfg(test)]
mod tests {
    use super::*;

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
