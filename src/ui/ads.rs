//! Razdor's promo ads (the main menu's corner picture): a folder `ads/` with one subfolder per ad,
//! each holding an `ad.ini` (`url=`, `image=`, `caption_ru=`, `caption_en=`, one `key=value` per
//! line, `#` or `;` comments) and the picture it names. The folder is read at run time, not built
//! into the program, so the ad can change without a new release.
//!
//! Where it is looked for, the first hit wins: `RAZDOR_ADS_DIR` when set (dev and snapshots, and
//! then nothing else is tried), else `ads/` next to the program, `ads/` in the game install, and
//! `ads/` in Razdor's own folder (`platform::app_dir`). In a folder the subfolders are taken sorted
//! by name and the first one with a valid `ad.ini` and a picture that decodes is the ad. A missing
//! folder means no promo, and no error.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};

use razdor::hub::{self, AdRef, Hub, Source};

use super::launch;

use macroquad::prelude::*;
use razdor::i18n::{self, Lang};
use razdor::local;

/// The name of the ads folder, next to the program, in the install and in Razdor's folder.
const ADS_DIR: &str = "ads";
/// The ad's text file in its subfolder.
const AD_FILE: &str = "ad.ini";

/// What `ad.ini` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdText {
    pub url: String,
    /// The picture's file name, in the ad's folder.
    pub image: String,
    pub caption_ru: String,
    pub caption_en: String,
}

/// The ad as the menu draws it: its link, its captions and its picture, decoded once.
pub struct Promo {
    pub url: String,
    caption_ru: String,
    caption_en: String,
    pub texture: Texture2D,
}

impl Promo {
    /// The caption in the interface language: Russian for Russian, else English; the other one
    /// when the chosen one is empty.
    pub fn caption(&self) -> &str {
        let (first, second) = if i18n::lang() == Lang::Ru {
            (&self.caption_ru, &self.caption_en)
        } else {
            (&self.caption_en, &self.caption_ru)
        };
        if first.is_empty() { second } else { first }
    }
}

/// Parses an `ad.ini`. None when the link is not an http(s) address or the picture's name is
/// missing or names a folder. Unknown keys are ignored.
pub fn parse_ad(text: &str) -> Option<AdText> {
    let (mut url, mut image) = (None, None);
    let (mut caption_ru, mut caption_en) = (String::new(), String::new());
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim().to_string();
        match key.trim() {
            "url" => url = Some(value),
            "image" => image = Some(value),
            "caption_ru" => caption_ru = value,
            "caption_en" => caption_en = value,
            _ => {}
        }
    }
    let url = url.filter(|u| u.starts_with("https://") || u.starts_with("http://"))?;
    let image = image.filter(|i| !i.is_empty() && i != ".." && !i.contains(['/', '\\']))?;
    Some(AdText { url, image, caption_ru, caption_en })
}

/// The subfolders of `root`, sorted by name. Empty when `root` is not a folder.
pub fn ad_folders(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs
}

/// The first ad among `folders` (in their order) whose `ad.ini` parses and whose picture
/// `load_image` accepts (given the picture's path). The picture is the loader's result.
pub fn select_ad<T>(folders: &[PathBuf], load_image: impl Fn(&Path) -> Option<T>) -> Option<(AdText, T)> {
    folders.iter().find_map(|dir| {
        let text = parse_ad(&std::fs::read_to_string(dir.join(AD_FILE)).ok()?)?;
        let image = load_image(&dir.join(&text.image))?;
        Some((text, image))
    })
}

/// The places to look in, in order (pure): `RAZDOR_ADS_DIR` alone when it names one; else the copy of
/// the hub's ad in the app's folder (`app_dir/ads`, [`sync_tick`]), `ads/` next to the program, and the
/// install's `ads/`.
pub fn ad_roots(env: Option<&OsStr>, app_dir: Option<&Path>, exe_dir: Option<&Path>, install: Option<&Path>) -> Vec<PathBuf> {
    if let Some(dir) = env.filter(|d| !d.is_empty()) {
        return vec![PathBuf::from(dir)];
    }
    app_dir.map(|d| d.join(ADS_DIR)).into_iter().chain(exe_dir.map(|d| d.join(ADS_DIR))).chain(install.map(|d| d.join(ADS_DIR))).collect()
}

/// The places to look in, in order (see [`ad_roots`]).
fn roots() -> Vec<PathBuf> {
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    let install = razdor::dt::install::locate();
    ad_roots(std::env::var_os("RAZDOR_ADS_DIR").as_deref(), razdor::platform::app_dir().as_deref(), exe_dir.as_deref(), install.as_deref())
}

/// The first ad found in the places of `roots`, or None.
fn find_promo() -> Option<Promo> {
    roots().iter().find_map(|root| {
        let folders = ad_folders(root);
        let (text, image) = select_ad(&folders, |path| {
            let bytes = std::fs::read(path).ok()?;
            Image::from_file_with_format(&bytes, None).ok()
        })?;
        let texture = Texture2D::from_image(&image);
        texture.set_filter(FilterMode::Linear);
        Some(Promo { url: text.url, caption_ru: text.caption_ru, caption_en: text.caption_en, texture })
    })
}

/// The looked-up ad, kept until [`Slot::forget`]: a lookup that found nothing is kept too (the
/// menu draws nothing then), so a later install is only seen after the forget.
pub struct Slot<T> {
    value: Option<Option<T>>,
}

impl<T: Clone> Slot<T> {
    pub const fn new() -> Slot<T> {
        Slot { value: None }
    }

    /// The kept value, or the result of `load` when there is none yet.
    pub fn get_or_load(&mut self, load: impl FnOnce() -> Option<T>) -> Option<T> {
        self.value.get_or_insert_with(load).clone()
    }

    /// Drops the kept value: the next [`Slot::get_or_load`] looks again.
    pub fn forget(&mut self) {
        self.value = None;
    }
}

local! {
    static PROMO: std::cell::RefCell<Slot<Rc<Promo>>> = const { std::cell::RefCell::new(Slot::new()) };
}

/// The promo of the current install (or of the program's and Razdor's folders), or None when no ad
/// is found (the menu draws nothing then).
pub fn promo() -> Option<Rc<Promo>> {
    PROMO.with(|slot| slot.borrow_mut().get_or_load(|| find_promo().map(Rc::new)))
}

/// The install was (re)loaded: the ad is looked for again on the next draw. The first draw of a run
/// without an install keeps "none", so the game's own `ads/` (copied in by the import) is only seen
/// after this.
pub fn forget() {
    PROMO.with(|slot| slot.borrow_mut().forget());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system temp dir for one test.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-ads-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_ad(root: &Path, folder: &str, ini: &str, image: Option<&[u8]>) {
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(AD_FILE), ini).unwrap();
        if let Some(bytes) = image {
            std::fs::write(dir.join("capsule.png"), bytes).unwrap();
        }
    }

    const INI: &str = "url=https://example.org/app/1/\nimage=capsule.png\ncaption_ru=Новая игра\ncaption_en=A new game\n";

    #[test]
    fn an_ad_file_parses_its_keys_and_skips_the_rest() {
        let ad = parse_ad(&format!("\u{feff}# a comment\n; another\n\n{INI}extra=1\nnoequals\n")).unwrap();
        assert_eq!(ad.url, "https://example.org/app/1/");
        assert_eq!(ad.image, "capsule.png");
        assert_eq!(ad.caption_ru, "Новая игра");
        assert_eq!(ad.caption_en, "A new game");
    }

    #[test]
    fn an_ad_needs_a_web_link_and_a_picture_name_in_its_own_folder() {
        assert!(parse_ad("image=capsule.png\n").is_none(), "no link");
        assert!(parse_ad("url=javascript:alert(1)\nimage=capsule.png\n").is_none(), "not http(s)");
        assert!(parse_ad("url=https://a.b/\n").is_none(), "no picture");
        assert!(parse_ad("url=https://a.b/\nimage=../capsule.png\n").is_none(), "a path out of the folder");
        assert!(parse_ad("url=https://a.b/\nimage=sub/capsule.png\n").is_none(), "a path into a folder");
        let ad = parse_ad("url=http://a.b/\nimage=x.png\n").unwrap();
        assert_eq!((ad.caption_ru.as_str(), ad.caption_en.as_str()), ("", ""), "captions may be left out");
    }

    #[test]
    fn the_folders_are_taken_in_name_order() {
        let root = temp("order");
        for name in ["c-last", "a-first", "b-middle"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        std::fs::write(root.join("file.txt"), "x").unwrap();
        let names: Vec<String> = ad_folders(&root).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["a-first", "b-middle", "c-last"]);
        assert!(ad_folders(&root.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_first_valid_ad_in_order_is_chosen() {
        let root = temp("select");
        let png = b"picture";
        write_ad(&root, "a-broken", "caption_en=no link\n", Some(png));
        write_ad(&root, "b-no-picture", INI, None);
        write_ad(&root, "c-good", "url=https://good.example/\nimage=capsule.png\n", Some(png));
        write_ad(&root, "d-also-good", "url=https://second.example/\nimage=capsule.png\n", Some(png));
        let folders = ad_folders(&root);
        // The loader accepts any file that exists (no decoding in this test).
        let (ad, bytes) = select_ad(&folders, |p| std::fs::read(p).ok()).expect("an ad");
        assert_eq!(ad.url, "https://good.example/");
        assert_eq!(bytes, png);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_picture_that_does_not_load_skips_its_ad() {
        let root = temp("loader");
        write_ad(&root, "a", INI, Some(b"not a picture"));
        write_ad(&root, "b", INI, Some(b"also not"));
        let folders = ad_folders(&root);
        assert!(select_ad(&folders, |_| None::<()>).is_none());
        assert!(select_ad(&[], |_| Some(())).is_none(), "no folder, no ad");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_shipped_ad_is_valid_and_its_picture_decodes() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(ADS_DIR).join("thorny-times");
        let text = std::fs::read_to_string(dir.join(AD_FILE)).unwrap();
        let ad = parse_ad(&text).expect("the shipped ad.ini parses");
        assert_eq!(ad.url, "https://store.steampowered.com/app/2182610/");
        assert!(ad.caption_ru.starts_with("Новая игра") && !ad.caption_en.is_empty());
        let bytes = std::fs::read(dir.join(&ad.image)).unwrap();
        assert!(Image::from_file_with_format(&bytes, None).is_ok(), "the shipped picture decodes");
    }

    #[test]
    fn a_kept_none_is_seen_again_only_after_a_forget() {
        // The menu's first draw has no install: "none" is kept, and the install that the game
        // loads later (with its `ads/`) is found only once the slot is forgotten.
        let mut slot: Slot<&str> = Slot::new();
        let mut looks = 0;
        assert_eq!(slot.get_or_load(|| { looks += 1; None }), None);
        assert_eq!(slot.get_or_load(|| { looks += 1; Some("promo") }), None, "the kept none stands");
        assert_eq!(looks, 1);
        slot.forget();
        assert_eq!(slot.get_or_load(|| { looks += 1; Some("promo") }), Some("promo"));
        assert_eq!(slot.get_or_load(|| { looks += 1; None }), Some("promo"), "a found ad is kept");
        assert_eq!(looks, 2);
    }
}

/// The catalog's ad copy on disk, as the launch's catalog sets it ([`sync_tick`]).
enum Sync {
    /// The launch's catalog has not come yet.
    Waiting,
    /// The copy is being checked and, when it differs, downloaded (`Hub::install_ad`).
    Installing(Receiver<Result<bool, String>>),
    /// Decided for this launch.
    Done,
}

local! {
    static SYNC: std::cell::RefCell<Sync> = const { std::cell::RefCell::new(Sync::Waiting) };
}

/// Each frame: once the launch's catalog has come, its ad is kept in the app's folder
/// (`app_dir/ads`, `Hub::install_ad`): a copy that differs (id, version, picture or text) is downloaded
/// on a thread and then the menu looks for the ad again. Offline, a catalog without an ad or a failed
/// download leave what is on disk as it is.
pub fn sync_tick() {
    let state = SYNC.with(|s| std::mem::replace(&mut *s.borrow_mut(), Sync::Done));
    let next = match state {
        Sync::Done => Sync::Done,
        Sync::Waiting => match launch::catalog() {
            None => Sync::Waiting,
            Some(Err(why)) => {
                razdor::diag!("ads: no catalog this launch: {why}");
                Sync::Done
            }
            Some(Ok(catalog)) => match catalog.ad.clone() {
                Some(ad) => start_install(ad),
                None => Sync::Done,
            },
        },
        Sync::Installing(rx) => match rx.try_recv() {
            Err(TryRecvError::Empty) => Sync::Installing(rx),
            Ok(Ok(true)) => {
                razdor::diag!("ads: the promo is updated from the catalog");
                forget();
                Sync::Done
            }
            Ok(Ok(false)) => Sync::Done,
            Ok(Err(why)) => {
                razdor::diag!("ads: {why}");
                Sync::Done
            }
            Err(TryRecvError::Disconnected) => Sync::Done,
        },
    };
    SYNC.with(|s| *s.borrow_mut() = next);
}

/// Starts the copy's check on a thread: the hub's address and the app's `ads/` folder, or Done when
/// there is no folder for them.
fn start_install(ad: AdRef) -> Sync {
    let (Some(local), Some(app)) = (hub::local_dir(), razdor::platform::app_dir()) else { return Sync::Done };
    let hub = Hub { source: Source::from_env(), local };
    let root = app.join(ADS_DIR);
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("ad-copy".into()).spawn(move || {
        let _ = tx.send(hub.install_ad(&ad, &root));
    });
    match spawned {
        Ok(_) => Sync::Installing(rx),
        Err(e) => {
            razdor::diag!("ads: the copy cannot start: {e}");
            Sync::Done
        }
    }
}

#[cfg(test)]
mod order_tests {
    use super::*;

    #[test]
    fn the_app_copy_comes_before_the_program_folder_and_the_install() {
        let app = Path::new("/home/me/.local/share/razdor");
        let exe = Path::new("/opt/razdor");
        let install = Path::new("/home/me/Games/Discord Times");
        assert_eq!(
            ad_roots(None, Some(app), Some(exe), Some(install)),
            vec![app.join(ADS_DIR), exe.join(ADS_DIR), install.join(ADS_DIR)],
        );
        assert_eq!(ad_roots(None, None, None, None), Vec::<PathBuf>::new());
        assert_eq!(ad_roots(None, Some(app), None, Some(install)), vec![app.join(ADS_DIR), install.join(ADS_DIR)]);
    }

    #[test]
    fn the_environment_names_the_only_folder_when_set() {
        let app = Path::new("/a");
        assert_eq!(ad_roots(Some(OsStr::new("/dev/ads")), Some(app), Some(app), Some(app)), vec![PathBuf::from("/dev/ads")]);
        assert_eq!(ad_roots(Some(OsStr::new("")), Some(app), None, None), vec![app.join(ADS_DIR)], "an empty value is no setting");
    }
}
