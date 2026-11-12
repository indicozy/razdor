//! The first screen's art when no game is installed: the title with its logo, flames and tiles,
//! the loading window and its frames, and the bitmap fonts (`Graphics/Windows/…`, the files of
//! [`FILES`]). They are taken from the game's own files (`menu-art.zip`, made from Community
//! Update 1.2 by the release scripts: `scripts/fetch-menu-art.sh`), so the install's art and this
//! folder have the same paths, and [`super::chrome`] reads them from here exactly as from an
//! install. Used only while no install is loaded: `main` gives the folder to the window art then,
//! and an install (`announce_install`) replaces it.
//!
//! Where the folder is looked for, the first one holding the art wins: `RAZDOR_MENU_ART_DIR` when
//! set (dev and snapshots: nothing else is tried), else `menu-art/` next to the program, and on a
//! macOS app bundle `Resources/menu-art/`. On Android the files are in the APK's assets (the
//! `assets` of Cargo.toml's `[package.metadata.android]`); they are copied once into the app's
//! private cache folder, which then serves as the folder.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The environment variable naming the folder (dev and snapshots).
pub const ENV: &str = "RAZDOR_MENU_ART_DIR";
/// The folder's name, next to the program (and in the macOS bundle's `Resources`).
pub const DIR: &str = "menu-art";
/// The picture the folder must hold for it to count (the title's tile frame).
const PROBE: &str = "Graphics/Windows/Win-marble.lit";

/// The files of the folder, relative to it: the first screen's pictures and fonts, only. The
/// Android copy reads them (the desktop's folder is taken as it is).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub const FILES: &[&str] = &[
    "Graphics/Windows/Castle.lit",
    "Graphics/Windows/Title_RUS.ugs",
    "Graphics/Windows/TitleFlame_RUS.lit",
    "Graphics/Windows/MenuFlame.lit",
    "Graphics/Windows/SMText_RUS.lit",
    "Graphics/Windows/MB2.ugs",
    "Graphics/Windows/Win-marble.lit",
    "Graphics/Windows/button-icon-6.lit",
    "Graphics/Windows/button-icon-8.lit",
    "Graphics/Windows/Cursor-Normal.lit",
    "Graphics/Windows/SanSerif.lit",
    "Graphics/Windows/SanSerif_Shadow.lit",
    "Graphics/Windows/Benguiat.lit",
    "Graphics/Windows/Benguiat_Shadow.lit",
    "Graphics/Windows/Corner-Left.lit",
    "Graphics/Windows/Corner-Right.lit",
    "Graphics/Windows/SteelLine.lit",
    "Graphics/Windows/Btn3Up.lit",
    "Graphics/Windows/Btn3Down.lit",
    "Graphics/Windows/CloseButtonGreen-Up.lit",
];

/// The folders to look in, in order (pure): the environment's alone when it names one, else the
/// program's `menu-art/`, then the bundle's `Resources/menu-art/` (macOS).
pub fn candidates(env: Option<&OsStr>, exe_dir: Option<&Path>, resources: Option<&Path>) -> Vec<PathBuf> {
    if let Some(dir) = env.filter(|d| !d.is_empty()) {
        return vec![PathBuf::from(dir)];
    }
    exe_dir.map(|d| d.join(DIR)).into_iter().chain(resources.map(|r| r.join(DIR))).collect()
}

/// Whether `dir` holds the art (its probe picture).
pub fn holds_art(dir: &Path) -> bool {
    dir.join(PROBE).is_file()
}

/// The first of `candidates` that holds the art.
pub fn first_holding(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|d| holds_art(d)).cloned()
}

/// The folder outside Android (the desktop's programs and a macOS bundle): see the module doc.
pub fn root() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    // macOS: `Razdor.app/Contents/MacOS/razdor` has its art in `Contents/Resources/`.
    let resources = exe_dir.as_deref().and_then(|d| d.parent()).map(|c| c.join("Resources"));
    let resources = resources.filter(|_| cfg!(target_os = "macos"));
    first_holding(&candidates(std::env::var_os(ENV).as_deref(), exe_dir.as_deref(), resources.as_deref()))
}

/// The folder for this run: [`root`] on the desktop; on Android the APK's files, copied into the
/// private cache folder at the first run (`platform::cache_dir`). None when there is no art.
pub async fn root_or_extract() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        extract_from_apk().await
    }
    #[cfg(not(target_os = "android"))]
    {
        root()
    }
}

/// The APK's copy of the folder, in the private cache (made when missing). A file that cannot be
/// read or written stops the copy (logged): the screen then keeps its plain look.
#[cfg(target_os = "android")]
async fn extract_from_apk() -> Option<PathBuf> {
    let dest = razdor::platform::cache_dir()?.join(DIR);
    if holds_art(&dest) {
        return Some(dest);
    }
    for rel in FILES {
        let bytes = match macroquad::file::load_file(rel).await {
            Ok(bytes) => bytes,
            Err(e) => {
                razdor::diag!("menu art: {rel} is not in the app: {e}");
                return None;
            }
        };
        let path = dest.join(rel);
        let written = path.parent().map(std::fs::create_dir_all).transpose().and_then(|_| std::fs::write(&path, &bytes));
        if let Err(e) = written {
            razdor::diag!("menu art: {} is not written: {e}", path.display());
            return None;
        }
    }
    Some(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system temp dir for one test.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-menu-art-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A folder holding the probe picture (`root/menu-art/…`).
    fn with_art(root: &Path, dir: &str) -> PathBuf {
        let folder = root.join(dir);
        std::fs::create_dir_all(folder.join("Graphics/Windows")).unwrap();
        std::fs::write(folder.join(PROBE), b"art").unwrap();
        folder
    }

    #[test]
    fn the_environment_names_the_only_folder_when_set() {
        let exe = Path::new("/opt/razdor");
        let res = Path::new("/opt/Razdor.app/Contents/Resources");
        assert_eq!(candidates(Some(OsStr::new("/dev/art")), Some(exe), Some(res)), vec![PathBuf::from("/dev/art")]);
        // An empty value is no setting.
        assert_eq!(candidates(Some(OsStr::new("")), Some(exe), None), vec![exe.join(DIR)]);
    }

    #[test]
    fn the_program_folder_comes_before_the_bundle_resources() {
        let exe = Path::new("/a/bin");
        let res = Path::new("/a/Resources");
        assert_eq!(candidates(None, Some(exe), Some(res)), vec![exe.join(DIR), res.join(DIR)]);
        assert_eq!(candidates(None, Some(exe), None), vec![exe.join(DIR)]);
        assert!(candidates(None, None, None).is_empty());
    }

    #[test]
    fn the_first_folder_holding_the_art_is_used() {
        let root = temp("first");
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let art = with_art(&root, "res");
        let later = with_art(&root, "later");
        let found = first_holding(&[empty.clone(), art.clone(), later]);
        assert_eq!(found, Some(art));
        assert_eq!(first_holding(&[empty]), None, "a folder without the art is no folder");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_listed_files_start_with_the_probe_and_are_unique() {
        assert!(FILES.contains(&PROBE));
        let mut sorted = FILES.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), FILES.len(), "no file is listed twice");
        assert!(FILES.iter().all(|f| f.starts_with("Graphics/Windows/")));
    }
}
