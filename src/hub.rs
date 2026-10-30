//! The map hub (Razdor's own; a mock for now): a catalog of community maps, campaigns and
//! mods that the player browses and installs from inside the game (`ui::hub_view`).
//!
//! - **Catalog**: an `index.json` ([`Catalog`]) next to the files it names. Each entry is a
//!   map, a campaign (its chapters in order, linked by their next-map names) or a mod, with
//!   its titles and descriptions in Russian and English, the languages its maps come in
//!   (Russian as written, English from the import's translation) and the mods it needs.
//! - **Source** ([`Source`]): `RAZDOR_HUB` names an `https://` address or a folder; unset, a
//!   mock hub is written once under the hub folder ([`mock::build`]) and read from there.
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

pub use mods::*;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;

/// Environment variable naming the hub: an `https://` address or a folder.
pub const SOURCE_ENV: &str = "RAZDOR_HUB";
/// Environment variable naming the local hub folder.
pub const DIR_ENV: &str = "RAZDOR_HUB_DIR";
/// The catalog's file name at the hub's root.
pub const INDEX: &str = "index.json";
/// The default hub: the latest release of the community repository (its assets are the
/// catalog and the files it names).
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
    /// For a mod: the base install it is made for (e.g. "Community Update 1.2"). Nothing checks it.
    #[serde(default)]
    pub base: String,
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

/// The hub's `index.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub format: u32,
    #[serde(default)]
    pub entries: Vec<Entry>,
}

impl Catalog {
    /// Reads a catalog. Refused whole when any string that becomes part of a path is not safe:
    /// an id, a file name, a path, a SHA-256. The SHA-256s are stored lower-cased.
    pub fn parse(bytes: &[u8]) -> Result<Catalog, String> {
        let mut c: Catalog = serde_json::from_slice(bytes).map_err(|e| format!("index.json: {e}"))?;
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
            if let Some(p) = &mut e.preview {
                if !safe_location(&p.path) {
                    return Err(format!("index.json: {}: bad preview path", e.id));
                }
                if !safe_sha256(&p.sha256) {
                    return Err(format!("index.json: {}: the preview's SHA-256 is not 64 hex digits", e.id));
                }
                p.sha256 = p.sha256.to_ascii_lowercase();
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
    !name.is_empty() && !name.starts_with('.') && name.chars().all(|c| c.is_alphanumeric() || "-_. ".contains(c))
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
    Url(String),
    Dir(PathBuf),
    /// The mock hub, written under the hub folder on first use.
    Mock,
}

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
            None => Source::Url(DEFAULT_SOURCE.to_string()),
        }
    }

    /// Short words for the window's corner.
    pub fn label(&self) -> String {
        match self {
            Source::Url(u) => u.clone(),
            Source::Dir(d) => d.display().to_string(),
            Source::Mock => "mock".into(),
        }
    }
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

    pub fn catalog(&self) -> Result<Catalog, String> {
        Catalog::parse(&self.fetch(INDEX)?)
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
            // All files are fetched and checked before any is written.
            let mut got: Vec<(&FileRef, Got)> = Vec::new();
            for (n, &f) in files.iter().enumerate() {
                if f.archive {
                    std::fs::create_dir_all(downloads).map_err(|err| format!("downloads: {err}"))?;
                    let part = downloads.join(format!("{}-{n}.zip", e.id));
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
            let record = InstalledEntry { version: e.version.clone(), kind: e.kind, lang, title: e.title.clone(), requires: e.requires.clone(), files: written };
            installed.entries.insert(e.id.clone(), record);
            self.save_installed(&installed)?;
        }
        Ok(())
    }

    /// Writes a mod's files: the files it ships and its archives' entries, built in
    /// `files.tmp/` and put in place of the old `files/` in one rename (a reinstall replaces
    /// them all, and a file the new version dropped goes too). Returns the records' paths.
    fn write_mod(&self, id: &str, got: &[(&FileRef, Got)]) -> Result<Vec<String>, String> {
        let tmp = mod_dir(&self.local, id).join("files.tmp");
        let _ = std::fs::remove_dir_all(&tmp);
        let built = self.build_mod_files(&tmp, got);
        let names = match built {
            Ok(names) => names,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(e);
            }
        };
        let files = mod_files_dir(&self.local, id);
        let _ = std::fs::remove_dir_all(&files);
        std::fs::rename(&tmp, &files).map_err(|e| format!("{}: {e}", files.display()))?;
        Ok(names.into_iter().map(|n| format!("mods/{id}/files/{n}")).collect())
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
                languages: langs.iter().map(|l| l.to_string()).collect(),
                machine_translated: langs.contains(&"en"),
                requires: requires.iter().map(|r| r.to_string()).collect(),
                source_url: String::new(),
                added: added.into(),
                status,
                status_note: Text { ru: note.0.into(), en: note.1.into() },
                preview: None,
                base: String::new(),
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
                languages: vec!["ru".into()],
                machine_translated: false,
                requires: requires.iter().map(|r| r.to_string()).collect(),
                source_url: String::new(),
                added: added.into(),
                status,
                status_note: Text { ru: note.0.into(), en: note.1.into() },
                preview: None,
                base: "Community Update 1.2".into(),
                files: vec![f],
            });
            Ok(())
        }
    }

    /// A PNG of `width` x `height` (a gradient), made here: stored deflate blocks, no compression.
    pub fn preview_png(width: u32, height: u32) -> Vec<u8> {
        let mut raw = Vec::new();
        for y in 0..height {
            raw.push(0);
            for x in 0..width {
                raw.extend_from_slice(&[(x * 255 / width.max(1)) as u8, (y * 255 / height.max(1)) as u8, 120]);
            }
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
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
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
        let catalog = Catalog { format: 1, entries: o.entries };
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
        some.entries.insert("evolution-mock".into(), InstalledEntry { version: "9.0".into(), kind: Kind::Mod, lang: String::new(), title: Text::default(), requires: vec![], files: vec![] });
        assert_eq!(plan_install(&c, &some, "merc-road").unwrap(), ["evolution-mercs", "merc-road"]);
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
    fn the_default_source_is_the_release_and_the_mock_only_when_asked() {
        assert_eq!(Source::from_setting(None, false), Source::Url(DEFAULT_SOURCE.to_string()));
        assert_eq!(Source::from_setting(Some("  "), false), Source::Url(DEFAULT_SOURCE.to_string()));
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
}
