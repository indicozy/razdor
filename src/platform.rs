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

use std::path::PathBuf;

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

/// Razdor's own folder: saves, the log, the picture cache, the editor's maps, the translation
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
        let Some(home) = dirs::home_dir() else { return Vec::new() };
        vec![home.join("Games"), home.join("Downloads"), home]
    }
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
    fn stored_insets_read_back() {
        set_safe_insets((12.0, 34.0, 56.0, 78.0));
        assert_eq!(safe_insets(), (12.0, 34.0, 56.0, 78.0));
        set_safe_insets((0.0, 0.0, 0.0, 0.0));
    }
}
