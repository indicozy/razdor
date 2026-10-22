//! Where Razdor keeps its own files and looks for the install, per platform.
//!
//! On desktop these are the platform's folders (`dirs`). Android has no such folders for
//! a program that is not a game (`dirs` gives nothing useful there), so Razdor uses a
//! `Razdor` folder on the shared storage: it survives an uninstall, and the player reaches
//! it with a file manager. Reading and writing it needs the all-files permission (see
//! `ui::android`).

use std::path::PathBuf;

/// Android: Razdor's folder on the shared internal storage.
#[cfg(target_os = "android")]
pub const ANDROID_ROOT: &str = "/storage/emulated/0/Razdor";

/// Razdor's own folder: saves, the log, the picture cache, the editor's maps, the translation
/// jobs. Desktop: `razdor` in the platform's data folder. Android: [`ANDROID_ROOT`].
pub fn app_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        Some(PathBuf::from(ANDROID_ROOT))
    }
    #[cfg(not(target_os = "android"))]
    {
        dirs::data_dir().map(|d| d.join("razdor"))
    }
}

/// The folder of the settings Razdor remembers (the install's folder): `razdor` in the
/// platform's config folder on desktop; on Android, [`ANDROID_ROOT`] too.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        Some(PathBuf::from(ANDROID_ROOT))
    }
    #[cfg(not(target_os = "android"))]
    {
        dirs::config_dir().map(|d| d.join("razdor"))
    }
}

/// Folders searched for an install when none is set or remembered. Desktop: `~/Games`,
/// `~/Downloads` and the home folder. Android: the folder the player is told to use
/// (`DiscordTimes`, in internal storage), then the usual download and game folders, then
/// the whole internal storage (three levels deep, see `dt::install`).
pub fn install_roots() -> Vec<PathBuf> {
    #[cfg(target_os = "android")]
    {
        ["DiscordTimes", "Download", "Games", ""].iter().map(|d| PathBuf::from("/storage/emulated/0").join(d)).collect()
    }
    #[cfg(not(target_os = "android"))]
    {
        let Some(home) = dirs::home_dir() else { return Vec::new() };
        vec![home.join("Games"), home.join("Downloads"), home]
    }
}
