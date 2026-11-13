//! The map hub (Razdor's own): a catalog of community maps, campaigns and
//! mods that the player browses and installs from inside the game (`ui::hub_view`).
//!
//! - **Catalog**: an `index.json` ([`Catalog`]) next to the files it names. Each entry is a
//!   map, a campaign (its chapters in order, linked by their next-map names) or a mod, with
//!   its titles and descriptions in Russian and English, the languages its maps come in
//!   (Russian as written, English from the import's translation) and the mods it needs.
//! - **Source** ([`Source`]): `RAZDOR_HUB` names an `https://` address or a folder; unset, the
//!   mirrors of [`mirror::HUB`] (GitVerse, then GitHub) are tried in order; a snapshot or `RAZDOR_HUB=mock`
//!   reads a mock hub written once under the hub folder ([`mock::build`]).
//! - **Cache** ([`read_cache`], [`write_cache`]): from an address, the catalog last fetched is kept
//!   with its address, and the window shows it at once while the current one is read.
//! - **Install**: a map or campaign needs its mods first ([`plan_install`] orders them,
//!   dependencies before what needs them). Every file is checked against the catalog's size
//!   and SHA-256, then written atomically: maps into `<hub>/maps/` (flat, so a campaign's
//!   next-map names find each other and the new-game list can read them), a mod's files into
//!   `<hub>/mods/<id>/files/` under their paths inside the mod (a mod ships only the files that
//!   differ from the base install, never the base's own). `<hub>/installed.json` records what
//!   went where.
//! - **Mods play as layers** ([`mods`]): [`prepare_mod`] builds `<hub>/mods/<id>/game/`, the
//!   base install's files hard-linked and the mod's files laid over them; [`active_install_dir`]
//!   is the folder the game reads (the active mod's, else the base's) and [`use_mod`] picks it.
//! - **Remove**: refused while an installed entry still needs the mod ([`needed_by`]); a mod's
//!   whole folder goes, its game folder with it.
//!
//! The hub folder is `RAZDOR_HUB_DIR`, else `hub` in the app's folder ([`crate::platform::app_dir`],
//! which is also the phones' storage).

mod mods;
/// Which mod a refused save needs (1552): pure decision, see the module.
pub mod save_mod;

pub use mods::*;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;
use crate::mirror;

/// Environment variable naming the hub: an `https://` address or a folder.
pub const SOURCE_ENV: &str = "RAZDOR_HUB";
/// Environment variable naming the local hub folder.
pub const DIR_ENV: &str = "RAZDOR_HUB_DIR";
/// The catalog's file name at the hub's root.
pub const INDEX: &str = "index.json";
/// The hub on GitHub, the default's fallback after GitVerse (`mirror::HUB`): the latest release of the
/// community repository (its assets are the catalog and the files it names).
pub const DEFAULT_SOURCE: &str = "https://github.com/indicozy/razdor-community/releases/latest/download";
/// The most a downloaded archive may be (the desktop's curl stops there).
pub const MAX_ARCHIVE_DOWNLOAD: u64 = 256 << 20;
/// The most a preview picture may be.
const MAX_PREVIEW_DOWNLOAD: u64 = 4 << 20;
/// A preview is at most this big (pixels); a bigger picture is not shown.
pub const PREVIEW_MAX_W: u32 = 480;
pub const PREVIEW_MAX_H: u32 = 270;

/// The address of `rel` in a hub at `base`: an `https://` address as it is, else under `base`.
pub fn resolve(base: &str, rel: &str) -> String {
    if rel.starts_with("https://") {
        rel.to_string()
    } else {
        format!("{}/{}", base.trim_end_matches('/'), rel.trim_start_matches('/'))
    }
}

/// Where a cached preview of `f` is: `<hub>/previews/<sha256>.png`.
pub fn preview_path(local: &Path, f: &FileRef) -> PathBuf {
    local.join("previews").join(format!("{}.png", f.sha256.to_lowercase()))
}

/// Whether `bytes` is a PNG no bigger than the preview limit (read from its header; the picture
/// is not decoded). Macroquad decodes PNG and TGA only, so the catalog's previews are PNG.
pub fn preview_ok(bytes: &[u8]) -> bool {
    const SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || !bytes.starts_with(SIGNATURE) || &bytes[12..16] != b"IHDR" {
        return false;
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    (1..=PREVIEW_MAX_W).contains(&w) && (1..=PREVIEW_MAX_H).contains(&h)
}

/// A PNG of `width` x `height` pixels from `rgba` (`width * height * 4` bytes, row by row): colour
/// type 6 (RGBA, 8 bit), filter 0 on every row, stored deflate blocks (no compression).
pub fn png_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let row = width as usize * 4;
    assert_eq!(rgba.len(), row * height as usize, "pixels for a {width}x{height} picture");
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for line in rgba.chunks(row.max(1)) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    png_chunk(&mut png, b"IHDR", &header);
    png_chunk(&mut png, b"IDAT", &z);
    png_chunk(&mut png, b"IEND", &[]);
    png
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { 0xEDB8_8320 ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// `razdor --map-picture <map.DTm> <out.png>` (the catalog pack asks for it, see
/// `tools/community-pack/build.py`): writes the map's own picture, the LIT picture it carries, as
/// a PNG, and exits without a window. Returns `None` for other arguments, else the exit code: 0
/// written, 2 the map has no picture (nothing is written), 1 an error.
pub fn export_cli(args: &[String]) -> Option<i32> {
    if args.first().map(String::as_str) != Some("--map-picture") {
        return None;
    }
    let (Some(map), Some(out)) = (args.get(1), args.get(2)) else {
        eprintln!("--map-picture needs a map file and a PNG file to write");
        return Some(1);
    };
    match map_picture(Path::new(map), Path::new(out)) {
        Ok(true) => Some(0),
        Ok(false) => Some(2),
        Err(e) => {
            eprintln!("{map}: {e}");
            Some(1)
        }
    }
}

/// Writes the picture of the map at `map` to `out` as a PNG; `Ok(false)` when the map has none.
fn map_picture(map: &Path, out: &Path) -> Result<bool, String> {
    let scenario = crate::dt::dtm::Scenario::load(map).map_err(|e| e.to_string())?;
    let Some(lit) = scenario.scenario_picture.as_deref() else {
        return Ok(false);
    };
    let img = crate::dt::gfx::decode_lit(lit).map_err(|e| format!("the picture: {e}"))?;
    if img.width == 0 || img.height == 0 || img.rgba.len() != img.width as usize * img.height as usize * 4 {
        return Err("the picture has no pixels".into());
    }
    std::fs::write(out, png_rgba(img.width, img.height, &img.rgba)).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(true)
}

/// A text in both interface languages.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    pub ru: String,
    #[serde(default)]
    pub en: String,
}

impl Text {
    /// The text in `lang`, else the Russian.
    pub fn get(&self, lang: Lang) -> &str {
        match lang {
            Lang::En if !self.en.trim().is_empty() => &self.en,
            _ => &self.ru,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Map,
    Campaign,
    Mod,
}

/// How well an entry works, as its author or the hub's checks say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Works as its author meant.
    Full,
    /// Works with the limits its note names.
    Partial,
    /// Does not start (its note says why).
    Broken,
    /// Not checked yet.
    #[default]
    Unknown,
}

/// One file of an entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    /// Where it is in the hub, relative to the catalog.
    pub path: String,
    /// The name it is installed under (a campaign's chapters keep the names their next-map
    /// fields use, whatever the language).
    pub name: String,
    /// The language of its texts (`ru`, `en`); none for a file that has no text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    pub size: u64,
    pub sha256: String,
    /// A `.zip` of files (mods only): `size` and `sha256` are the zip's, and its entries are
    /// unpacked under the mod's folder at their own paths.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archive: bool,
}

/// Likes and dislikes (👍/👎) on an entry's Discord post.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rating {
    pub up: u32,
    pub down: u32,
}

impl Rating {
    /// Likes minus dislikes.
    pub fn score(&self) -> i64 {
        i64::from(self.up) - i64::from(self.down)
    }

    /// All the votes.
    pub fn votes(&self) -> u64 {
        u64::from(self.up) + u64::from(self.down)
    }
}

/// The order of the list: rated entries first, by score, then by votes; unrated ones after
/// them; ties by the Russian title (case-insensitive).
pub fn by_rating(a: &Entry, b: &Entry) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let by_title = || a.title.ru.to_lowercase().cmp(&b.title.ru.to_lowercase());
    match (a.rating, b.rating) {
        (Some(x), Some(y)) => y.score().cmp(&x.score()).then_with(|| y.votes().cmp(&x.votes())).then_with(by_title),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => by_title(),
    }
}

/// An entry of the catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub kind: Kind,
    pub title: Text,
    pub author: String,
    pub version: String,
    #[serde(default)]
    pub description: Text,
    /// The map's own story (the `.DTm`'s description; a campaign's first chapter), when the catalog
    /// has it. The window does not draw it yet (it shows `description`, the post's text).
    #[serde(default)]
    pub about: Text,
    /// The languages its maps come in; `en` beyond the author's `ru` came from the import's
    /// machine translation when `machine_translated` is set.
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub machine_translated: bool,
    /// Ids of the mods it needs.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Where it was posted (a Discord message, a forum thread).
    #[serde(default)]
    pub source_url: String,
    /// Date it was added, `YYYY-MM-DD`.
    #[serde(default)]
    pub added: String,
    /// How well it works, with the note that says why not all of it.
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub status_note: Text,
    /// A picture for the window, when the hub has one (shown in the picked entry's place).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<FileRef>,
    /// The map's own picture (the `.DTm`'s embedded one; a campaign's first chapter), when the
    /// catalog has it: newer clients show it in place of `preview`, older ones ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<FileRef>,
    /// For a mod: the base install it is made for (e.g. "Community Update 1.2"). Nothing checks it.
    #[serde(default)]
    pub base: String,
    /// Likes and dislikes (👍/👎) on the post it came from, when it came from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<Rating>,
    pub files: Vec<FileRef>,
}

impl Entry {
    /// The files to install for `lang`: those in that language or with no language; when it
    /// has no file in `lang`, the Russian ones.
    pub fn files_for(&self, lang: &str) -> Vec<&FileRef> {
        let has = self.files.iter().any(|f| f.lang.as_deref() == Some(lang));
        let want = if has { lang } else { "ru" };
        self.files.iter().filter(|f| f.lang.as_deref().is_none_or(|l| l == want)).collect()
    }

    /// The number of maps it installs (a campaign's chapters).
    pub fn map_count(&self) -> usize {
        self.files_for("ru").iter().filter(|f| is_map_name(&f.name)).count()
    }
}

/// The game archive the hub offers (the catalog's `game`): a player with no game installed gets
/// it downloaded and installed at start (`ui::game_import`). Optional, and an older client ignores it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameRef {
    /// An `https://` address, or a path inside the hub (relative to the catalog).
    pub url: String,
    /// Other `https://` addresses of the same file, tried in order after `url` (a mirror). A bad one
    /// is dropped with a log line; an older client ignores them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mirrors: Vec<String>,
    /// The archive's size and SHA-256 (checked before anything is unpacked).
    pub size: u64,
    pub sha256: String,
    /// The file's name, as the player sees it (a safe file name).
    pub name: String,
}

/// The promo ad the hub offers (the catalog's `ad`, optional): the main menu's picture and its
/// texts (`ui::ads`). Read on every launch: a copy of it on the player's disk that differs (id, version
/// or picture) is downloaded and replaced. An older client ignores it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdRef {
    /// The ad's folder name (letters, digits, `-` and `_`).
    pub id: String,
    pub version: u32,
    /// The picture: an `https://` address, or a path in the hub (relative to the catalog).
    pub image: String,
    /// The picture's size and SHA-256 (checked before it is kept).
    pub size: u64,
    pub sha256: String,
    /// Where the ad leads: the game's page (`http://` or `https://`).
    pub url: String,
    /// The caption under the picture, in Russian and in English (one line each).
    pub caption_ru: String,
    pub caption_en: String,
}

/// The largest promo picture (the shipped one is 135 KiB).
pub const MAX_AD_BYTES: u64 = 2 << 20;
/// The most a caption may be, in bytes.
const MAX_CAPTION: usize = 200;

/// The hub's `index.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub format: u32,
    #[serde(default)]
    pub entries: Vec<Entry>,
    /// The game itself, when the hub has it (see [`GameRef`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<GameRef>,
    /// The promo ad of the main menu, when the hub has one (see [`AdRef`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ad: Option<AdRef>,
}

impl Catalog {
    /// Reads a catalog. Refused whole when any string that becomes part of a path is not safe:
    /// an id, a file name, a path, a SHA-256. The SHA-256s are stored lower-cased.
    pub fn parse(bytes: &[u8]) -> Result<Catalog, String> {
        let c: Catalog = serde_json::from_slice(bytes).map_err(|e| format!("index.json: {e}"))?;
        Catalog::checked(c)
    }

    /// The checks [`Catalog::parse`] makes, also on a catalog read back from the cache.
    fn checked(mut c: Catalog) -> Result<Catalog, String> {
        if c.format != 1 {
            return Err(format!("index.json: format {} is not known", c.format));
        }
        for e in &mut c.entries {
            // An id names a mod's folder (`mods/<id>/`) and the downloads' names: letters, digits, `-` and `_` only.
            if !safe_id(&e.id) || !e.requires.iter().all(|r| safe_id(r)) {
                return Err(format!("index.json: bad id {:?}", e.id));
            }
            for f in &mut e.files {
                // A mod's file keeps its place in the mod (`data/units.ini`); a map's is a name.
                let name_ok = if e.kind == Kind::Mod { safe_rel_name(&f.name) } else { safe_name(&f.name) };
                if !name_ok || !safe_location(&f.path) {
                    return Err(format!("index.json: {}: bad file name {:?}", e.id, f.name));
                }
                if !safe_sha256(&f.sha256) {
                    return Err(format!("index.json: {}: {}: the SHA-256 is not 64 hex digits", e.id, f.name));
                }
                f.sha256 = f.sha256.to_ascii_lowercase();
                if f.archive && e.kind != Kind::Mod {
                    return Err(format!("index.json: {}: only a mod may be an archive", e.id));
                }
            }
            for (what, slot) in [("preview", &mut e.preview), ("picture", &mut e.picture)] {
                if let Some(p) = slot {
                    if !safe_location(&p.path) {
                        return Err(format!("index.json: {}: bad {what} path", e.id));
                    }
                    if !safe_sha256(&p.sha256) {
                        return Err(format!("index.json: {}: the {what}'s SHA-256 is not 64 hex digits", e.id));
                    }
                    p.sha256 = p.sha256.to_ascii_lowercase();
                }
            }
        }
        if let Some(g) = &mut c.game {
            // The game's name is a file name, its address an https one (or a path in the hub), its size a
            // sane archive's: the same limit as any download.
            if !safe_name(&g.name) {
                return Err(format!("index.json: game: bad file name {:?}", g.name));
            }
            if g.url.starts_with("http://") {
                return Err("index.json: game: the address must be https".into());
            }
            if !safe_location(&g.url) {
                return Err(format!("index.json: game: bad address {:?}", g.url));
            }
            // A mirror is an https address and nothing else; one that is not is dropped, the catalog stays.
            g.mirrors.retain(|m| {
                let ok = m.starts_with("https://") && safe_location(m);
                if !ok {
                    crate::diag!("index.json: game: mirror {m:?} is dropped (not an https address)");
                }
                ok
            });
            if !safe_sha256(&g.sha256) {
                return Err("index.json: game: the SHA-256 is not 64 hex digits".into());
            }
            g.sha256 = g.sha256.to_ascii_lowercase();
            if g.size == 0 || g.size > MAX_ARCHIVE_DOWNLOAD {
                return Err(format!("index.json: game: bad size {}", g.size));
            }
        }
        if let Some(a) = &mut c.ad {
            // The ad's folder name, its picture (https or a path in the hub, a sane size), its
            // link and its captions (one line each): what the menu and the ad's file take.
            if !safe_id(&a.id) {
                return Err(format!("index.json: ad: bad id {:?}", a.id));
            }
            if !safe_location(&a.image) || (a.image.starts_with("http://")) {
                return Err(format!("index.json: ad: bad picture address {:?}", a.image));
            }
            if !safe_sha256(&a.sha256) {
                return Err("index.json: ad: the SHA-256 is not 64 hex digits".into());
            }
            a.sha256 = a.sha256.to_ascii_lowercase();
            if a.size == 0 || a.size > MAX_AD_BYTES {
                return Err(format!("index.json: ad: bad size {}", a.size));
            }
            if !(a.url.starts_with("https://") || a.url.starts_with("http://")) || a.url.len() <= 8 || a.url.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err(format!("index.json: ad: bad link {:?}", a.url));
            }
            for (what, text) in [("caption_ru", &a.caption_ru), ("caption_en", &a.caption_en)] {
                if text.len() > MAX_CAPTION || text.chars().any(|c| c == '\n' || c == '\r' || c.is_control()) {
                    return Err(format!("index.json: ad: bad {what}"));
                }
            }
        }
        Ok(c)
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }
}

/// An entry id: it becomes a folder name, so nothing that could leave the hub folder.
fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A file name the hub may write: no folders, no dots in front, nothing odd.
fn safe_name(name: &str) -> bool {
    // What a Windows folder takes: no control characters, no separators, none of `<>:"|?*`.
    const FORBIDDEN: &str = "/\\<>:\"|?*";
    if name.is_empty() || name.len() > 255 || name == "." || name == ".." {
        return false;
    }
    if name.starts_with(' ') || name.ends_with(' ') || name.ends_with('.') {
        return false;
    }
    if name.chars().any(|c| c.is_control() || FORBIDDEN.contains(c)) {
        return false;
    }
    !is_windows_device(name)
}

/// A Windows device name (`CON`, `NUL`, `COM1`…, with or without an extension, any case): such
/// a file cannot be made on Windows.
fn is_windows_device(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let numbered = |p: &str| stem.len() == 4 && stem.starts_with(p) && matches!(stem.as_bytes()[3], b'1'..=b'9');
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered("COM") || numbered("LPT")
}

/// A file name inside a mod: `/`-separated names the same way, relative, none empty or dotted.
pub fn safe_rel_name(name: &str) -> bool {
    !name.is_empty() && name.split('/').all(safe_name)
}

/// A path inside the hub: relative, no `..`.
fn safe_path(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('/') && !path.contains('\\') && path.split('/').all(|p| p != ".." && !p.is_empty())
}

/// The file `rel` in a folder hub: a path inside the folder, never an address or a path out.
fn folder_file(folder: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.starts_with("https://") || !safe_path(rel) {
        return Err(format!("{rel}: not a path inside the hub folder"));
    }
    Ok(folder.join(rel))
}

/// Where a file is in the hub: a path inside it, or an `https://` address (no spaces or
/// backslashes; it is fetched, never joined onto a folder).
fn safe_location(path: &str) -> bool {
    match path.strip_prefix("https://") {
        Some(rest) => !rest.is_empty() && !path.chars().any(|c| c.is_whitespace() || c == '\\'),
        None => safe_path(path),
    }
}

/// A SHA-256 as the catalog spells it: exactly 64 ASCII hex digits, either case. It names
/// the preview and download files, so nothing else may get into a path.
fn safe_sha256(sum: &str) -> bool {
    sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_map_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("DTm"))
}

/// What is installed: per entry id, its version, language and the files written.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    #[serde(default)]
    pub entries: BTreeMap<String, InstalledEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledEntry {
    pub version: String,
    pub kind: Kind,
    #[serde(default)]
    pub lang: String,
    /// Its title when it was installed (the picker lists mods without the catalog).
    #[serde(default)]
    pub title: Text,
    /// The mods it needed, so the new-game list can hide its maps without the mod.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Relative to the hub folder.
    pub files: Vec<String>,
    /// A map's catalog files as installed (path, size, SHA-256), to check the map files against
    /// and to download them again (`Hub::repair_maps`). Empty for a map installed before this,
    /// and for a mod (its manifest has them).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<FileRef>,
}

impl Installed {
    pub fn has(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }
}

/// What is installed in `local`, or nothing when the record is missing or does not read.
pub fn read_installed(local: &Path) -> Installed {
    std::fs::read(local.join("installed.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// What an older release's hub folder added to the local hub ([`import_old`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OldHubImport {
    /// The entries (maps, mods) copied in.
    pub entries: usize,
    /// The entries left out: the local hub has them already, a file is missing or collides with one
    /// of the local hub's, or a path is not a plain relative one.
    pub skipped: usize,
}

/// Whether `rel` is a plain relative path inside a folder: no root, no `..`, no empty part.
fn plain_relative(rel: &str) -> bool {
    !rel.is_empty() && !rel.starts_with(['/', '\\']) && rel.split(['/', '\\']).all(|p| !p.is_empty() && p != "." && p != "..")
}

/// Merges an older release's hub folder `src` into the hub folder `local` (Android's saves move, see
/// `ui::saves_move`). Each entry of `src`'s `installed.json` is copied with the files it lists, and
/// added to the local `installed.json` only when the local hub does not have that id yet (no
/// duplicate entries). An entry is skipped when one of its files is missing from `src`, is not a
/// plain relative path, or already exists in `local` (a local file is never overwritten).
pub fn import_old(src: &Path, local: &Path) -> Result<OldHubImport, String> {
    let old = read_installed(src);
    let mut mine = read_installed(local);
    let mut report = OldHubImport::default();
    for (id, entry) in old.entries {
        let usable = !mine.entries.contains_key(&id)
            && entry.files.iter().all(|f| plain_relative(f) && src.join(f).is_file() && !local.join(f).exists());
        if !usable {
            report.skipped += 1;
            continue;
        }
        for f in &entry.files {
            let to = local.join(f);
            if let Some(dir) = to.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{f}: {e}"))?;
            }
            std::fs::copy(src.join(f), &to).map_err(|e| format!("{f}: {e}"))?;
        }
        mine.entries.insert(id, entry);
        report.entries += 1;
    }
    if report.entries > 0 {
        let json = serde_json::to_vec_pretty(&mine).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(local).map_err(|e| e.to_string())?;
        crate::editor::files::write_atomically(&local.join("installed.json"), &json).map_err(|e| format!("installed.json: {e}"))?;
    }
    Ok(report)
}

#[cfg(test)]
mod import_old_tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("razdor-hub-old-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn entry(files: &[&str]) -> InstalledEntry {
        InstalledEntry { version: "1".into(), kind: Kind::Map, lang: "ru".into(), title: Text::default(), requires: vec![], files: files.iter().map(|f| f.to_string()).collect(), sources: vec![] }
    }

    fn write_installed(local: &Path, entries: &[(&str, InstalledEntry)]) {
        let mut i = Installed::default();
        for (id, e) in entries {
            i.entries.insert(id.to_string(), e.clone());
        }
        std::fs::write(local.join("installed.json"), serde_json::to_vec(&i).unwrap()).unwrap();
    }

    #[test]
    fn old_maps_and_mods_come_in_once_and_keep_the_local_ones() {
        let src = dir("src");
        let local = dir("local");
        std::fs::create_dir_all(src.join("maps")).unwrap();
        std::fs::write(src.join("maps/a.dtm"), b"A").unwrap();
        std::fs::write(src.join("maps/b.dtm"), b"B").unwrap();
        std::fs::create_dir_all(src.join("maps/x")).unwrap();
        std::fs::write(src.join("maps/x/kept.dtm"), b"local-version").unwrap();
        write_installed(&src, &[("a", entry(&["maps/a.dtm"])), ("b", entry(&["maps/b.dtm"])), ("missing", entry(&["maps/none.dtm"])), ("clash", entry(&["maps/x/kept.dtm"])), ("bad", entry(&["../escape.dtm"]))]);
        std::fs::create_dir_all(local.join("maps/x")).unwrap();
        std::fs::write(local.join("maps/x/kept.dtm"), b"mine").unwrap();
        write_installed(&local, &[("b", entry(&["maps/b.dtm"]))]);

        let report = import_old(&src, &local).unwrap();
        assert_eq!(report, OldHubImport { entries: 1, skipped: 4 }, "a, not b (already there), not missing, not the clash, not the escape");
        assert_eq!(std::fs::read(local.join("maps/a.dtm")).unwrap(), b"A");
        assert!(!local.join("maps/none.dtm").exists());
        assert_eq!(std::fs::read(local.join("maps/x/kept.dtm")).unwrap(), b"mine", "a local file is never overwritten");
        let installed = read_installed(&local);
        assert_eq!(installed.entries.len(), 2, "one entry per id, no duplicate");
        assert!(installed.entries.contains_key("a") && installed.entries.contains_key("b"));

        // The same import again changes nothing: the copied entry is there now.
        let again = import_old(&src, &local).unwrap();
        assert_eq!(again.entries, 0);
        assert_eq!(read_installed(&local).entries.len(), 2);
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&local);
    }
}

/// What a repair of the hub's maps (`Hub::repair_maps`) did: the entries restored, and those that
/// could not be, each with why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapRepair {
    pub restored: Vec<String>,
    pub failed: Vec<String>,
}

impl MapRepair {
    /// The repair to apply now, taken out of `pending`: none while the new-game list is on the
    /// screen (`list_open`), since the list's indexes must not move under the player; it stays
    /// pending until the list closes.
    pub fn take_when_closed(pending: &mut Option<MapRepair>, list_open: bool) -> Option<MapRepair> {
        if list_open {
            return None;
        }
        pending.take()
    }
}

/// The files of an installed map entry that are missing, changed or unreadable. With its sources
/// recorded each file is checked against them (size and SHA-256); without (installed before
/// sources were recorded) each file it lists must read.
fn entry_damaged_files(local: &Path, e: &InstalledEntry) -> Vec<PathBuf> {
    if e.sources.is_empty() {
        return e
            .files
            .iter()
            .filter(|rel| safe_path(rel))
            .map(|rel| local.join(rel))
            .filter(|p| std::fs::File::open(p).and_then(|mut f| std::io::copy(&mut f, &mut std::io::sink())).is_err())
            .collect();
    }
    // A name that is not one the hub may use is not joined to a path (the repair refuses it).
    e.sources
        .iter()
        .filter(|f| safe_name(&f.name))
        .filter_map(|f| {
            let path = local.join(format!("maps/{}", f.name));
            Hub::check_file(&path, f).is_err().then_some(path)
        })
        .collect()
}

/// The installed hub maps' files (paths in the hub folder) that are missing, changed or
/// unreadable. The new-game list leaves them out until `Hub::repair_maps` restores them.
pub fn damaged_map_files(local: &Path) -> Vec<PathBuf> {
    read_installed(local).entries.values().filter(|e| matches!(e.kind, Kind::Map | Kind::Campaign)).flat_map(|e| entry_damaged_files(local, e)).collect()
}

/// Why an entry cannot be installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// The catalog has no entry of this id (a needed mod the hub does not carry).
    Missing(String),
    /// Mods needing each other in a circle.
    Cycle(String),
}

/// What installing `id` takes, in order: the mods it needs that are not installed yet
/// (dependencies first, each once), then `id` itself (also when it is installed: a reinstall).
pub fn plan_install(catalog: &Catalog, installed: &Installed, id: &str) -> Result<Vec<String>, PlanError> {
    fn visit(catalog: &Catalog, installed: &Installed, id: &str, top: bool, path: &mut Vec<String>, out: &mut Vec<String>) -> Result<(), PlanError> {
        if out.iter().any(|o| o == id) || (!top && installed.has(id)) {
            return Ok(());
        }
        if path.iter().any(|p| p == id) {
            return Err(PlanError::Cycle(id.to_string()));
        }
        let e = catalog.entry(id).ok_or_else(|| PlanError::Missing(id.to_string()))?;
        path.push(id.to_string());
        for r in &e.requires {
            visit(catalog, installed, r, false, path, out)?;
        }
        path.pop();
        out.push(id.to_string());
        Ok(())
    }
    let mut out = Vec::new();
    visit(catalog, installed, id, true, &mut Vec::new(), &mut out)?;
    Ok(out)
}

/// The installed entries that need `id`.
pub fn needed_by(catalog: &Catalog, installed: &Installed, id: &str) -> Vec<String> {
    installed
        .entries
        .keys()
        .filter(|other| *other != id && catalog.entry(other).is_some_and(|e| e.requires.iter().any(|r| r == id)))
        .cloned()
        .collect()
}

/// Where the hub is read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// One address (`RAZDOR_HUB`).
    Url(String),
    /// The default: the mirrors of the hub in order (`mirror::HUB`, GitVerse first).
    Mirrors,
    Dir(PathBuf),
    /// The mock hub, written under the hub folder on first use.
    Mock,
}

/// The cache key of the default source (its catalog is the first mirror's or the next's).
const MIRRORS_KEY: &str = "mirrors";

impl Source {
    /// `RAZDOR_HUB`, read with [`Source::from_setting`]; snapshot runs render offline.
    pub fn from_env() -> Source {
        let snapshot = std::env::var_os("RAZDOR_SNAPSHOT").is_some();
        Source::from_setting(std::env::var(SOURCE_ENV).ok().as_deref(), snapshot)
    }

    /// The source a setting names: `mock`, an `https://` address, or a folder. Unset, the
    /// default release ([`DEFAULT_SOURCE`]), or the mock in a snapshot run.
    pub fn from_setting(value: Option<&str>, snapshot: bool) -> Source {
        match value.map(str::trim).filter(|s| !s.is_empty()) {
            Some("mock") => Source::Mock,
            Some(s) if s.starts_with("https://") => Source::Url(s.trim_end_matches('/').to_string()),
            Some(s) => Source::Dir(PathBuf::from(s)),
            None if snapshot => Source::Mock,
            None => Source::Mirrors,
        }
    }

    /// The address a catalog cache is kept for: an `https://` source (or the mirrors) only (a folder
    /// or the mock is read in place, instantly).
    pub fn cache_key(&self) -> Option<&str> {
        match self {
            Source::Url(u) => Some(u),
            Source::Mirrors => Some(MIRRORS_KEY),
            Source::Dir(_) | Source::Mock => None,
        }
    }

    /// Short words for the window's corner.
    pub fn label(&self) -> String {
        match self {
            Source::Url(u) => u.clone(),
            Source::Mirrors => mirror::HUB[0].to_string(),
            Source::Dir(d) => d.display().to_string(),
            Source::Mock => "mock".into(),
        }
    }
}

/// Makes every address in `c` absolute against `base`, the folder that gave the catalog: a catalog
/// names its files relative to itself, so each file is fetched from the host that gave the catalog.
pub fn rebase(c: &mut Catalog, base: &str) {
    fn abs(path: &mut String, base: &str) {
        *path = resolve(base, path.as_str());
    }
    for e in &mut c.entries {
        for f in &mut e.files {
            abs(&mut f.path, base);
        }
        for p in [&mut e.preview, &mut e.picture].into_iter().flatten() {
            abs(&mut p.path, base);
        }
    }
    if let Some(g) = &mut c.game {
        abs(&mut g.url, base);
    }
    if let Some(a) = &mut c.ad {
        abs(&mut a.image, base);
    }
}

/// The catalog of the default hub: the first of `mirror::HUB` whose `index.json` reads as a catalog
/// (`get` gives the bytes of an address, or why not; a web page instead of JSON is no catalog). Its
/// files are made absolute against that folder ([`rebase`]). When none reads, the last reason.
pub fn catalog_from_mirrors(bases: &[&str], mut get: impl FnMut(&str) -> Result<Vec<u8>, String>) -> Result<Catalog, String> {
    mirror::first_ok("hub catalog", bases, |base| {
        let mut c = Catalog::parse(&get(&resolve(base, INDEX))?)?;
        rebase(&mut c, base);
        Ok(c)
    })
}

/// The local hub folder: `RAZDOR_HUB_DIR`, else `hub` in the app's folder ([`crate::platform::app_dir`]:
/// the data folder on the desktop, the storage Razdor keeps across an uninstall on Android,
/// `Documents/Razdor` on iOS).
pub fn local_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os(DIR_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    crate::platform::app_dir().map(|d| d.join("hub"))
}

/// The folder installed maps go to.
pub fn maps_dir(local: &Path) -> PathBuf {
    local.join("maps")
}

/// The ad's text file in its folder: the keys the menu reads (`ui::ads`), plus the copy's version and picture.
pub const AD_INI: &str = "ad.ini";

/// The picture's file name in the ad's folder: `picture` and the address's own extension (`png` when it
/// has none that is a plain word).
pub fn ad_picture_name(a: &AdRef) -> String {
    let ext = a.image.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).filter(|e| !e.is_empty() && e.len() <= 4 && e.chars().all(|c| c.is_ascii_alphanumeric()));
    format!("picture.{}", ext.unwrap_or_else(|| "png".into()))
}

/// The `ad.ini` of the catalog's ad `a`, its picture named `picture` (see [`ad_picture_name`]). Exactly
/// this text is what the copy on disk must hold to be current ([`Hub::install_ad`]).
pub fn ad_ini_text(a: &AdRef, picture: &str) -> String {
    format!("url={}\nimage={picture}\ncaption_ru={}\ncaption_en={}\nversion={}\nsha256={}\n", a.url, a.caption_ru, a.caption_en, a.version, a.sha256)
}

/// The folder of the catalog cache.
pub const CACHE_DIR: &str = "cache";

/// The cache file: the catalog as last fetched from `source`, in `<hub>/cache/index.json`.
#[derive(Deserialize)]
struct CacheRead {
    source: String,
    catalog: Catalog,
}

#[derive(Serialize)]
struct CacheWrite<'a> {
    source: &'a str,
    catalog: &'a Catalog,
}

/// Writes `c`, fetched from `source`, to the cache (atomically: a temp file, then a rename).
pub fn write_cache(local: &Path, source: &str, c: &Catalog) -> Result<(), String> {
    let json = serde_json::to_vec(&CacheWrite { source, catalog: c }).map_err(|e| e.to_string())?;
    crate::editor::files::write_atomically(&local.join(CACHE_DIR).join(INDEX), &json).map_err(|e| format!("cache: {e}"))
}

/// The cached catalog, when it was fetched from `source` and reads back whole. A missing,
/// corrupt, or other-source cache is None (and is fetched again).
pub fn read_cache(local: &Path, source: &str) -> Option<Catalog> {
    let bytes = std::fs::read(local.join(CACHE_DIR).join(INDEX)).ok()?;
    let read: CacheRead = serde_json::from_slice(&bytes).ok()?;
    if read.source != source {
        return None;
    }
    let mut c = Catalog::checked(read.catalog).ok()?;
    // A cache from an address keeps the catalog's own paths: made absolute as the catalog was read.
    if source.starts_with("https://") {
        rebase(&mut c, source);
    }
    Some(c)
}

/// A hub and the local folder it installs into.
#[derive(Clone, Debug)]
pub struct Hub {
    pub source: Source,
    pub local: PathBuf,
}

impl Hub {
    fn root(&self) -> Result<Root, String> {
        match &self.source {
            Source::Url(u) => Ok(Root::Url(u.clone())),
            // Only addresses are fetched from the mirrors' catalog (its files are made absolute, see `rebase`).
            Source::Mirrors => Ok(Root::Url(mirror::HUB[0].to_string())),
            Source::Dir(d) => Ok(Root::Dir(d.clone())),
            Source::Mock => {
                let dir = self.local.join("mock-hub");
                if !dir.join(INDEX).is_file() {
                    mock::build(&dir).map_err(|e| format!("mock hub: {e}"))?;
                }
                Ok(Root::Dir(dir))
            }
        }
    }

    fn fetch(&self, rel: &str) -> Result<Vec<u8>, String> {
        match self.root()? {
            Root::Url(u) => crate::update::fetch(&resolve(&u, rel), 60),
            Root::Dir(d) => std::fs::read(folder_file(&d, rel)?).map_err(|e| format!("{rel}: {e}")),
        }
    }

    /// Downloads `rel` into the file `dest`, streamed (a folder hub copies it), at most
    /// `max_bytes` on the desktop.
    fn download(&self, rel: &str, dest: &Path, max_bytes: u64) -> Result<(), String> {
        let _ = std::fs::remove_file(dest);
        match self.root()? {
            Root::Url(u) => crate::update::download_to(&resolve(&u, rel), dest, 900, max_bytes),
            Root::Dir(d) => std::fs::copy(folder_file(&d, rel)?, dest).map(|_| ()).map_err(|e| format!("{rel}: {e}")),
        }
    }

    /// The file `f` is as the catalog says (its size and SHA-256), at `path`.
    fn check_file(path: &Path, f: &FileRef) -> Result<(), String> {
        let len = std::fs::metadata(path).map(|m| m.len()).map_err(|e| format!("{}: {e}", f.path))?;
        let sum = crate::update::sha256_file(path).map_err(|e| format!("{}: {e}", f.path))?;
        if len != f.size || sum != f.sha256.to_lowercase() {
            return Err(format!("{}: the file does not match the catalog", f.path));
        }
        Ok(())
    }

    /// The cached picture of an entry (`<hub>/previews/<sha256>.png`), downloaded when it is
    /// not there yet. Refused unless it is the catalog's file and a PNG within the preview limit.
    pub fn cache_preview(&self, f: &FileRef) -> Result<PathBuf, String> {
        let path = preview_path(&self.local, f);
        if path.is_file() {
            return Ok(path);
        }
        let dir = self.local.join("previews");
        std::fs::create_dir_all(&dir).map_err(|e| format!("previews: {e}"))?;
        let part = dir.join(format!("{}.part", f.sha256.to_lowercase()));
        self.download(&f.path, &part, MAX_PREVIEW_DOWNLOAD)?;
        let checked = Hub::check_file(&part, f).and_then(|()| {
            let bytes = std::fs::read(&part).map_err(|e| format!("{}: {e}", f.path))?;
            if preview_ok(&bytes) { Ok(()) } else { Err(format!("{}: not a PNG within {PREVIEW_MAX_W}x{PREVIEW_MAX_H}", f.path)) }
        });
        match checked {
            Ok(()) => std::fs::rename(&part, &path).map(|()| path).map_err(|e| format!("previews: {e}")),
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                Err(e)
            }
        }
    }

    /// Puts the catalog's promo ad on disk: `ads_root/<id>/` holds `ad.ini` and the picture, and the
    /// other folders of `ads_root` are removed (that folder is the catalog's copy alone). Nothing is
    /// written when the folder already holds this ad (`Ok(false)`). The picture is checked (size and
    /// SHA-256) before it is kept, and `ad.ini` is written last, so an update that failed half way is
    /// never read. Offline or a bad picture: an error, and the copy on disk stays.
    pub fn install_ad(&self, a: &AdRef, ads_root: &Path) -> Result<bool, String> {
        let folder = ads_root.join(&a.id);
        let picture = ad_picture_name(a);
        if fs::read_to_string(folder.join(AD_INI)).is_ok_and(|text| text == ad_ini_text(a, &picture)) {
            return Ok(false);
        }
        fs::create_dir_all(&folder).map_err(|e| format!("ads: {e}"))?;
        let part = folder.join(format!("{picture}.part"));
        let file = FileRef { path: a.image.clone(), name: picture.clone(), lang: None, size: a.size, sha256: a.sha256.clone(), archive: false };
        self.download(&a.image, &part, MAX_AD_BYTES).and_then(|()| Hub::check_file(&part, &file)).inspect_err(|_| {
            let _ = fs::remove_file(&part);
        })?;
        fs::rename(&part, folder.join(&picture)).map_err(|e| format!("ads: {e}"))?;
        if let Ok(entries) = fs::read_dir(ads_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path != folder && path.is_dir() {
                    let _ = fs::remove_dir_all(&path);
                }
            }
        }
        crate::editor::files::write_atomically(&folder.join(AD_INI), ad_ini_text(a, &picture).as_bytes()).map_err(|e| format!("ads: {e}"))?;
        Ok(true)
    }

    /// The hub's catalog: the default source tries its mirrors in order (`catalog_from_mirrors`); an
    /// address's files are made absolute against it (`rebase`); a folder or the mock is read as it is.
    pub fn catalog(&self) -> Result<Catalog, String> {
        match &self.source {
            Source::Mirrors => catalog_from_mirrors(&mirror::HUB, |url| crate::update::fetch(url, 60)),
            Source::Url(base) => {
                let mut c = Catalog::parse(&self.fetch(INDEX)?)?;
                rebase(&mut c, base);
                Ok(c)
            }
            Source::Dir(_) | Source::Mock => Catalog::parse(&self.fetch(INDEX)?),
        }
    }

    /// The catalog the cache holds for this hub's address (see [`read_cache`]); None for a
    /// folder or the mock.
    pub fn cached_catalog(&self) -> Option<Catalog> {
        read_cache(&self.local, self.source.cache_key()?)
    }

    /// Fetches the catalog and, from an address, writes it to the cache. A failed write is
    /// logged: the fetched catalog is still returned.
    pub fn fetch_catalog(&self) -> Result<Catalog, String> {
        let c = self.catalog()?;
        if let Some(source) = self.source.cache_key() {
            if let Err(why) = write_cache(&self.local, source, &c) {
                crate::diag!("hub: the catalog cache is not written: {why}");
            }
        }
        Ok(c)
    }

    pub fn installed(&self) -> Installed {
        read_installed(&self.local)
    }

    fn save_installed(&self, i: &Installed) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(i).map_err(|e| e.to_string())?;
        crate::editor::files::write_atomically(&self.local.join("installed.json"), &json).map_err(|e| format!("installed.json: {e}"))
    }

    /// Installs `id` and the mods it needs (see [`plan_install`]); returns the ids installed.
    pub fn install(&self, catalog: &Catalog, id: &str, lang: Lang) -> Result<Vec<String>, String> {
        let installed = self.installed();
        let plan = plan_install(catalog, &installed, id).map_err(|e| match e {
            PlanError::Missing(m) => format!("{m}: not in the hub"),
            PlanError::Cycle(m) => format!("{m}: mods need each other"),
        })?;
        let code = match lang {
            Lang::En => "en",
            Lang::Ru => "ru",
        };
        // Downloads (the archives) go to a scratch folder, removed after the install either way.
        let downloads = self.local.join("downloads");
        let _ = std::fs::remove_dir_all(&downloads);
        let done = self.install_plan(catalog, &plan, code, &downloads);
        let _ = std::fs::remove_dir_all(&downloads);
        done.map(|()| plan)
    }

    fn install_plan(&self, catalog: &Catalog, plan: &[String], code: &str, downloads: &Path) -> Result<(), String> {
        let mut installed = self.installed();
        for pid in plan {
            let e = catalog.entry(pid).expect("planned from the catalog");
            let files = e.files_for(code);
            let got = self.fetch_files(&e.id, &files, downloads)?;
            let got_sources: Vec<FileRef> = got.iter().map(|(f, _)| (*f).clone()).collect();
            let written = match e.kind {
                Kind::Mod => self.write_mod(&e.id, &got)?,
                Kind::Map | Kind::Campaign => {
                    let mut written = Vec::new();
                    for (f, g) in got {
                        let Got::Bytes(bytes) = g else { return Err(format!("{}: only a mod may be an archive", f.path)) };
                        let rel = format!("maps/{}", f.name);
                        crate::editor::files::write_atomically(&self.local.join(&rel), &bytes).map_err(|err| format!("{rel}: {err}"))?;
                        written.push(rel);
                    }
                    written
                }
            };
            let lang = files.iter().find_map(|f| f.lang.clone()).unwrap_or_default();
            let sources = if e.kind == Kind::Mod { Vec::new() } else { got_sources };
            let record = InstalledEntry { version: e.version.clone(), kind: e.kind, lang, title: e.title.clone(), requires: e.requires.clone(), files: written, sources };
            installed.entries.insert(e.id.clone(), record);
            self.save_installed(&installed)?;
        }
        Ok(())
    }

    /// The files of one entry, fetched and checked before anything is written (`files` in
    /// their catalog order). An archive is taken from its kept zip (`mods::archive_path`, its
    /// index is its place in `files`) when that still matches the catalog, else downloaded to
    /// `downloads`; the rest are fetched.
    fn fetch_files<'a>(&self, id: &str, files: &[&'a FileRef], downloads: &Path) -> Result<Vec<(&'a FileRef, Got)>, String> {
        let mut got = Vec::with_capacity(files.len());
        for (n, &f) in files.iter().enumerate() {
            // The names come from the local record too (a repair reads them): checked where they are used.
            if !safe_name(&f.name) {
                return Err(format!("{}: not a file name the hub may use", f.name));
            }
            if f.archive {
                let kept = mods::archive_path(&self.local, id, n);
                if kept.is_file() && Hub::check_file(&kept, f).is_ok() {
                    got.push((f, Got::Zip(kept)));
                    continue;
                }
                std::fs::create_dir_all(downloads).map_err(|err| format!("downloads: {err}"))?;
                let part = downloads.join(format!("{id}-{n}.zip"));
                self.download(&f.path, &part, MAX_ARCHIVE_DOWNLOAD)?;
                Hub::check_file(&part, f)?;
                got.push((f, Got::Zip(part)));
            } else {
                let bytes = self.fetch(&f.path)?;
                if bytes.len() as u64 != f.size || crate::update::sha256_hex(&bytes) != f.sha256.to_lowercase() {
                    return Err(format!("{}: the file does not match the catalog", f.path));
                }
                got.push((f, Got::Bytes(bytes)));
            }
        }
        Ok(got)
    }

    /// Writes a mod's files: the files it ships and its archives' entries, built in
    /// `files.tmp/` and put in place of the old `files/` in one rename (a reinstall replaces
    /// them all, and a file the new version dropped goes too). Then the archives are kept and
    /// the manifest written (`mods::Manifest`). Returns the records' paths.
    fn write_mod(&self, id: &str, got: &[(&FileRef, Got)]) -> Result<Vec<String>, String> {
        let tmp = mod_dir(&self.local, id).join("files.tmp");
        let _ = std::fs::remove_dir_all(&tmp);
        let built = self.build_mod_files(&tmp, got).and_then(|names| mods::file_entries(&tmp).map(|entries| (names, entries)));
        let (names, entries) = match built {
            Ok(built) => built,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(e);
            }
        };
        let files = mod_files_dir(&self.local, id);
        let _ = std::fs::remove_dir_all(&files);
        std::fs::rename(&tmp, &files).map_err(|e| format!("{}: {e}", files.display()))?;
        self.keep_archives(id, got)?;
        let manifest = mods::Manifest { files: entries, sources: got.iter().map(|(f, _)| (*f).clone()).collect() };
        mods::write_manifest(&self.local, id, &manifest)?;
        Ok(names.into_iter().map(|n| format!("mods/{id}/files/{n}")).collect())
    }

    /// Keeps the zips of `got`'s archives under `mods/<id>/archives/<n>.zip` (`n`: the place in
    /// `got`), replacing the kept ones. Without archives, none is kept.
    fn keep_archives(&self, id: &str, got: &[(&FileRef, Got)]) -> Result<(), String> {
        let dir = mod_dir(&self.local, id);
        let kept = dir.join("archives");
        let fresh = dir.join("archives.tmp");
        let _ = std::fs::remove_dir_all(&fresh);
        if !got.iter().any(|(_, g)| matches!(g, Got::Zip(_))) {
            let _ = std::fs::remove_dir_all(&kept);
            return Ok(());
        }
        std::fs::create_dir_all(&fresh).map_err(|e| format!("{}: {e}", fresh.display()))?;
        for (n, (_, g)) in got.iter().enumerate() {
            if let Got::Zip(path) = g {
                let to = fresh.join(format!("{n}.zip"));
                std::fs::rename(path, &to).or_else(|_| std::fs::copy(path, &to).map(|_| ())).map_err(|e| format!("{n}.zip: {e}"))?;
            }
        }
        let _ = std::fs::remove_dir_all(&kept);
        std::fs::rename(&fresh, &kept).map_err(|e| format!("{}: {e}", kept.display()))
    }

    /// Restores mod `id`'s files that are missing or changed (an antivirus may have quarantined
    /// them): the whole mod is written again from its sources, the kept zips where they still
    /// match and the rest downloaded and checked as an install checks them, and its merged folder
    /// is dropped to be built again. Returns whether anything was damaged. A mod with no manifest
    /// takes its sources from the catalog, which must hold the installed version.
    pub fn repair_mod(&self, id: &str) -> Result<bool, String> {
        let damaged = mods::damaged_files(&self.local, id)?;
        if damaged.is_empty() {
            return Ok(false);
        }
        let installed = self.installed();
        let record = installed.entries.get(id).filter(|e| e.kind == Kind::Mod).ok_or_else(|| format!("{id}: not installed"))?;
        let sources = match mods::read_manifest(&self.local, id) {
            Some(manifest) => manifest.sources,
            None => self.catalog_files(id, &record.version, if record.lang.is_empty() { "ru" } else { &record.lang })?,
        };
        let downloads = self.local.join("downloads");
        let _ = std::fs::remove_dir_all(&downloads);
        let refs: Vec<&FileRef> = sources.iter().collect();
        let done = self.fetch_files(id, &refs, &downloads).and_then(|got| self.write_mod(id, &got));
        let _ = std::fs::remove_dir_all(&downloads);
        done.map_err(|e| format!("{id}: {e}"))?;
        mods::drop_game_folder(&self.local, id);
        crate::diag!("hub: restored the damaged files of mod {id}: {}", damaged.join(", "));
        Ok(true)
    }

    /// The files of mod `id` in the catalog at `version`, for `lang` (the cached catalog when the
    /// hub cannot be reached).
    fn catalog_files(&self, id: &str, version: &str, lang: &str) -> Result<Vec<FileRef>, String> {
        let catalog = match self.catalog() {
            Ok(catalog) => catalog,
            Err(e) => self.cached_catalog().ok_or(e)?,
        };
        let entry = catalog.entry(id).ok_or_else(|| format!("{id}: not in the hub"))?;
        if entry.version != version {
            return Err(format!("{id}: the hub has version {}, {version} is installed", entry.version));
        }
        Ok(entry.files_for(lang).into_iter().cloned().collect())
    }

    /// Restores the installed maps whose files are missing, changed or unreadable (see
    /// [`damaged_map_files`]): each such entry's files are fetched again through the download
    /// path with its checks, written over the damaged ones, and its record kept with its sources.
    /// An entry installed before sources were recorded takes them from the catalog.
    pub fn repair_maps(&self) -> MapRepair {
        let installed = self.installed();
        let mut out = MapRepair::default();
        for (id, e) in installed.entries.iter().filter(|(_, e)| matches!(e.kind, Kind::Map | Kind::Campaign)) {
            if entry_damaged_files(&self.local, e).is_empty() {
                continue;
            }
            match self.repair_map_entry(id, e) {
                Ok(()) => out.restored.push(id.clone()),
                Err(why) => out.failed.push(format!("{id}: {why}")),
            }
        }
        out
    }

    fn repair_map_entry(&self, id: &str, e: &InstalledEntry) -> Result<(), String> {
        let lang = if e.lang.is_empty() { "ru" } else { e.lang.as_str() };
        let sources = if e.sources.is_empty() { self.catalog_files(id, &e.version, lang)? } else { e.sources.clone() };
        let downloads = self.local.join("downloads");
        let _ = std::fs::remove_dir_all(&downloads);
        let refs: Vec<&FileRef> = sources.iter().collect();
        let got = self.fetch_files(id, &refs, &downloads);
        let _ = std::fs::remove_dir_all(&downloads);
        for (f, g) in got? {
            let Got::Bytes(bytes) = g else { return Err(format!("{}: only a mod may be an archive", f.path)) };
            let rel = format!("maps/{}", f.name);
            crate::editor::files::write_atomically(&self.local.join(&rel), &bytes).map_err(|err| format!("{rel}: {err}"))?;
        }
        let mut installed = self.installed();
        if let Some(record) = installed.entries.get_mut(id) {
            record.sources = sources;
        }
        self.save_installed(&installed)
    }

    /// The files of `got` built in `tmp` (a fresh folder); their names.
    fn build_mod_files(&self, tmp: &Path, got: &[(&FileRef, Got)]) -> Result<Vec<String>, String> {
        std::fs::create_dir_all(tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let mut names = Vec::new();
        for (f, g) in got {
            match g {
                Got::Bytes(bytes) => {
                    crate::editor::files::write_atomically(&tmp.join(&f.name), bytes).map_err(|e| format!("{}: {e}", f.name))?;
                    names.push(f.name.clone());
                }
                Got::Zip(path) => names.extend(unpack_zip(path, tmp)?),
            }
        }
        Ok(names)
    }

    /// Removes `id`'s files, unless an installed entry needs it.
    pub fn remove(&self, catalog: &Catalog, id: &str) -> Result<(), String> {
        let mut installed = self.installed();
        let users = needed_by(catalog, &installed, id);
        if !users.is_empty() {
            return Err(format!("{id}: needed by {}", users.join(", ")));
        }
        let Some(e) = installed.entries.remove(id) else { return Ok(()) };
        if e.kind == Kind::Mod && safe_id(id) {
            // The mod's folder: its files and its merged game folder.
            let _ = std::fs::remove_dir_all(mod_dir(&self.local, id));
        } else {
            for rel in &e.files {
                if safe_path(rel) {
                    let _ = std::fs::remove_file(self.local.join(rel));
                }
            }
        }
        self.save_installed(&installed)
    }
}

enum Root {
    Url(String),
    Dir(PathBuf),
}

/// A file of an entry as it is fetched: its bytes, or the zip it is in (a file on disk).
enum Got {
    Bytes(Vec<u8>),
    Zip(PathBuf),
}

/// The mock hub: small empty maps made here (nothing from the game), with catalog texts of our
/// own, covering each case the window shows: maps in one or two languages, campaigns, mods,
/// a map needing a mod, a campaign needing a mod that needs another, and a map needing a mod
/// the hub does not carry.
pub mod mock {
    use super::*;
    use crate::dt::container;
    use crate::editor::defaults::{new_scenario, NewMap};

    struct MapSpec {
        file: &'static str,
        title: (&'static str, &'static str),
        about: (&'static str, &'static str),
        size: u32,
        fill: u8,
    }

    /// The bytes of a mock map in `lang`: an empty map with its texts. `kind` is header
    /// 0x10F (0 standalone, 1 a campaign's first map, 2 a later one).
    fn map_bytes(s: &MapSpec, lang: &str, campaign: &str, kind: u8, next: &str) -> Vec<u8> {
        let mut sc = new_scenario(NewMap { width: s.size, height: s.size, fill: s.fill });
        let en = lang == "en";
        sc.title = if en { s.title.1 } else { s.title.0 }.into();
        sc.description = if en { s.about.1 } else { s.about.0 }.into();
        sc.campaign_name = campaign.into();
        sc.next_map = next.into();
        sc.header.scenario_kind = kind;
        container::encode(19, &sc.to_payload())
    }

    struct Out<'a> {
        dir: &'a Path,
        entries: Vec<Entry>,
    }

    impl Out<'_> {
        fn file(&self, path: String, name: &str, lang: Option<&str>, bytes: &[u8]) -> std::io::Result<FileRef> {
            crate::editor::files::write_atomically(&self.dir.join(&path), bytes)?;
            Ok(FileRef { path, name: name.into(), lang: lang.map(Into::into), size: bytes.len() as u64, sha256: crate::update::sha256_hex(bytes), archive: false })
        }

        /// A map or campaign: its chapters in `langs`, linked by their next-map names.
        #[allow(clippy::too_many_arguments)]
        fn maps(&mut self, id: &str, kind: Kind, title: (&str, &str), author: &str, about: (&str, &str), langs: &[&str], requires: &[&str], added: &str, chapters: &[MapSpec]) -> std::io::Result<()> {
            let mut files = Vec::new();
            for lang in langs {
                for (i, ch) in chapters.iter().enumerate() {
                    let (campaign, k, next) = match kind {
                        Kind::Campaign => (if *lang == "en" { title.1 } else { title.0 }, if i == 0 { 1 } else { 2 }, chapters.get(i + 1).map_or(String::new(), |n| format!("{}.DTm", n.file))),
                        _ => ("", 0, String::new()),
                    };
                    let bytes = map_bytes(ch, lang, campaign, k, &next);
                    let name = format!("{}.DTm", ch.file);
                    files.push(self.file(format!("maps/{id}/{lang}/{name}"), &name, Some(lang), &bytes)?);
                }
            }
            // The mock's states: one that works, one partly, one that does not start, the rest unchecked.
            let (status, note) = match id {
                "black-wood" => (Status::Broken, ("Не запускается: в карте нет входа для героя.", "Does not start: the map has no entrance for the hero.")),
                "north-march" => (Status::Partial, ("Вторая глава без озвучки.", "The second chapter has no sounds.")),
                _ => (Status::Full, ("", "")),
            };
            self.entries.push(Entry {
                id: id.into(),
                kind,
                title: Text { ru: title.0.into(), en: title.1.into() },
                author: author.into(),
                version: "1.0".into(),
                description: Text { ru: about.0.into(), en: about.1.into() },
                about: chapters.first().map_or_else(Text::default, |ch| Text { ru: ch.about.0.into(), en: ch.about.1.into() }),
                languages: langs.iter().map(|l| l.to_string()).collect(),
                machine_translated: langs.contains(&"en"),
                requires: requires.iter().map(|r| r.to_string()).collect(),
                source_url: String::new(),
                added: added.into(),
                status,
                status_note: Text { ru: note.0.into(), en: note.1.into() },
                preview: None,
                picture: None,
                base: String::new(),
                rating: None,
                files,
            });
            Ok(())
        }

        /// A mod: a manifest only (a mock mod carries no game data).
        #[allow(clippy::too_many_arguments)]
        fn a_mod(&mut self, id: &str, title: (&str, &str), author: &str, version: &str, about: (&str, &str), requires: &[&str], added: &str) -> std::io::Result<()> {
            let manifest = serde_json::json!({ "id": id, "name": title.0, "version": version, "mock": true });
            let bytes = serde_json::to_vec_pretty(&manifest).expect("json");
            let f = self.file(format!("mods/{id}/mod.json"), "mod.json", None, &bytes)?;
            let (status, note) = if id == "evolution-mock" {
                (Status::Partial, ("Пока только отряды: предметов и заклинаний нет.", "Units only so far: no items or spells."))
            } else {
                (Status::Unknown, ("", ""))
            };
            self.entries.push(Entry {
                id: id.into(),
                kind: Kind::Mod,
                title: Text { ru: title.0.into(), en: title.1.into() },
                author: author.into(),
                version: version.into(),
                description: Text { ru: about.0.into(), en: about.1.into() },
                about: Text::default(),
                languages: vec!["ru".into()],
                machine_translated: false,
                requires: requires.iter().map(|r| r.to_string()).collect(),
                source_url: String::new(),
                added: added.into(),
                status,
                status_note: Text { ru: note.0.into(), en: note.1.into() },
                preview: None,
                picture: None,
                base: "Community Update 1.2".into(),
                rating: None,
                files: vec![f],
            });
            Ok(())
        }
    }

    /// A PNG of `width` x `height` (a gradient), made here by [`png_rgba`].
    pub fn preview_png(width: u32, height: u32) -> Vec<u8> {
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[(x * 255 / width.max(1)) as u8, (y * 255 / height.max(1)) as u8, 120, 255]);
            }
        }
        png_rgba(width, height, &rgba)
    }

    /// Writes the mock hub (its files and `index.json`) into `dir`.
    pub fn build(dir: &Path) -> std::io::Result<()> {
        let mut o = Out { dir, entries: Vec::new() };
        let map = |file, title, about, size, fill| MapSpec { file, title, about, size, fill };
        let picture = preview_png(PREVIEW_MAX_W, PREVIEW_MAX_H * 2 / 3);
        o.maps(
            "ford-outpost",
            Kind::Map,
            ("Застава у брода", "The Outpost at the Ford"),
            "Ратибор",
            ("Удержите брод, пока не подойдёт подмога из города.", "Hold the ford until help comes from the town."),
            &["ru", "en"],
            &[],
            "2026-09-02",
            &[map("Hub_Ford", ("Застава у брода", "The Outpost at the Ford"), ("Удержите брод.", "Hold the ford."), 40, 6)],
        )?;
        let preview = o.file("previews/ford-outpost.png".into(), "preview.png", None, &picture)?;
        if let Some(entry) = o.entries.last_mut() {
            entry.preview = Some(preview);
        }
        o.maps(
            "black-wood",
            Kind::Map,
            ("Тени Чернолесья", "Shadows of the Black Wood"),
            "Велимира",
            ("Лесные разбойники грабят обозы. Найдите их логово.", "Forest robbers raid the carts. Find their lair."),
            &["ru"],
            &[],
            "2026-09-11",
            &[map("Hub_BlackWood", ("Тени Чернолесья", "Shadows of the Black Wood"), ("Найдите логово разбойников.", "Find the robbers' lair."), 50, 7)],
        )?;
        o.maps(
            "north-march",
            Kind::Campaign,
            ("Сага о Северной марке", "The Saga of the North March"),
            "Гостомысл",
            ("Три главы: от пограничной крепости до столицы марки.", "Three chapters, from a border fort to the march's capital."),
            &["ru", "en"],
            &[],
            "2026-09-20",
            &[
                map("Hub_North1", ("Пограничная крепость", "The Border Fort"), ("Глава первая.", "Chapter one."), 40, 6),
                map("Hub_North2", ("Перевал", "The Pass"), ("Глава вторая.", "Chapter two."), 40, 9),
                map("Hub_North3", ("Столица марки", "The March's Capital"), ("Глава третья.", "Chapter three."), 50, 6),
            ],
        )?;
        o.a_mod(
            "evolution-mock",
            ("Эволюция (макет)", "Evolution (mock)"),
            "Сообщество",
            "9.0",
            ("Новые отряды, предметы и заклинания. В макете файлов игры нет.", "New units, items and spells. The mock carries no game files."),
            &[],
            "2026-08-15",
        )?;
        o.a_mod(
            "evolution-mercs",
            ("Эволюция: наёмники (макет)", "Evolution: Mercenaries (mock)"),
            "Сообщество",
            "1.2",
            ("Дополнение к «Эволюции»: гильдии наёмников.", "An add-on to Evolution: mercenary guilds."),
            &["evolution-mock"],
            "2026-09-01",
        )?;
        o.maps(
            "rotten-castle",
            Kind::Map,
            ("Осада Гнилого замка", "Siege of the Rotten Castle"),
            "Ратибор",
            ("Нужен мод «Эволюция»: в гарнизоне его новые отряды.", "Needs the Evolution mod: its new units hold the garrison."),
            &["ru", "en"],
            &["evolution-mock"],
            "2026-09-25",
            &[map("Hub_Rotten", ("Осада Гнилого замка", "Siege of the Rotten Castle"), ("Возьмите замок.", "Take the castle."), 40, 8)],
        )?;
        o.maps(
            "merc-road",
            Kind::Campaign,
            ("Наёмничья тропа", "The Mercenary Road"),
            "Велимира",
            ("Две главы для «Эволюции» с дополнением о наёмниках.", "Two chapters for Evolution with the mercenaries add-on."),
            &["ru"],
            &["evolution-mercs"],
            "2026-10-03",
            &[
                map("Hub_Merc1", ("Вербовка", "The Muster"), ("Глава первая.", "Chapter one."), 40, 6),
                map("Hub_Merc2", ("Расплата", "The Reckoning"), ("Глава вторая.", "Chapter two."), 40, 7),
            ],
        )?;
        o.maps(
            "lost-update",
            Kind::Map,
            ("Забытый остров", "The Forgotten Isle"),
            "Неизвестный",
            ("Сделана для мода, которого в хабе нет.", "Made for a mod the hub does not carry."),
            &["ru"],
            &["community-update"],
            "2026-10-08",
            &[map("Hub_Isle", ("Забытый остров", "The Forgotten Isle"), ("Найдите остров.", "Find the isle."), 30, 2)],
        )?;
        // The mock's votes: a few rated entries (two that tie on score, told apart by votes), the rest unrated.
        let votes = [("ford-outpost", 12, 1), ("north-march", 5, 0), ("rotten-castle", 7, 3), ("merc-road", 5, 1), ("evolution-mock", 20, 2)];
        for e in &mut o.entries {
            e.rating = votes.iter().find(|v| v.0 == e.id).map(|&(_, up, down)| Rating { up, down });
        }
        let catalog = Catalog { format: 1, entries: o.entries, game: None, ad: None };
        let json = serde_json::to_vec_pretty(&catalog).expect("json");
        crate::editor::files::write_atomically(&dir.join(INDEX), &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::Scenario;

    fn temp(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("razdor-hub-{tag}-{}-{nanos}", std::process::id()))
    }

    fn mock_hub(tag: &str) -> (Hub, Catalog) {
        let hub = Hub { source: Source::Mock, local: temp(tag) };
        let c = hub.catalog().unwrap();
        (hub, c)
    }

    #[test]
    fn the_mock_catalog_reads_back_and_its_maps_parse() {
        let (hub, c) = mock_hub("parse");
        assert!(c.entries.len() >= 8);
        for e in c.entries.iter().filter(|e| e.kind != Kind::Mod) {
            for f in &e.files {
                let bytes = std::fs::read(hub.local.join("mock-hub").join(&f.path)).unwrap();
                let s = Scenario::from_file_bytes(&bytes).unwrap();
                assert!(!s.title.is_empty(), "{}", f.path);
            }
        }
        let north = c.entry("north-march").unwrap();
        assert_eq!(north.map_count(), 3);
        let first = std::fs::read(hub.local.join("mock-hub").join(&north.files_for("ru")[0].path)).unwrap();
        let s = Scenario::from_file_bytes(&first).unwrap();
        assert_eq!((s.header.scenario_kind, s.next_map.as_str()), (1, "Hub_North2.DTm"));
    }

    #[test]
    fn files_follow_the_language_and_fall_back_to_russian() {
        let (_, c) = mock_hub("lang");
        let ford = c.entry("ford-outpost").unwrap();
        assert!(ford.files_for("en").iter().all(|f| f.lang.as_deref() == Some("en")));
        let wood = c.entry("black-wood").unwrap();
        assert!(wood.files_for("en").iter().all(|f| f.lang.as_deref() == Some("ru")));
        assert_eq!(wood.files_for("en").len(), 1);
    }

    #[test]
    fn mods_come_first_in_the_plan_and_once() {
        let (_, c) = mock_hub("plan");
        let none = Installed::default();
        assert_eq!(plan_install(&c, &none, "merc-road").unwrap(), ["evolution-mock", "evolution-mercs", "merc-road"]);
        assert_eq!(plan_install(&c, &none, "ford-outpost").unwrap(), ["ford-outpost"]);
        assert_eq!(plan_install(&c, &none, "lost-update"), Err(PlanError::Missing("community-update".into())));
        let mut some = Installed::default();
        some.entries.insert("evolution-mock".into(), InstalledEntry { version: "9.0".into(), kind: Kind::Mod, lang: String::new(), title: Text::default(), requires: vec![], files: vec![], sources: vec![] });
        assert_eq!(plan_install(&c, &some, "merc-road").unwrap(), ["evolution-mercs", "merc-road"]);
    }

    #[test]
    fn entries_sort_by_rating_then_votes_then_title() {
        use std::cmp::Ordering::{Greater, Less};
        let (_, c) = mock_hub("sort");
        let with = |title: &str, rating: Option<(u32, u32)>| {
            let mut e = c.entry("ford-outpost").unwrap().clone();
            e.title.ru = title.into();
            e.rating = rating.map(|(up, down)| Rating { up, down });
            e
        };
        // Higher score first.
        assert_eq!(by_rating(&with("Б", Some((5, 0))), &with("А", Some((3, 0)))), Less);
        // Equal score: more votes first, whatever the titles say.
        assert_eq!(by_rating(&with("А", Some((2, 0))), &with("Б", Some((4, 2)))), Greater);
        // Rated before unrated, even with a poor score.
        assert_eq!(by_rating(&with("А", Some((0, 9))), &with("Б", None)), Less);
        // Unrated entries share the rest by title, case-insensitively.
        assert_eq!(by_rating(&with("abc", None), &with("ABD", None)), Less);
        // Same score and votes: the title decides.
        assert_eq!(by_rating(&with("Яблоко", Some((1, 1))), &with("берёза", Some((1, 1)))), Greater);
        assert_eq!(Rating { up: 1, down: 4 }.score(), -3);
    }

    #[test]
    fn a_circle_of_mods_is_refused() {
        let (_, mut c) = mock_hub("cycle");
        c.entries.iter_mut().find(|e| e.id == "evolution-mock").unwrap().requires = vec!["evolution-mercs".into()];
        assert!(matches!(plan_install(&c, &Installed::default(), "merc-road"), Err(PlanError::Cycle(_))));
    }

    #[test]
    fn install_writes_the_maps_and_mods_and_remove_respects_needs() {
        let (hub, c) = mock_hub("install");
        let done = hub.install(&c, "merc-road", Lang::En).unwrap();
        assert_eq!(done.len(), 3);
        let maps = maps_dir(&hub.local);
        assert!(maps.join("Hub_Merc1.DTm").is_file());
        assert!(hub.local.join("mods/evolution-mock/files/mod.json").is_file());
        let inst = hub.installed();
        assert_eq!(inst.entries["merc-road"].lang, "ru", "no English files: the Russian ones");
        assert!(hub.remove(&c, "evolution-mock").is_err(), "the add-on needs it");
        hub.remove(&c, "merc-road").unwrap();
        hub.remove(&c, "evolution-mercs").unwrap();
        hub.remove(&c, "evolution-mock").unwrap();
        assert!(hub.installed().entries.is_empty());
        assert!(!hub.local.join("mods/evolution-mock").exists());
    }

    #[test]
    fn english_maps_install_under_the_chapter_names() {
        let (hub, c) = mock_hub("english");
        hub.install(&c, "north-march", Lang::En).unwrap();
        let s = Scenario::load(&maps_dir(&hub.local).join("Hub_North1.DTm")).unwrap();
        assert_eq!(s.title, "The Border Fort");
        assert_eq!(s.next_map, "Hub_North2.DTm");
    }

    #[test]
    fn a_damaged_file_is_not_installed() {
        let (hub, c) = mock_hub("damaged");
        let f = &c.entry("ford-outpost").unwrap().files[0];
        std::fs::write(hub.local.join("mock-hub").join(&f.path), b"broken").unwrap();
        assert!(hub.install(&c, "ford-outpost", Lang::Ru).is_err());
        assert!(!hub.installed().has("ford-outpost"));
    }

    #[test]
    fn the_catalog_refuses_paths_out_of_the_hub() {
        let bad = br#"{"format":1,"entries":[{"id":"x","kind":"map","title":{"ru":"x"},"author":"a","version":"1","files":[{"path":"../x","name":"x.DTm","size":1,"sha256":""}]}]}"#;
        assert!(Catalog::parse(bad).is_err());
        let bad_name = br#"{"format":1,"entries":[{"id":"x","kind":"map","title":{"ru":"x"},"author":"a","version":"1","files":[{"path":"x","name":"../x.DTm","size":1,"sha256":""}]}]}"#;
        assert!(Catalog::parse(bad_name).is_err());
        let bad_id = br#"{"format":1,"entries":[{"id":"../../x","kind":"mod","title":{"ru":"x"},"author":"a","version":"1","files":[{"path":"x","name":"mod.json","size":1,"sha256":""}]}]}"#;
        assert!(Catalog::parse(bad_id).is_err());
        let bad_need = br#"{"format":1,"entries":[{"id":"x","kind":"map","title":{"ru":"x"},"author":"a","version":"1","requires":["a/b"],"files":[]}]}"#;
        assert!(Catalog::parse(bad_need).is_err());
    }

    /// A catalog with one mod whose file (and, with `preview`, its picture) has the SHA-256 `sum`.
    fn catalog_with_sum(sum: &str, preview: bool) -> String {
        let picture = if preview { format!(r#","preview":{{"path":"previews/p.png","name":"p.png","size":1,"sha256":"{sum}"}}"#) } else { String::new() };
        format!(
            r#"{{"format":1,"entries":[{{"id":"m","kind":"mod","title":{{"ru":"x"}},"author":"a","version":"1","files":[{{"path":"mods/m/a.ini","name":"a.ini","size":1,"sha256":"{sum}"}}]{picture}}}]}}"#
        )
    }

    #[test]
    fn a_sha256_that_is_not_64_hex_digits_refuses_the_whole_catalog() {
        assert!(Catalog::parse(catalog_with_sum(&"ab".repeat(32), true).as_bytes()).is_ok());
        let bad = vec![
            "../../x".to_string(),
            "a".repeat(63),
            "a".repeat(65),
            format!("{}/..", "a".repeat(62)),
            format!("{}\\", "a".repeat(63)),
            "g".repeat(64),
            String::new(),
        ];
        for sum in &bad {
            assert!(Catalog::parse(catalog_with_sum(sum, false).as_bytes()).is_err(), "{sum:?} as a file's SHA-256");
            assert!(Catalog::parse(catalog_with_sum(sum, true).as_bytes()).is_err(), "{sum:?} as the preview's SHA-256");
        }
    }

    #[test]
    fn the_sha256_is_kept_in_lower_case() {
        let c = Catalog::parse(catalog_with_sum(&"AB".repeat(32), true).as_bytes()).unwrap();
        assert_eq!(c.entries[0].files[0].sha256, "ab".repeat(32));
        assert_eq!(c.entries[0].preview.as_ref().unwrap().sha256, "ab".repeat(32));
    }

    /// A `picture` is checked as a `preview` is: a path out of the hub or a bad SHA-256 refuses the whole
    /// catalog; a good one reads back (lower case), and a catalog without it reads with none.
    #[test]
    fn a_bad_picture_is_refused_like_a_bad_preview() {
        let entry = |picture: &str| {
            format!(
                r#"{{"format":1,"entries":[{{"id":"m","kind":"map","title":{{"ru":"x"}},"author":"a","version":"1","files":[],"picture":{picture}}}]}}"#
            )
        };
        let good = format!(r#"{{"path":"pictures/p.png","name":"picture.png","size":1,"sha256":"{}"}}"#, "AB".repeat(32));
        let c = Catalog::parse(entry(&good).as_bytes()).unwrap();
        assert_eq!(c.entries[0].picture.as_ref().unwrap().sha256, "ab".repeat(32));
        assert!(Catalog::parse(entry(r#"{"path":"pictures/p.png","name":"p.png","size":1,"sha256":"ab"}"#).as_bytes()).is_err());
        assert!(Catalog::parse(entry(r#"{"path":"../../x.png","name":"p.png","size":1,"sha256":"ab"}"#).as_bytes()).is_err());
        let bad_sum = format!(r#"{{"path":"pictures/p.png","name":"p.png","size":1,"sha256":"{}"}}"#, "g".repeat(64));
        assert!(Catalog::parse(entry(&bad_sum).as_bytes()).is_err());
        let none = Catalog::parse(entry("null").as_bytes()).unwrap();
        assert!(none.entries[0].picture.is_none());
    }

    #[test]
    fn an_address_path_is_fetched_as_it_is_and_never_joined_onto_a_folder() {
        let sum = "ab".repeat(32);
        let json = |path: &str| {
            format!(r#"{{"format":1,"entries":[{{"id":"m","kind":"mod","title":{{"ru":"x"}},"author":"a","version":"1","files":[{{"path":"{path}","name":"a.ini","size":1,"sha256":"{sum}"}}]}}]}}"#)
        };
        assert!(Catalog::parse(json("https://example.org/a.ini").as_bytes()).is_ok());
        assert!(Catalog::parse(json("https://exa mple.org/a.ini").as_bytes()).is_err(), "a space in an address");
        assert!(Catalog::parse(json("https://").as_bytes()).is_err(), "no address after the scheme");
        assert!(folder_file(Path::new("/hub"), "https://example.org/a.ini").is_err());
        assert!(folder_file(Path::new("/hub"), "../a.ini").is_err());
        assert_eq!(folder_file(Path::new("/hub"), "mods/m/a.ini").unwrap(), Path::new("/hub/mods/m/a.ini"));
    }

    #[test]
    fn the_default_source_is_the_mirrors_and_the_mock_only_when_asked() {
        assert_eq!(Source::from_setting(None, false), Source::Mirrors);
        assert_eq!(Source::from_setting(Some("  "), false), Source::Mirrors);
        assert_eq!(Source::Mirrors.cache_key(), Some("mirrors"));
        assert_eq!(Source::Mirrors.label(), mirror::HUB[0]);
        assert_eq!(Source::from_setting(Some("mock"), false), Source::Mock);
        assert_eq!(Source::from_setting(None, true), Source::Mock, "a snapshot renders offline");
        assert_eq!(Source::from_setting(Some("https://example.org/hub/"), false), Source::Url("https://example.org/hub".into()));
        assert_eq!(Source::from_setting(Some("/games/hub"), false), Source::Dir(PathBuf::from("/games/hub")));
    }

    #[test]
    fn paths_resolve_under_the_hub_or_as_their_own_address() {
        assert_eq!(resolve(DEFAULT_SOURCE, "mods/m/pack.zip"), format!("{DEFAULT_SOURCE}/mods/m/pack.zip"));
        assert_eq!(resolve(DEFAULT_SOURCE, "index.json"), format!("{DEFAULT_SOURCE}/index.json"));
        assert_eq!(resolve(DEFAULT_SOURCE, "https://example.org/a.png"), "https://example.org/a.png");
        assert_eq!(resolve("https://example.org/hub/", "maps/a.DTm"), "https://example.org/hub/maps/a.DTm");
    }

    #[test]
    fn an_archive_is_for_a_mod_only() {
        let file = |kind: &str| {
            format!(r#"{{"format":1,"entries":[{{"id":"x","kind":"{kind}","title":{{"ru":"x"}},"author":"a","version":"1","files":[{{"path":"p.zip","name":"x.zip","size":1,"sha256":"00000000000000000000000000000000000000000000000000000000000000ab","archive":true}}]}}]}}"#)
        };
        assert!(Catalog::parse(file("mod").as_bytes()).is_ok());
        assert!(Catalog::parse(file("map").as_bytes()).is_err());
        assert!(Catalog::parse(file("campaign").as_bytes()).is_err());
    }

    /// A folder hub with one mod, `pack`, whose files are a zip: returns the hub and the zip's
    /// size and SHA-256.
    fn zip_hub(tag: &str) -> (PathBuf, PathBuf, u64, String) {
        use std::io::Write;
        let root = temp(&format!("{tag}-root"));
        let local = temp(&format!("{tag}-local"));
        std::fs::create_dir_all(root.join("mods/pack")).unwrap();
        let mut w = zip::ZipWriter::new(std::fs::File::create(root.join("mods/pack/pack.zip")).unwrap());
        w.start_file("data/units.ini", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(b"units").unwrap();
        w.start_file("Pictures/a.lit", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(b"pic").unwrap();
        w.finish().unwrap();
        let bytes = std::fs::read(root.join("mods/pack/pack.zip")).unwrap();
        let sum = crate::update::sha256_hex(&bytes);
        let json = serde_json::json!({"format": 1, "entries": [{
            "id": "pack", "kind": "mod", "title": {"ru": "Пакет"}, "author": "a", "version": "1.0",
            "files": [{"path": "mods/pack/pack.zip", "name": "pack.zip", "size": bytes.len(), "sha256": sum, "archive": true}]
        }]});
        std::fs::write(root.join(INDEX), json.to_string()).unwrap();
        (root, local, bytes.len() as u64, sum)
    }

    #[test]
    fn a_mod_zip_installs_its_entries_from_a_folder_hub() {
        let (root, local, _, _) = zip_hub("zip-install");
        let hub = Hub { source: Source::Dir(root.clone()), local: local.clone() };
        let catalog = hub.catalog().unwrap();
        assert_eq!(hub.install(&catalog, "pack", Lang::Ru).unwrap(), ["pack"]);
        assert_eq!(std::fs::read(local.join("mods/pack/files/data/units.ini")).unwrap(), b"units");
        assert_eq!(std::fs::read(local.join("mods/pack/files/Pictures/a.lit")).unwrap(), b"pic");
        assert!(hub.installed().entries["pack"].files.contains(&"mods/pack/files/data/units.ini".to_string()));
        assert!(!local.join("downloads").exists(), "the download is removed after the install");
        assert!(!local.join("mods/pack/files.tmp").exists());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_mod_zip_that_does_not_match_the_catalog_writes_nothing() {
        let (root, local, size, _) = zip_hub("zip-bad");
        let json = std::fs::read_to_string(root.join(INDEX)).unwrap();
        let wrong = json.replace(&crate::update::sha256_hex(&std::fs::read(root.join("mods/pack/pack.zip")).unwrap()), &"0".repeat(64));
        std::fs::write(root.join(INDEX), wrong).unwrap();
        let hub = Hub { source: Source::Dir(root.clone()), local: local.clone() };
        let catalog = hub.catalog().unwrap();
        assert!(hub.install(&catalog, "pack", Lang::Ru).is_err());
        assert!(!local.join("mods/pack/files").exists());
        assert!(!local.join("mods/pack/files.tmp").exists());
        assert!(!hub.installed().has("pack"));
        assert!(size > 0);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// Installs the zip mod `pack` from a folder hub and returns the hub and its folders.
    fn installed_pack(tag: &str) -> (Hub, PathBuf, PathBuf) {
        let (root, local, _, _) = zip_hub(tag);
        let hub = Hub { source: Source::Dir(root.clone()), local: local.clone() };
        let catalog = hub.catalog().unwrap();
        hub.install(&catalog, "pack", Lang::Ru).unwrap();
        (hub, root, local)
    }

    #[test]
    fn an_install_keeps_the_zip_and_writes_a_manifest() {
        let (_hub, root, local) = installed_pack("manifest");
        let manifest = mods::read_manifest(&local, "pack").expect("the install writes a manifest");
        let paths: Vec<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["Pictures/a.lit", "data/units.ini"]);
        assert_eq!(manifest.sources.len(), 1);
        assert_eq!(std::fs::read(mods::archive_path(&local, "pack", 0)).unwrap(), std::fs::read(root.join("mods/pack/pack.zip")).unwrap());
        assert!(mods::damaged_files(&local, "pack").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_missing_and_a_changed_mod_file_are_restored_from_the_kept_zip() {
        let (hub, root, local) = installed_pack("restore");
        std::fs::remove_file(local.join("mods/pack/files/data/units.ini")).unwrap();
        std::fs::write(local.join("mods/pack/files/Pictures/a.lit"), b"changed").unwrap();
        // The catalog's zip is gone: the kept one is what restores the files.
        std::fs::remove_file(root.join("mods/pack/pack.zip")).unwrap();
        assert_eq!(mods::damaged_files(&local, "pack").unwrap().len(), 2);
        assert!(hub.repair_mod("pack").unwrap());
        assert_eq!(std::fs::read(local.join("mods/pack/files/data/units.ini")).unwrap(), b"units");
        assert_eq!(std::fs::read(local.join("mods/pack/files/Pictures/a.lit")).unwrap(), b"pic");
        assert!(mods::damaged_files(&local, "pack").unwrap().is_empty());
        assert!(!hub.repair_mod("pack").unwrap(), "an intact mod is not written again");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_repair_without_the_kept_zip_downloads_it_again_and_checks_it() {
        let (hub, root, local) = installed_pack("download");
        std::fs::remove_dir_all(local.join("mods/pack/archives")).unwrap();
        std::fs::remove_file(local.join("mods/pack/files/data/units.ini")).unwrap();
        assert!(hub.repair_mod("pack").unwrap());
        assert_eq!(std::fs::read(local.join("mods/pack/files/data/units.ini")).unwrap(), b"units");
        assert!(mods::archive_path(&local, "pack", 0).is_file(), "the zip is kept again");
        assert!(!local.join("downloads").exists());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_mod_that_cannot_be_restored_says_why_and_stays_damaged() {
        let (hub, root, local) = installed_pack("unrestorable");
        std::fs::remove_file(local.join("mods/pack/files/data/units.ini")).unwrap();
        std::fs::remove_dir_all(local.join("mods/pack/archives")).unwrap();
        std::fs::remove_file(root.join("mods/pack/pack.zip")).unwrap();
        let err = hub.repair_mod("pack").unwrap_err();
        assert!(err.starts_with("pack:"), "{err}");
        assert_eq!(mods::damaged_files(&local, "pack").unwrap(), ["data/units.ini"]);
        assert!(!local.join("downloads").exists(), "nothing is left behind");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_mod_without_a_manifest_is_repaired_only_when_a_file_does_not_read() {
        let (hub, root, local) = installed_pack("legacy");
        let manifest = mods::mod_dir(&local, "pack").join("manifest.json");
        std::fs::remove_file(&manifest).unwrap();
        // Without a manifest a missing file is not known to be damage (only a file that does not
        // read is): nothing is written again, and no manifest is made from the damaged files.
        std::fs::remove_file(local.join("mods/pack/files/data/units.ini")).unwrap();
        assert!(!hub.repair_mod("pack").unwrap());
        assert!(!manifest.exists(), "no manifest is made from damaged files");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_deleted_or_changed_hub_map_is_fetched_again() {
        let (hub, c) = mock_hub("map-repair");
        hub.install(&c, "merc-road", Lang::En).unwrap();
        let map = hub.local.join("maps/Hub_Merc1.DTm");
        let want = std::fs::read(&map).unwrap();
        assert!(damaged_map_files(&hub.local).is_empty(), "an intact map is not damaged");
        // Deleted (an antivirus took it): listed as damaged, then fetched again through the download path.
        std::fs::remove_file(&map).unwrap();
        assert_eq!(damaged_map_files(&hub.local), vec![map.clone()]);
        let out = hub.repair_maps();
        assert_eq!(out.restored, ["merc-road"]);
        assert!(out.failed.is_empty(), "{:?}", out.failed);
        assert_eq!(std::fs::read(&map).unwrap(), want);
        // Changed: its bytes differ from the catalog's, so it is fetched again too.
        std::fs::write(&map, b"changed").unwrap();
        assert_eq!(damaged_map_files(&hub.local), vec![map.clone()]);
        assert_eq!(hub.repair_maps().restored, ["merc-road"]);
        assert_eq!(std::fs::read(&map).unwrap(), want);
        assert!(damaged_map_files(&hub.local).is_empty());
        assert!(hub.repair_maps() == MapRepair::default(), "nothing to do once intact");
        let _ = std::fs::remove_dir_all(&hub.local);
    }

    #[test]
    fn a_hub_map_that_cannot_be_fetched_again_is_reported_and_left_out() {
        let (hub, c) = mock_hub("map-unfetchable");
        hub.install(&c, "merc-road", Lang::En).unwrap();
        let map = hub.local.join("maps/Hub_Merc1.DTm");
        std::fs::remove_file(&map).unwrap();
        // The hub (an empty folder here) no longer has the file: the repair says so and the map stays out of the list.
        let gone = Hub { source: Source::Dir(hub.local.join("empty-hub")), local: hub.local.clone() };
        let out = gone.repair_maps();
        assert!(out.restored.is_empty());
        assert_eq!(out.failed.len(), 1, "{:?}", out.failed);
        assert!(out.failed[0].starts_with("merc-road:"), "{:?}", out.failed);
        assert_eq!(damaged_map_files(&hub.local), vec![map.clone()]);
        let _ = std::fs::remove_dir_all(&hub.local);
    }

    #[test]
    fn a_map_repair_waits_while_the_list_is_open() {
        let mut pending = Some(MapRepair { restored: vec!["merc-road".into()], failed: vec![] });
        assert!(MapRepair::take_when_closed(&mut pending, true).is_none(), "the list is on the screen: nothing changes");
        assert!(pending.is_some(), "the repair is kept");
        let now = MapRepair::take_when_closed(&mut pending, false).expect("applied once the list closes");
        assert_eq!(now.restored, ["merc-road"]);
        assert!(pending.is_none());
    }

    #[test]
    fn a_tampered_record_with_a_name_outside_the_maps_folder_writes_nothing() {
        let (hub, c) = mock_hub("map-tamper");
        hub.install(&c, "merc-road", Lang::En).unwrap();
        // A local record edited by hand: a source named out of the maps folder, next to a real map
        // that is missing, so the repair runs.
        let outside = format!("razdor-tamper-{}", std::process::id());
        let mut installed = read_installed(&hub.local);
        installed.entries.get_mut("merc-road").unwrap().sources.push(FileRef {
            path: "x".into(),
            name: format!("../{outside}"),
            lang: None,
            size: 1,
            sha256: "0".repeat(64),
            archive: false,
        });
        std::fs::write(hub.local.join("installed.json"), serde_json::to_vec(&installed).unwrap()).unwrap();
        std::fs::remove_file(hub.local.join("maps/Hub_Merc1.DTm")).unwrap();
        let out = hub.repair_maps();
        assert!(out.restored.is_empty());
        assert_eq!(out.failed.len(), 1, "{:?}", out.failed);
        assert!(!hub.local.join(&outside).exists(), "nothing is written beside the hub folder");
        assert!(!hub.local.join("maps").join(format!("../{outside}")).exists());
        assert!(!hub.local.parent().unwrap().join(&outside).exists(), "nothing is written outside the hub");
        let _ = std::fs::remove_dir_all(&hub.local);
    }

    #[test]
    fn previews_are_png_within_the_limit_and_the_mock_one_decodes() {
        let png = mock::preview_png(480, 270);
        assert!(preview_ok(&png));
        assert!(!preview_ok(&mock::preview_png(481, 270)), "wider than the limit");
        assert!(!preview_ok(b"GIF89a................................"));
        let img = macroquad::texture::Image::from_file_with_format(&png, None).expect("the mock's PNG decodes");
        assert_eq!((img.width, img.height), (480, 270));
    }

    #[test]
    fn a_rgba_picture_is_written_as_a_png_that_decodes_back() {
        let pixels: Vec<u8> = (0..3 * 2 * 4).map(|i| (i * 17) as u8).collect();
        let png = png_rgba(3, 2, &pixels);
        assert!(preview_ok(&png));
        let img = macroquad::texture::Image::from_file_with_format(&png, None).expect("the PNG decodes");
        assert_eq!((img.width, img.height), (3, 2));
        assert_eq!(img.bytes, pixels);
    }

    /// `--map-picture` is for its own arguments only; the other command lines start the game.
    #[test]
    fn export_cli_takes_only_its_own_flag() {
        assert_eq!(export_cli(&[]), None);
        assert_eq!(export_cli(&["--replay".into(), "actions.jsonl".into()]), None);
        assert_eq!(export_cli(&["--map-picture".into()]), Some(1), "a map and a PNG are needed");
    }

    /// A mock map has no picture: `--map-picture` says so (exit 2) and writes nothing.
    #[test]
    fn a_map_without_a_picture_exports_nothing() {
        let (hub, c) = mock_hub("export");
        let f = &c.entries.iter().find(|e| e.kind == Kind::Map).unwrap().files[0];
        let map = hub.local.join("mock-hub").join(&f.path);
        let out = hub.local.join("picture.png");
        let args = ["--map-picture".to_string(), map.display().to_string(), out.display().to_string()];
        assert_eq!(export_cli(&args), Some(2));
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&hub.local);
    }

    #[test]
    fn a_preview_is_downloaded_once_into_the_cache() {
        let root = temp("preview-root");
        let local = temp("preview-local");
        std::fs::create_dir_all(root.join("previews")).unwrap();
        let png = mock::preview_png(320, 180);
        std::fs::write(root.join("previews/p.png"), &png).unwrap();
        let f = FileRef { path: "previews/p.png".into(), name: "p.png".into(), lang: None, size: png.len() as u64, sha256: crate::update::sha256_hex(&png), archive: false };
        let hub = Hub { source: Source::Dir(root.clone()), local: local.clone() };
        let path = hub.cache_preview(&f).unwrap();
        assert_eq!(path, preview_path(&local, &f));
        std::fs::remove_file(root.join("previews/p.png")).unwrap();
        assert_eq!(hub.cache_preview(&f).unwrap(), path, "the cached picture is used, not fetched again");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&local);
    }

    const URL: &str = "https://example.org/hub/latest/download";

    /// The cache reads back the whole catalog, optional fields and all, for the source it came from.
    #[test]
    fn the_catalog_cache_reads_back_for_its_source() {
        let (hub, mut c) = mock_hub("cache-rt");
        let local = hub.local.clone();
        // Fields the mock leaves out: a preview, a rating, an archive, a status note.
        let e = &mut c.entries[0];
        e.preview = Some(FileRef { path: "previews/x.png".into(), name: "x.png".into(), lang: None, size: 9, sha256: "ab".repeat(32), archive: false });
        e.rating = Some(Rating { up: 3, down: 1 });
        e.status_note = Text { ru: "Примечание".into(), en: "A note".into() };
        if let Some(m) = c.entries.iter_mut().find(|e| e.kind == Kind::Mod) {
            m.files[0].archive = true;
        }
        write_cache(&local, URL, &c).unwrap();
        assert!(cache_path_of(&local).is_file());
        // A cache of an address reads back made absolute, as the catalog was read (an older cache
        // may still hold relative paths).
        let mut absolute = c.clone();
        rebase(&mut absolute, URL);
        assert_eq!(read_cache(&local, URL), Some(absolute));
        let _ = std::fs::remove_dir_all(&local);
    }

    fn cache_path_of(local: &Path) -> PathBuf {
        local.join(CACHE_DIR).join(INDEX)
    }

    /// A cache is used only for the address it was fetched from: another address, or the same one
    /// with a trailing slash, finds nothing.
    #[test]
    fn a_cache_of_another_source_is_not_used() {
        let (hub, c) = mock_hub("cache-other");
        let local = hub.local.clone();
        write_cache(&local, URL, &c).unwrap();
        assert_eq!(read_cache(&local, "https://example.org/other/latest/download"), None);
        assert_eq!(read_cache(&local, &format!("{URL}/")), None, "the address as it was saved");
        let _ = std::fs::remove_dir_all(&local);
    }

    /// Whatever does not read back whole is ignored: a missing file, text that is no JSON, a
    /// catalog of another format, or a catalog with a path out of the hub.
    #[test]
    fn a_corrupt_cache_is_ignored() {
        let (hub, c) = mock_hub("cache-bad");
        let local = hub.local.clone();
        assert_eq!(read_cache(&local, URL), None, "no cache yet");
        std::fs::create_dir_all(local.join(CACHE_DIR)).unwrap();
        std::fs::write(cache_path_of(&local), b"{ not json").unwrap();
        assert_eq!(read_cache(&local, URL), None);
        let good = serde_json::to_string(&CacheWrite { source: URL, catalog: &c }).unwrap();
        std::fs::write(cache_path_of(&local), good.replacen("\"format\":1", "\"format\":7", 1)).unwrap();
        assert_eq!(read_cache(&local, URL), None, "a format this build does not know");
        let evil = good.replacen("\"id\":\"ford-outpost\"", "\"id\":\"../../x\"", 1);
        assert!(evil.contains("../../x"), "the test edits an id");
        std::fs::write(cache_path_of(&local), evil).unwrap();
        assert_eq!(read_cache(&local, URL), None, "an id that would leave the folder");
        std::fs::write(cache_path_of(&local), good).unwrap();
        assert!(read_cache(&local, URL).is_some(), "the good one reads");
        let _ = std::fs::remove_dir_all(&local);
    }

    /// Only an address has a cache: a folder or the mock is read in place.
    #[test]
    fn only_an_address_has_a_cache() {
        assert_eq!(Source::Url(URL.into()).cache_key(), Some(URL));
        assert_eq!(Source::Dir("/tmp/hub".into()).cache_key(), None);
        assert_eq!(Source::Mock.cache_key(), None);
        let (hub, _) = mock_hub("cache-mock");
        assert_eq!(hub.cached_catalog(), None);
        let _ = std::fs::remove_dir_all(&hub.local);
    }

    /// A catalog of format 1 with no entries, and the `game` object `game` (raw JSON).
    fn catalog_with_game(game: &str) -> String {
        format!(r#"{{"format":1,"entries":[],"game":{game}}}"#)
    }

    const GOOD_GAME: &str = r#"{"url":"https://example.org/discord-times-cu.zip","size":44619982,"sha256":"DFF554E22A0485F0ED06B3459D3E7E868224D49E264BBA0ECF40DB852514568D","name":"discord-times-cu.zip"}"#;

    #[test]
    fn a_catalog_without_a_game_parses_as_before() {
        let c = Catalog::parse(br#"{"format":1,"entries":[]}"#).unwrap();
        assert_eq!(c.game, None);
        let c = Catalog::parse(catalog_with_game("null").as_bytes()).unwrap();
        assert_eq!(c.game, None);
    }

    #[test]
    fn a_catalog_with_a_game_reads_it_and_lowers_its_sha256() {
        let c = Catalog::parse(catalog_with_game(GOOD_GAME).as_bytes()).unwrap();
        let g = c.game.expect("the game is read");
        assert_eq!(g.url, "https://example.org/discord-times-cu.zip");
        assert_eq!(g.size, 44_619_982);
        assert_eq!(g.sha256, "dff554e22a0485f0ed06b3459d3e7e868224d49e264bba0ecf40db852514568d");
        assert_eq!(g.name, "discord-times-cu.zip");
        // A path inside the hub is an address too (relative to the catalog).
        let rel = GOOD_GAME.replace("https://example.org/discord-times-cu.zip", "game/discord-times-cu.zip");
        assert!(Catalog::parse(catalog_with_game(&rel).as_bytes()).is_ok());
    }

    #[test]
    fn a_game_with_a_bad_field_refuses_the_catalog() {
        let bad = [
            GOOD_GAME.replace("\"name\":\"discord-times-cu.zip\"", "\"name\":\"../x.zip\""),
            GOOD_GAME.replace("\"name\":\"discord-times-cu.zip\"", "\"name\":\"\""),
            GOOD_GAME.replace("https://example.org/discord-times-cu.zip", "http://example.org/discord-times-cu.zip"),
            GOOD_GAME.replace("https://example.org/discord-times-cu.zip", "https://exa mple.org/x.zip"),
            GOOD_GAME.replace("https://example.org/discord-times-cu.zip", "../../etc/x.zip"),
            GOOD_GAME.replace("DFF554E22A0485F0ED06B3459D3E7E868224D49E264BBA0ECF40DB852514568D", "abc"),
            GOOD_GAME.replace("44619982", "0"),
            GOOD_GAME.replace("44619982", &(MAX_ARCHIVE_DOWNLOAD + 1).to_string()),
        ];
        for game in bad {
            assert!(Catalog::parse(catalog_with_game(&game).as_bytes()).is_err(), "{game}");
        }
    }

    #[test]
    fn a_game_mirror_is_read_when_https_and_a_bad_one_is_dropped_alone() {
        let with = GOOD_GAME.replace(r#""size""#, r#""mirrors":["https://gitverse.example/game.zip","http://plain.example/game.zip","https://exa mple.org/x.zip","../rel.zip"],"size""#);
        let g = Catalog::parse(catalog_with_game(&with).as_bytes()).unwrap().game.unwrap();
        assert_eq!(g.mirrors, vec!["https://gitverse.example/game.zip".to_string()], "the bad ones are dropped, the catalog is kept");
        assert_eq!(g.url, "https://example.org/discord-times-cu.zip");
        let none = Catalog::parse(catalog_with_game(GOOD_GAME).as_bytes()).unwrap().game.unwrap();
        assert!(none.mirrors.is_empty(), "absent: none");
    }

    #[test]
    fn an_older_catalog_reads_without_mirrors_and_a_newer_one_with_fields_he_does_not_know() {
        // An old client's catalog (no mirrors, no unknown fields) reads as it always did.
        let old = catalog_with_game(GOOD_GAME);
        assert!(Catalog::parse(old.as_bytes()).unwrap().game.unwrap().mirrors.is_empty());
        // A newer catalog: a field this build does not know, at the top, in an entry and in the game.
        let newer = format!(
            r#"{{"format":1,"future":{{"x":1}},"entries":[{{"id":"m","kind":"mod","title":{{"ru":"x"}},"author":"a","version":"1","files":[],"tags":["new"]}}],"game":{}}}"#,
            GOOD_GAME.replace(r#""size""#, r#""sizes_by_host":{"a":1},"size""#)
        );
        let c = Catalog::parse(newer.as_bytes()).expect("unknown fields are not refused");
        assert_eq!(c.entries.len(), 1);
        assert_eq!(c.game.unwrap().size, 44_619_982);
    }

    /// The default hub's catalog, as the mirror gives it: the GitVerse one names its files by
    /// address, the GitHub one by a relative path (its files are on the same release).
    #[test]
    fn the_default_catalog_comes_from_the_first_mirror_that_reads_and_its_files_stay_with_it() {
        let gitverse_html = b"<!doctype html><html><body>Page not found</body></html>".to_vec();
        let github_json = format!(
            r#"{{"format":1,"entries":[{{"id":"m","kind":"mod","title":{{"ru":"x"}},"author":"a","version":"1","files":[{{"path":"mods/m/a.ini","name":"a.ini","size":1,"sha256":"{s}"}}],"preview":{{"path":"previews/p.png","name":"p.png","size":1,"sha256":"{s}"}}}}],"game":{{"url":"game/discord-times-cu.zip","mirrors":["https://gitverse.ru/api/attachments/x"],"size":44619982,"sha256":"{s}","name":"discord-times-cu.zip"}}}}"#,
            s = "ab".repeat(32)
        );
        let mut asked = Vec::new();
        let c = catalog_from_mirrors(&mirror::HUB, |url| {
            asked.push(url.to_string());
            if url.starts_with("https://indicozy.gitverse") { Ok(gitverse_html.clone()) } else { Ok(github_json.clone().into_bytes()) }
        })
        .unwrap();
        assert_eq!(asked, vec![format!("{}/index.json", mirror::HUB[0]), format!("{}/index.json", mirror::HUB[1])]);
        let e = &c.entries[0];
        assert_eq!(e.files[0].path, format!("{DEFAULT_SOURCE}/mods/m/a.ini"), "resolved against the GitHub folder that gave it");
        assert_eq!(e.preview.as_ref().unwrap().path, format!("{DEFAULT_SOURCE}/previews/p.png"));
        assert_eq!(c.game.as_ref().unwrap().url, format!("{DEFAULT_SOURCE}/game/discord-times-cu.zip"));
        assert_eq!(c.game.as_ref().unwrap().mirrors, vec!["https://gitverse.ru/api/attachments/x".to_string()]);
    }

    #[test]
    fn a_gitverse_catalog_keeps_its_own_absolute_addresses() {
        let body = format!(r#"{{"format":1,"entries":[],"ad":{{"id":"a","version":1,"image":"https://example.org/capsule.png","size":1,"sha256":"{s}","url":"https://example.org/","caption_ru":"","caption_en":""}}}}"#, s = "cd".repeat(32));
        let c = catalog_from_mirrors(&["https://indicozy.gitverse.site/razdor-community"], |_| Ok(body.clone().into_bytes())).unwrap();
        assert_eq!(c.ad.unwrap().image, "https://example.org/capsule.png");
    }

    #[test]
    fn when_no_mirror_reads_the_last_reason_is_the_error() {
        let asked = std::cell::Cell::new(0usize);
        let err = catalog_from_mirrors(&mirror::HUB, |url| {
            asked.set(asked.get() + 1);
            if url.contains("gitverse") { Err("stalled".into()) } else { Ok(b"<html>nope</html>".to_vec()) }
        })
        .unwrap_err();
        assert_eq!(asked.get(), 2, "both mirrors are tried");
        assert!(err.starts_with("index.json:"), "the last one is a web page, not a catalog: {err}");
    }

    const GOOD_AD: &str = r#"{"id":"thorny-times","version":1,"image":"https://example.org/capsule.png","size":135584,"sha256":"3AA411F3AB65A986DD7FDE5ECFBBCB0D399458306678421D4DEFC6B899C550CA","url":"https://store.steampowered.com/app/2182610/","caption_ru":"Новая игра","caption_en":"A new game"}"#;

    fn catalog_with_ad(ad: &str) -> String {
        format!(r#"{{"format":1,"entries":[],"ad":{ad}}}"#)
    }

    #[test]
    fn a_catalog_with_an_ad_reads_it_and_one_without_does_not() {
        assert_eq!(Catalog::parse(br#"{"format":1,"entries":[]}"#).unwrap().ad, None);
        assert_eq!(Catalog::parse(catalog_with_ad("null").as_bytes()).unwrap().ad, None);
        let a = Catalog::parse(catalog_with_ad(GOOD_AD).as_bytes()).unwrap().ad.expect("the ad is read");
        assert_eq!((a.id.as_str(), a.version, a.size), ("thorny-times", 1, 135_584));
        assert_eq!(a.sha256, "3aa411f3ab65a986dd7fde5ecfbbcb0d399458306678421d4defc6b899c550ca");
        assert_eq!(a.url, "https://store.steampowered.com/app/2182610/");
        assert_eq!(a.caption_en, "A new game");
    }

    #[test]
    fn a_bad_ad_refuses_the_catalog() {
        let bad = [
            GOOD_AD.replace("\"thorny-times\"", "\"../x\""),
            GOOD_AD.replace("https://example.org/capsule.png", "http://example.org/capsule.png"),
            GOOD_AD.replace("https://example.org/capsule.png", "https://exa mple.org/capsule.png"),
            GOOD_AD.replace("3AA411F3AB65A986DD7FDE5ECFBBCB0D399458306678421D4DEFC6B899C550CA", "abc"),
            GOOD_AD.replace("\"size\":135584", "\"size\":0"),
            GOOD_AD.replace("\"size\":135584", &format!("\"size\":{}", MAX_AD_BYTES + 1)),
            GOOD_AD.replace("https://store.steampowered.com/app/2182610/", "ftp://store.example.org/x"),
            GOOD_AD.replace("\"caption_en\":\"A new game\"", "\"caption_en\":\"two\\nlines\""),
            GOOD_AD.replace("\"caption_ru\":\"Новая игра\"", &format!("\"caption_ru\":\"{}\"", "я".repeat(120))),
        ];
        for ad in bad {
            assert!(Catalog::parse(catalog_with_ad(&ad).as_bytes()).is_err(), "{ad}");
        }
    }

    /// The Thorny Times ad as a test's catalog entry, its picture the given bytes.
    fn sample_ad(picture: &[u8]) -> AdRef {
        AdRef {
            id: "thorny-times".into(),
            version: 1,
            image: "capsule.png".into(),
            size: picture.len() as u64,
            sha256: crate::update::sha256_hex(picture),
            url: "https://store.steampowered.com/app/2182610/".into(),
            caption_ru: "Новая игра".into(),
            caption_en: "A new game".into(),
        }
    }

    #[test]
    fn the_ad_picture_keeps_its_extension_and_defaults_to_png() {
        let mut a = sample_ad(b"x");
        assert_eq!(ad_picture_name(&a), "picture.png");
        a.image = "https://example.org/a/Capsule.JPG".into();
        assert_eq!(ad_picture_name(&a), "picture.jpg");
        a.image = "https://example.org/capsule".into();
        assert_eq!(ad_picture_name(&a), "picture.png");
        a.image = "https://example.org/a.b/c.we-ird".into();
        assert_eq!(ad_picture_name(&a), "picture.png", "an extension that is not a plain word is none");
    }

    #[test]
    fn an_ad_is_installed_once_and_replaced_when_it_changes() {
        let root = std::env::temp_dir().join(format!("razdor-ad-install-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let src = root.join("hub");
        fs::create_dir_all(&src).unwrap();
        let picture = b"a small picture".to_vec();
        fs::write(src.join("capsule.png"), &picture).unwrap();
        let hub = Hub { source: Source::Dir(src), local: root.join("local") };
        let ads = root.join("ads");
        // Another ad's folder is the catalog copy's no more: it goes.
        fs::create_dir_all(ads.join("old-ad")).unwrap();

        let mut a = sample_ad(&picture);
        assert_eq!(hub.install_ad(&a, &ads), Ok(true));
        assert!(!ads.join("old-ad").exists(), "the other folders are removed");
        assert_eq!(fs::read(ads.join("thorny-times").join("picture.png")).unwrap(), picture);
        assert_eq!(fs::read_to_string(ads.join("thorny-times").join(AD_INI)).unwrap(), ad_ini_text(&a, "picture.png"));
        assert_eq!(hub.install_ad(&a, &ads), Ok(false), "the same ad is not written again");

        a.caption_en = "TEST".into();
        assert_eq!(hub.install_ad(&a, &ads), Ok(true), "a new caption is a new copy");
        assert!(fs::read_to_string(ads.join("thorny-times").join(AD_INI)).unwrap().contains("caption_en=TEST"));

        // A picture that is not the catalog's is refused, and the copy on disk stays as it was.
        let mut bad = sample_ad(b"another picture");
        bad.version = 2;
        bad.sha256 = "0".repeat(64);
        assert!(hub.install_ad(&bad, &ads).is_err());
        assert!(fs::read_to_string(ads.join("thorny-times").join(AD_INI)).unwrap().contains("caption_en=TEST"));
        assert_eq!(fs::read(ads.join("thorny-times").join("picture.png")).unwrap(), picture);
        let _ = fs::remove_dir_all(&root);
    }
}
