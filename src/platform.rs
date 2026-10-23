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
