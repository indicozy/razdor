//! Installed mods as layers over the base install. A mod's files live under
//! `<hub>/mods/<id>/files/` at their paths inside the mod. [`prepare_mod`] builds the mod's
//! merged game folder `<hub>/mods/<id>/game/`: every base folder, every base file hard-linked
//! where the file system allows it (never copied: where links fail, as on Android's app storage,
//! the file is left out and every lookup that misses in the merged folder reads the base,
//! `dt::install::overlay_fallback`), then each mod file laid over the base file it replaces. The game reads the active
//! mod's folder ([`active_install_dir`]); the choice is `<hub>/active.json`.
//!
//! The base install is never written: a link is removed before a file replaces it. A folder
//! is rebuilt when the base's files (names, sizes, times), the mod's version or its file list
//! and contents change (`stamp.json` in the folder).
//!
//! Pictures (`.ugs`, `.lit`) are kept compressed (`<name>.lz4`, `dt::stored`; backlog 1609 part 2,
//! step 3, `docs/specs/appdata-compression.md`): a mod's files are compressed after an unpack
//! ([`compress_pictures`], in `Hub::write_mod`) and, for an installed mod, at its next play
//! ([`prepare_mod`]). The manifest and the stamp name each file by its logical path (`Pictures/A.lit`,
//! the zip's own path) and its digest is of the decoded bytes, so the file is valid in either form.
//! The merged folder lays the stored file (`Pictures/A.lit.lz4`) and removes every other stored form
//! of the same name in it ([`drop_other_stored_form`]). The zips a mod was unpacked from are not kept.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{read_installed, safe_id, safe_rel_name, FileRef, Installed, Kind, Text};
use crate::dt::stored::{self, Stored};

/// The file naming the active mod, under the hub folder.
const ACTIVE: &str = "active.json";
/// The file recording how a merged game folder was built, inside it.
const STAMP: &str = "stamp.json";
/// The mod's manifest, beside its `files/` (`mods/<id>/manifest.json`).
const MANIFEST: &str = "manifest.json";
/// The empty file a mod's folder gets each time the mod is played; its modification time is the
/// record (`mods/<id>/last_played`).
const LAST_PLAYED: &str = "last_played";
/// The folder the mod zips of older installs were kept in (`mods/<id>/archives/<n>.zip`). Not written
/// any more; [`prepare_mod`] removes it.
const ARCHIVES: &str = "archives";

/// The mod's folder: `<hub>/mods/<id>/`, with `files/` (what was installed) and `game/` (the
/// merged folder the game plays from).
pub fn mod_dir(local: &Path, id: &str) -> PathBuf {
    local.join("mods").join(id)
}

/// The mod's installed files, as the hub wrote them (`<hub>/mods/<id>/files/`).
pub fn mod_files_dir(local: &Path, id: &str) -> PathBuf {
    mod_dir(local, id).join("files")
}

/// The mod's merged game folder (`<hub>/mods/<id>/game/`), built by [`prepare_mod`].
pub fn mod_game_dir(local: &Path, id: &str) -> PathBuf {
    mod_dir(local, id).join("game")
}

/// What a mod's files should be, written at each install (`mods/<id>/manifest.json`, beside
/// `files/`): every file's path, size and SHA-256, and the catalog files they came from, so a
/// damaged file can be restored (`Hub::repair_mod`). A mod installed before manifests has none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub files: Vec<ManifestFile>,
    pub sources: Vec<FileRef>,
}

/// One file of a mod as installed: its path in the mod, size and SHA-256.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// The manifest of mod `id`; None when it has none or it does not read.
pub fn read_manifest(local: &Path, id: &str) -> Option<Manifest> {
    serde_json::from_slice(&std::fs::read(mod_dir(local, id).join(MANIFEST)).ok()?).ok()
}

/// Writes mod `id`'s manifest.
pub fn write_manifest(local: &Path, id: &str, manifest: &Manifest) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?;
    crate::editor::files::write_atomically(&mod_dir(local, id).join(MANIFEST), &json).map_err(|e| format!("{MANIFEST}: {e}"))
}

/// The files under `files_dir` as a manifest lists them: the logical path, size and SHA-256 of the
/// decoded bytes of each (a compressed picture is listed as its plain name, see [`mod_files`]).
pub fn file_entries(files_dir: &Path) -> Result<Vec<ManifestFile>, String> {
    mod_files(files_dir)?
        .into_iter()
        .map(|f| {
            let stored = f.stored();
            let size = stored.logical_len().map_err(|e| format!("{}: {e}", f.path))?;
            let sha256 = if f.compressed {
                crate::update::sha256_hex(&stored.read().map_err(|e| format!("{}: {e}", f.path))?)
            } else {
                crate::update::sha256_file(&f.file).map_err(|e| format!("{}: {e}", f.path))?
            };
            Ok(ManifestFile { path: f.path, size, sha256 })
        })
        .collect()
}

/// The files of mod `id` that are missing or differ from its manifest (their paths in the mod).
/// A mod with no manifest (installed before manifests) has only its unreadable files reported:
/// its files are trusted until a repair can tell better. An error when a file name is not one a
/// mod may use.
pub fn damaged_files(local: &Path, id: &str) -> Result<Vec<String>, String> {
    let dir = mod_files_dir(local, id);
    let Some(manifest) = read_manifest(local, id) else {
        let mut bad = Vec::new();
        for f in mod_files(&dir)? {
            let readable = if f.compressed { f.stored().read().is_ok() } else { std::fs::File::open(&f.file).and_then(|mut file| std::io::copy(&mut file, &mut std::io::sink())).is_ok() };
            if !readable {
                bad.push(f.path);
            }
        }
        return Ok(bad);
    };
    Ok(manifest.files.iter().filter(|f| !matches_entry(&dir, f)).map(|f| f.path.clone()).collect())
}

/// Whether the file `f` names under `dir` is there with its size and SHA-256 of the decoded bytes,
/// in whichever stored form it is (`stored::find`: a plain file, or its `.lz4`).
fn matches_entry(dir: &Path, f: &ManifestFile) -> bool {
    if !safe_rel_name(&f.path) {
        return false;
    }
    let (folder, name) = match f.path.rsplit_once('/') {
        Some((folder, name)) => (dir.join(folder), name),
        None => (dir.to_path_buf(), f.path.as_str()),
    };
    let Some(stored) = stored::find(&folder, name).and_then(|s| s.present()) else { return false };
    if stored.logical_len().ok() != Some(f.size) {
        return false;
    }
    let sum = if stored.compressed { stored.read().ok().map(|bytes| crate::update::sha256_hex(&bytes)) } else { crate::update::sha256_file(&stored.path).ok() };
    sum.is_some_and(|sum| sum == f.sha256.to_lowercase())
}

/// The logical path of a file of a mod's `files/` as it is stored: `Pictures/A.lit.lz4` is
/// `Pictures/A.lit`; any other path is itself.
fn logical_rel(rel: &str) -> String {
    let (dir, name) = match rel.rfind('/') {
        Some(at) => (&rel[..=at], &rel[at + 1..]),
        None => ("", rel),
    };
    match stored::logical_name_of(name) {
        Some(logical) => format!("{dir}{logical}"),
        None => rel.to_string(),
    }
}

/// Whether the file path `rel` of a mod is a stored (compressed) form: its name is `<logical>.lz4`.
fn is_stored_rel(rel: &str) -> bool {
    stored::logical_name_of(rel.rsplit('/').next().unwrap_or(rel)).is_some()
}

/// A file of a mod as it is laid: its logical path in the mod (the manifest's), its stored path
/// (`…lz4` when compressed) and the file.
struct ModFile {
    path: String,
    rel: String,
    file: PathBuf,
    compressed: bool,
}

impl ModFile {
    fn stored(&self) -> Stored {
        Stored { path: self.file.clone(), compressed: self.compressed }
    }
}

/// The files of a mod's `files/` folder, by logical path, sorted. A compressed picture is listed
/// once, under its logical name; a plain file wins over a stored form of the same name (what an
/// interrupted conversion leaves). Names that differ only in case are all listed (a layer refuses
/// them, [`build`]).
fn mod_files(files_dir: &Path) -> Result<Vec<ModFile>, String> {
    let listed = list_mod_files(files_dir)?;
    let plain: HashSet<String> = listed.iter().filter(|(rel, _)| !is_stored_rel(rel)).map(|(rel, _)| rel.to_ascii_lowercase()).collect();
    let mut out = Vec::with_capacity(listed.len());
    for (rel, file) in listed {
        let compressed = is_stored_rel(&rel);
        let path = logical_rel(&rel);
        if compressed && plain.contains(&path.to_ascii_lowercase()) {
            continue;
        }
        out.push(ModFile { path, rel, file, compressed });
    }
    Ok(out)
}

/// Compresses the plain pictures in a mod's `files/` folder (`dt::stored::compress_folder`). A
/// failure is logged and the plain files stay: they read the same and the manifest names them the
/// same way, so nothing else is needed.
pub fn compress_pictures(files_dir: &Path) {
    if !files_dir.is_dir() {
        return;
    }
    if let Err(e) = stored::compress_folder(files_dir) {
        crate::diag!("hub: {}: the pictures are not compressed: {e}", files_dir.display());
    }
}

#[derive(Serialize, Deserialize)]
struct Active {
    #[serde(rename = "mod")]
    id: String,
}

/// The mod the game plays, or none (no file, or one that does not read).
pub fn active_mod(local: &Path) -> Option<String> {
    let bytes = std::fs::read(local.join(ACTIVE)).ok()?;
    let a: Active = serde_json::from_slice(&bytes).ok()?;
    safe_id(&a.id).then_some(a.id)
}

/// The installed mods, by id, with their titles (for the new-game window's picker).
pub fn installed_mods(local: &Path) -> Vec<(String, Text)> {
    read_installed(local).entries.iter().filter(|(_, e)| e.kind == Kind::Mod).map(|(id, e)| (id.clone(), e.title.clone())).collect()
}

/// The bytes of file `name` (a name at the install's top, matched without case) as the game reads
/// them with the layers `chain` over `base`: the topmost layer that has it (its copy in the mod's
/// files folder), else the base install's. None when none has it. Builds no folder and copies
/// nothing.
pub fn layer_bytes(local: &Path, base: &Path, chain: &[String], name: &str) -> Option<Vec<u8>> {
    for lid in chain.iter().rev() {
        if let Some(file) = top_file(&mod_files_dir(local, lid), name) {
            return file.read().ok();
        }
    }
    top_file(base, name).and_then(|file| file.read().ok())
}

/// The file `name` at the top of `dir`, matched without case, in its stored form (a plain file or
/// `<name>.lz4`, `stored::find`); its decoded bytes are what the game reads.
fn top_file(dir: &Path, name: &str) -> Option<Stored> {
    stored::find(dir, name).and_then(|s| s.present())
}

/// Makes `id` the mod the game plays from now on (`None`: the base game), after its folder is
/// built. Returns the folder the game reads.
pub fn use_mod(local: &Path, base: &Path, id: Option<&str>) -> Result<PathBuf, String> {
    match id {
        None => {
            write_active(local, None)?;
            Ok(base.to_path_buf())
        }
        Some(id) => {
            // Tried once more after a base conversion, as `active_install_dir_in` does.
            let dir = prepare_mod(local, base, id).or_else(|_| {
                stored::wait_converted(base);
                prepare_mod(local, base, id)
            })?;
            write_active(local, Some(id))?;
            mark_played(local, id);
            Ok(dir)
        }
    }
}

/// Records that mod `id` was just played: its `last_played` marker's modification time orders the
/// built folders kept (see [`keep_recent_game_folders`]). A failed write is ignored: it only
/// decides which folder goes.
fn mark_played(local: &Path, id: &str) {
    let _ = std::fs::write(mod_dir(local, id).join(LAST_PLAYED), b"");
}

fn write_active(local: &Path, id: Option<&str>) -> Result<(), String> {
    match id {
        None => match std::fs::remove_file(local.join(ACTIVE)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{ACTIVE}: {e}")),
        },
        Some(id) => {
            let json = serde_json::to_vec_pretty(&Active { id: id.into() }).map_err(|e| e.to_string())?;
            crate::editor::files::write_atomically(&local.join(ACTIVE), &json).map_err(|e| format!("{ACTIVE}: {e}"))
        }
    }
}

/// The folder the game reads its install from: the active mod's merged folder when the mod is
/// still installed and its folder builds, else `base`. A stale choice is cleared.
pub fn active_install_dir(base: &Path) -> PathBuf {
    match super::local_dir() {
        Some(local) => active_install_dir_in(&local, base),
        None => base.to_path_buf(),
    }
}

/// Whether mod `id` is installed as a mod (the record `installed.json`).
fn is_installed_mod(local: &Path, id: &str) -> bool {
    read_installed(local).entries.get(id).is_some_and(|e| e.kind == Kind::Mod)
}

/// Whether the game folder `base` is converting now and the install's load waits for it: a mod is active
/// and installed, so [`active_install_dir`] builds it, and `prepare_mod` waits for the conversion first.
/// The start shows its line while this holds (`ui::prep_line`); nothing is built here.
pub fn install_waits_for_conversion(local: &Path, base: &Path) -> bool {
    active_mod(local).is_some_and(|id| is_installed_mod(local, &id)) && stored::is_converting(base)
}

fn active_install_dir_in(local: &Path, base: &Path) -> PathBuf {
    let Some(id) = active_mod(local) else { return base.to_path_buf() };
    if !is_installed_mod(local, &id) {
        crate::diag!("hub: the active mod {id} is not installed; the base game is played");
        let _ = write_active(local, None);
        return base.to_path_buf();
    }
    // A build that fails is tried once more after the base's conversion (if one runs): a mod is not
    // dropped for a file that moved meanwhile.
    let built = prepare_mod(local, base, &id).or_else(|e| {
        crate::diag!("hub: the active mod {id} did not build ({e}); once more after the conversion");
        stored::wait_converted(base);
        prepare_mod(local, base, &id)
    });
    match built {
        Ok(dir) => dir,
        Err(e) => {
            crate::diag!("hub: the active mod {id} does not build ({e}); the base game is played");
            let _ = write_active(local, None);
            base.to_path_buf()
        }
    }
}

/// Where a hub map is shown in the new-game list (the rule of 1445, 1445b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapGroup {
    /// In the list's main part: with no mod played, the base game's maps (a hub map that needs
    /// no mod); with a mod played, the maps that need it or a mod it is laid over.
    Chosen,
    /// With a mod played, a map made for the base game (it needs no mod), or a base install map
    /// the mods leave unchanged (1445c): under the list's «Карты обычной игры» group, collapsed by
    /// default (1445b).
    Base,
    /// Not in the list.
    Hidden,
}

/// Where an install map of the played folder comes from (1445c): the base install's own map,
/// unchanged, or a map a mod adds or replaces (with other bytes than the base's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapOrigin {
    Base,
    Mod,
}

/// Where an install map of the played folder is shown (1445c). With no mod played (`chain`
/// empty) every install map is in the list's main part. With a mod played, a base map the mods
/// leave unchanged is under the base game's group ([`MapGroup::Base`]); a map the mods added or
/// changed is the played mod's ([`MapGroup::Chosen`]).
pub fn install_map_group(origin: MapOrigin, chain: &[String]) -> MapGroup {
    match origin {
        MapOrigin::Base if !chain.is_empty() => MapGroup::Base,
        _ => MapGroup::Chosen,
    }
}

/// The base install's maps, for [`BaseMaps::origin`]: each file's lower-cased name, size and path.
pub struct BaseMaps {
    files: Vec<(String, u64, PathBuf)>,
}

impl BaseMaps {
    /// The base's map files (those that do not read are left out).
    pub fn new(paths: &[PathBuf]) -> BaseMaps {
        BaseMaps { files: paths.iter().filter_map(|p| Some((file_key(p)?, std::fs::metadata(p).ok()?.len(), p.clone()))).collect() }
    }

    /// Where the install map `path` (of the played folder) comes from: [`MapOrigin::Base`] when the
    /// base has a map of the same file name (ignoring case) with the same bytes, else
    /// [`MapOrigin::Mod`], a map a mod added or replaced.
    pub fn origin(&self, path: &Path) -> MapOrigin {
        let (Some(key), Ok(size)) = (file_key(path), std::fs::metadata(path).map(|m| m.len())) else { return MapOrigin::Mod };
        let same = self.files.iter().filter(|(k, s, _)| *k == key && *s == size).any(|(_, _, p)| same_bytes(p, path));
        if same {
            MapOrigin::Base
        } else {
            MapOrigin::Mod
        }
    }
}

/// A map's file name, lower-cased (the overlay ignores case).
fn file_key(path: &Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase())
}

/// Where a hub map is shown (see [`MapGroup`]). `chain` is the played mod and the mods it is laid
/// over ([`layer_chain`]), empty with no mod played. A map the install already has (the same
/// bytes, `same_as_install`) is shown once, from the install, so it is [`MapGroup::Hidden`] here.
pub fn map_group(requires: &[String], chain: &[String], same_as_install: bool) -> MapGroup {
    if same_as_install {
        return MapGroup::Hidden;
    }
    if chain.is_empty() {
        return if requires.is_empty() { MapGroup::Chosen } else { MapGroup::Hidden };
    }
    if requires.iter().any(|r| chain.contains(r)) {
        MapGroup::Chosen
    } else if requires.is_empty() {
        MapGroup::Base
    } else {
        MapGroup::Hidden
    }
}

/// The maps of the install, for the duplicate check of [`map_group`]: each file's size, read
/// once. A hub map is the install's when an install map has its size and the same bytes.
pub struct InstallMaps {
    files: Vec<(u64, PathBuf)>,
}

impl InstallMaps {
    /// The install's map files (those that do not read are left out).
    pub fn new(paths: &[PathBuf]) -> InstallMaps {
        InstallMaps { files: paths.iter().filter_map(|p| std::fs::metadata(p).ok().map(|m| (m.len(), p.clone()))).collect() }
    }

    /// Whether an install map has the same bytes as the file `path` (the sizes first).
    pub fn has_same(&self, path: &Path) -> bool {
        let Ok(size) = std::fs::metadata(path).map(|m| m.len()) else { return false };
        self.files.iter().filter(|(s, _)| *s == size).any(|(_, p)| same_bytes(p, path))
    }
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

/// The maps of the hub's folder the new-game list shows, by [`map_group`]: the mod played and the
/// mods it is laid over (`chain`), and the entries' `requires` by file name.
pub struct MapGate {
    /// Lower-cased file name → the mods its entry needs.
    needs: BTreeMap<String, Vec<String>>,
    /// The mod played and the mods it is laid over (empty: the base game).
    chain: Vec<String>,
}

impl MapGate {
    /// Reads the gate from the hub folder `local`. A played mod whose chain does not read (a
    /// mod gone) is treated as the base game, as [`active_install_dir`] plays it.
    pub fn read(local: &Path) -> MapGate {
        let installed = read_installed(local);
        let mut needs: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for e in installed.entries.values().filter(|e| e.kind != Kind::Mod && !e.requires.is_empty()) {
            for f in &e.files {
                if let Some(name) = f.strip_prefix("maps/") {
                    needs.entry(stem_key(name)).or_default().extend(e.requires.iter().cloned());
                }
            }
        }
        let chain = active_mod(local).and_then(|id| layer_chain(&installed, &id).ok()).unwrap_or_default();
        MapGate { needs, chain }
    }

    /// Where the map `name` (its file name, or the stem the new-game list gives) is shown, given
    /// whether it is the install's (`same_as_install`, see [`InstallMaps::has_same`]).
    pub fn group(&self, name: &str, same_as_install: bool) -> MapGroup {
        let requires = self.needs.get(&stem_key(name)).map_or(&[][..], Vec::as_slice);
        map_group(requires, &self.chain, same_as_install)
    }

    /// Whether a mod is played (its chain reads), so the install's maps are told apart by
    /// [`BaseMaps::origin`] (1445c).
    pub fn plays_mod(&self) -> bool {
        !self.chain.is_empty()
    }

    /// Where an install map of the played folder is shown, given its [`MapOrigin`] (1445c).
    pub fn install_group(&self, origin: MapOrigin) -> MapGroup {
        install_map_group(origin, &self.chain)
    }
}

/// The key of a map in [`MapGate`]: its stem, lower-cased (the game's names ignore case).
fn stem_key(name: &str) -> String {
    Path::new(name).file_stem().map_or_else(|| name.to_string(), |s| s.to_string_lossy().into_owned()).to_ascii_lowercase()
}

/// What a merged game folder was built from; a folder whose stamp differs is rebuilt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    /// The base install's folder.
    base: String,
    /// The base's files: a digest of their names, sizes and times.
    base_files: String,
    /// Every mod laid, in the order they are laid (see [`layer_chain`]).
    layers: Vec<LayerStamp>,
    /// The merged folder's own files when it was built (see [`game_fingerprint`]); empty while
    /// it is being built. A file lost from the folder (an antivirus) rebuilds it.
    #[serde(default)]
    game: String,
    /// How the folder holds the base's files ([`LAYOUT`]); an older layout is rebuilt once.
    #[serde(default)]
    layout: u32,
}

/// The merged folder's layout: 2 = the base's files hard-linked where the file system allows it,
/// otherwise not there at all (read from the base through `dt::install::overlay_fallback`); 0 = older
/// folders, whose base files were copied where links failed (a full copy of the game per mod on
/// Android). A folder of an older layout is rebuilt once, which frees those copies.
const LAYOUT: u32 = 2;

/// One mod laid over the base: its id, its installed version and each of its files' path and
/// SHA-256. A change in any of them rebuilds every folder that lays it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LayerStamp {
    id: String,
    version: String,
    files: Vec<(String, String)>,
}

/// The mods a mod plays on, in the order they are laid: each requirement (after its own
/// requirements), each once, then `id` itself. A requirement that is not installed, and mods
/// that need each other in a circle, are refused with the reason.
pub fn layer_chain(installed: &Installed, id: &str) -> Result<Vec<String>, String> {
    fn visit(installed: &Installed, id: &str, path: &mut Vec<String>, out: &mut Vec<String>) -> Result<(), String> {
        if out.iter().any(|o| o == id) {
            return Ok(());
        }
        if path.iter().any(|p| p == id) {
            return Err(format!("{id}: mods need each other in a circle"));
        }
        let entry = match installed.entries.get(id).filter(|e| e.kind == Kind::Mod) {
            Some(entry) => entry,
            None => {
                return Err(match path.last() {
                    Some(parent) => format!("{parent} needs {id}, which is not installed"),
                    None => format!("{id}: not installed"),
                })
            }
        };
        path.push(id.to_string());
        for r in &entry.requires {
            visit(installed, r, path, out)?;
        }
        path.pop();
        out.push(id.to_string());
        Ok(())
    }
    let mut out = Vec::new();
    visit(installed, id, &mut Vec::new(), &mut out)?;
    Ok(out)
}

/// Builds the merged game folder of installed mod `id` over `base` (see the module doc), or
/// reuses it when its stamp still matches. Returns the folder.
pub fn prepare_mod(local: &Path, base: &Path, id: &str) -> Result<PathBuf, String> {
    if !safe_id(id) {
        return Err(format!("{id}: not a mod id"));
    }
    // The build reads the base folder as a whole (its fingerprint, its hard links): it waits for a
    // conversion of the base's pictures that is running (the first start after an update), so it never
    // lists a plain picture the conversion removes (backlog: "Bonus46.lit: No such file", the mod dropped).
    stored::wait_converted(base);
    // A mod whose files are damaged is not laid: a folder built from them would lack what the
    // mod's maps name (the caller repairs it first, `Hub::repair_mod`).
    let bad = damaged_files(local, id)?;
    if !bad.is_empty() {
        return Err(format!("{id}: damaged or missing: {}", bad.join(", ")));
    }
    let installed = read_installed(local);
    let order = layer_chain(&installed, id)?;
    // Mods installed before compression: their plain pictures are compressed now (a plain file is
    // kept until its `.lz4` is verified, and the manifests name files by their logical path, so
    // nothing else changes). The kept zips of older installs go too (`ARCHIVES`).
    for lid in &order {
        let _ = std::fs::remove_dir_all(mod_dir(local, lid).join(ARCHIVES));
        compress_pictures(&mod_files_dir(local, lid));
    }
    let mut layers = Vec::with_capacity(order.len());
    let mut stamps = Vec::with_capacity(order.len());
    for lid in &order {
        let version = installed.entries[lid].version.clone();
        let files = mod_files(&mod_files_dir(local, lid))?;
        // The asked mod's files were just checked against its manifest (`damaged_files` above),
        // so their digests are the manifest's: no second hash pass. Requirements are hashed.
        let manifest: HashMap<String, String> = match read_manifest(local, id).filter(|_| lid.as_str() == id) {
            Some(manifest) => manifest.files.into_iter().map(|f| (f.path, f.sha256.to_lowercase())).collect(),
            None => HashMap::new(),
        };
        // The stamp keeps the stored names (what the merged folder lays): a form change rebuilds the
        // folder once, after the conversion above.
        let mut digests = Vec::with_capacity(files.len());
        let mut laid = Vec::with_capacity(files.len());
        for f in &files {
            let digest = match manifest.get(&f.path) {
                Some(sum) => sum.clone(),
                None => {
                    let bytes = f.stored().read().map_err(|e| format!("{lid}: {}: {e}", f.path))?;
                    crate::update::sha256_hex(&bytes)
                }
            };
            digests.push((f.rel.clone(), digest));
            laid.push((f.rel.clone(), f.file.clone()));
        }
        stamps.push(LayerStamp { id: lid.clone(), version, files: digests });
        layers.push(laid);
    }
    let stamp = Stamp { base: base.display().to_string(), base_files: base_fingerprint(base)?, layers: stamps, game: String::new(), layout: LAYOUT };
    // Every lookup that misses in the merged folder reads the base (the base's files are not
    // copied where links fail, `build`).
    let game_dir = mod_game_dir(local, id);
    crate::dt::install::set_overlay(&game_dir, base);
    let game = game_dir;
    if let Some(mut built) = read_stamp(&game) {
        let fingerprint = std::mem::take(&mut built.game);
        if built == stamp && !fingerprint.is_empty() && game_fingerprint(&game).ok().as_deref() == Some(fingerprint.as_str()) {
            return Ok(game);
        }
        // A stamp written before fingerprints (1.3.13 and older) matches in everything else:
        // the folder is adopted, its fingerprint recorded, not rebuilt. On a phone the rebuild
        // is a full copy of the install, so it must not run at every start.
        if built == stamp && fingerprint.is_empty() && game.is_dir() {
            if let Ok(now) = game_fingerprint(&game) {
                let mut adopted = stamp.clone();
                adopted.game = now;
                if let Ok(json) = serde_json::to_vec_pretty(&adopted) {
                    if write_fresh(&game.join(STAMP), &json).is_ok() {
                        return Ok(game);
                    }
                }
            }
        }
    }
    let tmp = mod_dir(local, id).join("game.tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    let built = build(base, &layers, &tmp).and_then(|()| {
        let mut recorded = stamp.clone();
        recorded.game = game_fingerprint(&tmp)?;
        let json = serde_json::to_vec_pretty(&recorded).map_err(|e| e.to_string())?;
        write_fresh(&tmp.join(STAMP), &json)
    });
    if let Err(e) = built {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&game);
    std::fs::rename(&tmp, &game).map_err(|e| format!("{}: {e}", game.display()))?;
    Ok(game)
}

/// Removes the zips older installs kept under every installed mod's `archives/` (`prepare_mod` does it for
/// the mods it lays; this covers the mods never played). Called once at each launch, off the UI thread.
pub fn drop_all_kept_archives(local: &Path) {
    let Ok(read) = std::fs::read_dir(local.join("mods")) else { return };
    for entry in read.flatten() {
        let _ = std::fs::remove_dir_all(entry.path().join(ARCHIVES));
    }
}

/// Removes mod `id`'s built game folder (and a half-built one). Its `files/` stay, so playing
/// the mod again rebuilds it. Saves are never in it (`save::saves_dir_of`).
pub fn drop_game_folder(local: &Path, id: &str) {
    if !safe_id(id) {
        return;
    }
    let _ = std::fs::remove_dir_all(mod_game_dir(local, id));
    let _ = std::fs::remove_dir_all(mod_dir(local, id).join("game.tmp"));
}

/// Keeps the built game folders of the `keep` mods played most recently, and the active mod's
/// even when it is not among them; every other mod's game folder under `mods/` goes. The rule is
/// two (the owner's decision, 2026-11-10): switching between two mods is instant, and each folder
/// is a full copy of the base where hard links fail (about 128 MB on a phone), so no more than
/// two. Played order is the `last_played` marker's modification time; a mod without one is the
/// oldest, and ties go by id. Saves and `files/` are never touched (see [`drop_game_folder`]).
pub fn keep_recent_game_folders(local: &Path, keep: usize) {
    let Ok(read) = std::fs::read_dir(local.join("mods")) else { return };
    let mut mods: Vec<(String, Option<web_time::SystemTime>)> = read
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            let played = std::fs::metadata(mod_dir(local, &id).join(LAST_PLAYED)).and_then(|m| m.modified()).ok();
            (id, played)
        })
        .collect();
    // Newest first (`None`, no marker, sorts last); ties by id.
    mods.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut kept: HashSet<String> = mods.iter().take(keep).map(|(id, _)| id.clone()).collect();
    kept.extend(active_mod(local));
    for (id, _) in &mods {
        if !kept.contains(id) {
            drop_game_folder(local, id);
        }
    }
}

fn read_stamp(game: &Path) -> Option<Stamp> {
    serde_json::from_slice(&std::fs::read(game.join(STAMP)).ok()?).ok()
}

/// Writes `path`, first removing what is there: a hard link to a base file must not be written
/// through.
fn write_fresh(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let _ = std::fs::remove_file(path);
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Builds the merged folder `tmp` from `base` and the layers: each layer's files (`(stored path in
/// the mod, source)`, see [`mod_files`]) laid in turn, a later layer's file replacing an earlier one
/// of the same logical name (ignoring case): the other stored form of it goes
/// ([`drop_other_stored_form`]).
fn build(base: &Path, layers: &[Vec<(String, PathBuf)>], tmp: &Path) -> Result<(), String> {
    // Lower-cased path → the path as it is in the merged folder (the base's spelling where the
    // base has the folder or file).
    let mut known: BTreeMap<String, String> = BTreeMap::new();
    let mut all = Vec::new();
    walk(base, base, &mut all)?;
    for (rel, path, is_dir) in all {
        known.insert(rel.to_ascii_lowercase(), rel.clone());
        let target = tmp.join(&rel);
        if is_dir {
            std::fs::create_dir_all(&target).map_err(|e| format!("{rel}: {e}"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{rel}: {e}"))?;
        }
        // A link where the file system allows it; otherwise the base's file is left out, never
        // copied: the game reads it from the base (`dt::install::overlay_fallback`). Android's app
        // storage refuses links, and a copy there was a whole second game per mod.
        if links_refused() || std::fs::hard_link(&path, &target).is_err() {
            continue;
        }
    }
    for mod_files in layers {
        let mut seen = HashSet::new();
        for (rel, src) in mod_files {
            let logical = logical_rel(rel);
            if !seen.insert(logical.to_ascii_lowercase()) {
                return Err(format!("{logical}: two files of one mod differ only in case"));
            }
            let target = resolve(&mut known, rel);
            let dst = tmp.join(&target);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{target}: {e}"))?;
            }
            // Breaks the link to the base's file (or an earlier layer's copy) first; the base keeps its own.
            let _ = std::fs::remove_file(&dst);
            // The other stored form of this name goes too (the base's plain link or its `.lz4`, an
            // earlier layer's copy), so the mod's file is the only one (backlog 1609 part 2, rule 6).
            drop_other_stored_form(&dst);
            forget_other_form(&mut known, &target);
            std::fs::copy(src, &dst).map_err(|e| format!("{target}: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
crate::local! {
    /// Tests: build merged folders as on a file system without hard links.
    static NO_LINKS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether hard links are not to be tried (tests of the no-links layout).
#[cfg(test)]
fn links_refused() -> bool {
    NO_LINKS.with(std::cell::Cell::get)
}

#[cfg(not(test))]
fn links_refused() -> bool {
    false
}

/// Removes the other stored form of the picture `dst` from its folder of the merged folder, any case:
/// a mod's plain `A.lit` replaces the base's stored `a.lit.lz4`, and a mod's `A.lit.lz4` replaces the
/// base's plain `a.lit` (a hard link), so the folder holds one form of the name, and a plain file
/// there always wins over the stored one. Only the link (or the copy) in the merged folder goes; the
/// base's file stays. Names that may not be stored compressed: nothing.
fn drop_other_stored_form(dst: &Path) {
    let (Some(dir), Some(name)) = (dst.parent(), dst.file_name()) else { return };
    let name = name.to_string_lossy();
    let logical = stored::logical_name_of(&name).unwrap_or(&name);
    if !stored::is_compressible(logical) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let other = entry.file_name().to_string_lossy().into_owned();
        let other_logical = stored::logical_name_of(&other).unwrap_or(&other);
        if other_logical.eq_ignore_ascii_case(logical) && entry.path().is_file() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Forgets, in `known`, the other stored form of the picture at `target` (its spelling is gone from
/// the merged folder with the file, see [`drop_other_stored_form`]). `target`'s own key stays.
fn forget_other_form(known: &mut BTreeMap<String, String>, target: &str) {
    let logical = logical_rel(target);
    if !stored::is_compressible(&logical) {
        return;
    }
    let own = target.to_ascii_lowercase();
    let plain = logical.to_ascii_lowercase();
    for key in [plain.clone(), format!("{plain}.{}", stored::LZ4_EXT)] {
        if key != own {
            known.remove(&key);
        }
    }
}

/// The spelling a mod's path takes in the merged folder: each folder and file the base has
/// (ignoring ASCII case) keeps the base's spelling, the rest the mod's. Records what is new.
fn resolve(known: &mut BTreeMap<String, String>, rel: &str) -> String {
    let mut actual = String::new();
    let mut key = String::new();
    for comp in rel.split('/') {
        if !key.is_empty() {
            key.push('/');
        }
        key.push_str(&comp.to_ascii_lowercase());
        actual = match known.get(&key) {
            Some(found) => found.clone(),
            None => {
                let next = if actual.is_empty() { comp.to_string() } else { format!("{actual}/{comp}") };
                known.insert(key.clone(), next.clone());
                next
            }
        };
    }
    actual
}

/// Every entry under `dir`: its path from `root` with `/` separators, its path and whether it
/// is a folder. Symbolic links are followed.
fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf, bool)>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        let meta = std::fs::metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let rel = path.strip_prefix(root).unwrap_or(&path).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
        if meta.is_dir() {
            out.push((rel, path.clone(), true));
            walk(root, &path, out)?;
        } else if meta.is_file() {
            out.push((rel, path, false));
        }
    }
    Ok(())
}

/// The mod's files under `files_dir`, as (stored path in the mod, file), sorted: a plain file or a
/// `<name>.lz4`. A name that is not safe is an error; a folder with no files is an empty mod. The
/// leftover `<name>.lz4.part` of an interrupted conversion is not a file of the mod.
fn list_mod_files(files_dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    if !files_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut all = Vec::new();
    walk(files_dir, files_dir, &mut all)?;
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for (rel, path, is_dir) in all {
        if is_dir || rel.to_ascii_lowercase().ends_with(&format!(".{}.{}", stored::LZ4_EXT, stored::PART_SUFFIX)) {
            continue;
        }
        if !safe_rel_name(&rel) {
            return Err(format!("{rel}: not a file name a mod may use"));
        }
        files.push((rel, path));
    }
    files.sort();
    Ok(files)
}

/// The most a mod's archive may unpack to, in all.
pub const MAX_UNPACKED_BYTES: u64 = 2 << 30;
/// The most entries a mod's archive may hold.
pub const MAX_ZIP_ENTRIES: usize = 20_000;

/// Unpacks the zip `zip_path` into `dest` (a fresh folder): every file at its path in the zip
/// (a `\` counts as `/`), each name checked as a mod's file name is. Returns the paths written.
/// A zip over the limits or with a bad name is refused whole.
pub fn unpack_zip(zip_path: &Path, dest: &Path) -> Result<Vec<String>, String> {
    unpack_zip_within(zip_path, dest, MAX_UNPACKED_BYTES, MAX_ZIP_ENTRIES)
}

fn unpack_zip_within(zip_path: &Path, dest: &Path, max_bytes: u64, max_entries: usize) -> Result<Vec<String>, String> {
    let shown = zip_path.display();
    let file = std::fs::File::open(zip_path).map_err(|e| format!("{shown}: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{shown}: not a zip ({e})"))?;
    if zip.len() > max_entries {
        return Err(format!("{shown}: {} entries, at most {max_entries}", zip.len()));
    }
    let mut names = Vec::new();
    let mut total: u64 = 0;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| format!("{shown}: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().map_err(|e| format!("{shown}: {e}"))?.replace('\\', "/");
        if !safe_rel_name(&name) {
            return Err(format!("{name}: not a file name a mod may use"));
        }
        // The declared size first, then what is really read (an entry may lie about its size).
        let left = max_bytes.saturating_sub(total);
        if entry.size() > left {
            return Err(format!("{shown}: unpacks to more than {} MiB", max_bytes >> 20));
        }
        let target = dest.join(&name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{name}: {e}"))?;
        }
        let mut out = std::fs::File::create(&target).map_err(|e| format!("{name}: {e}"))?;
        // One byte past what is left, so an entry that runs over shows it.
        let copied = std::io::copy(&mut entry.by_ref().take(left + 1), &mut out).map_err(|e| format!("{name}: {e}"))?;
        total += copied;
        if total > max_bytes {
            return Err(format!("{shown}: unpacks to more than {} MiB", max_bytes >> 20));
        }
        names.push(name);
    }
    Ok(names)
}

/// A digest of a merged game folder's files: their names and sizes (its stamp left out). A
/// folder that lost a file, or gained one, does not match the stamp's and is built again.
fn game_fingerprint(game: &Path) -> Result<String, String> {
    let mut all = Vec::new();
    walk(game, game, &mut all)?;
    let mut lines = Vec::new();
    for (rel, path, is_dir) in all {
        if is_dir || rel == STAMP {
            continue;
        }
        let len = std::fs::metadata(&path).map_err(|e| format!("{rel}: {e}"))?.len();
        lines.push(format!("{rel}\t{len}"));
    }
    lines.sort();
    Ok(crate::update::sha256_hex(lines.join("\n").as_bytes()))
}

#[cfg(test)]
crate::local! {
    /// Tests: runs before each base file of [`base_fingerprint`] is stat'ed, after the listing (a
    /// conversion of the base can land in that window: the race of the backlog).
    static BEFORE_STAT: std::cell::RefCell<Option<Box<dyn FnMut(&Path)>>> = const { std::cell::RefCell::new(None) };
}

/// Tests: the hook of [`BEFORE_STAT`] for the file `path`, if one is set.
#[cfg(test)]
fn before_stat(path: &Path) {
    BEFORE_STAT.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(path);
        }
    });
}

#[cfg(not(test))]
fn before_stat(_path: &Path) {}

/// A digest of the base's files: their names, sizes and modification times.
fn base_fingerprint(base: &Path) -> Result<String, String> {
    let mut all = Vec::new();
    walk(base, base, &mut all)?;
    let mut lines = Vec::new();
    for (rel, path, is_dir) in all {
        if is_dir {
            continue;
        }
        before_stat(&path);
        let meta = std::fs::metadata(&path).map_err(|e| format!("{rel}: {e}"))?;
        let time = meta.modified().ok().and_then(|t| t.duration_since(web_time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        lines.push(format!("{rel}\t{}\t{time}", meta.len()));
    }
    lines.sort();
    Ok(crate::update::sha256_hex(lines.join("\n").as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{safe_name, Catalog, Installed};

    fn temp(tag: &str) -> PathBuf {
        let nanos = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("razdor-mods-{tag}-{}-{nanos}", std::process::id()))
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A base install with `Data/Units.ini` and one picture.
    fn base_install(tag: &str) -> PathBuf {
        let base = temp(&format!("{tag}-base"));
        put(&base.join("Data/Units.ini"), "base units");
        put(&base.join("Pictures/a.lit"), "picture");
        base
    }

    /// Installs mod `id` the way the hub does: its files under `files/`, the record in
    /// `installed.json`.
    fn install_mod(local: &Path, id: &str, version: &str, files: &[(&str, &str)]) {
        install_mod_on(local, id, version, &[], files);
    }

    /// [`install_mod`] for a mod that requires `requires` (installed before it).
    fn install_mod_on(local: &Path, id: &str, version: &str, requires: &[&str], files: &[(&str, &str)]) {
        let _ = std::fs::remove_dir_all(mod_files_dir(local, id));
        for (rel, text) in files {
            put(&mod_files_dir(local, id).join(rel), text);
        }
        let mut installed = read_installed(local);
        let record = super::super::InstalledEntry {
            version: version.into(),
            kind: Kind::Mod,
            lang: String::new(),
            title: Text { ru: id.into(), en: String::new() },
            requires: requires.iter().map(|r| r.to_string()).collect(),
            files: files.iter().map(|(rel, _)| format!("mods/{id}/files/{rel}")).collect(),
            sources: Vec::new(),
        };
        installed.entries.insert(id.into(), record);
        std::fs::create_dir_all(local).unwrap();
        std::fs::write(local.join("installed.json"), serde_json::to_vec(&installed).unwrap()).unwrap();
    }

    /// Whether the file system under `dir` tells names apart by case (macOS's usually does not):
    /// the checks of a second folder spelled in another case only mean something where it does.
    fn case_sensitive(dir: &Path) -> bool {
        std::fs::create_dir_all(dir).unwrap();
        let probe = dir.join("case-probe");
        std::fs::write(&probe, b"").unwrap();
        let sensitive = !dir.join("CASE-PROBE").exists();
        let _ = std::fs::remove_file(&probe);
        sensitive
    }

    #[test]
    fn a_mod_file_replaces_the_base_file_ignoring_case_and_keeps_the_base_spelling() {
        let base = base_install("overlay");
        let local = temp("overlay-hub");
        install_mod(&local, "m1", "1.0", &[("data/units.ini", "mod units"), ("Scripts/New.txt", "new")]);
        let game = prepare_mod(&local, &base, "m1").unwrap();
        assert_eq!(std::fs::read_to_string(game.join("Data/Units.ini")).unwrap(), "mod units");
        if case_sensitive(&local) {
            assert!(!game.join("data").exists(), "no second folder spelled as the mod has it");
        }
        assert_eq!(std::fs::read_to_string(game.join("Scripts/New.txt")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(game.join("Pictures/a.lit")).unwrap(), "picture", "the base's files are there");
        assert_eq!(std::fs::read_to_string(base.join("Data/Units.ini")).unwrap(), "base units", "the base is not written");
        assert!(!base.join("Scripts").exists(), "the base gets no new file");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn the_folder_is_rebuilt_when_the_mod_files_or_the_base_change() {
        let base = base_install("stale");
        let local = temp("stale-hub");
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units")]);
        let game = prepare_mod(&local, &base, "m1").unwrap();
        let first = std::fs::metadata(game.join(STAMP)).unwrap().modified().ok();
        assert!(first.is_some());
        // Same mod: the folder is reused as it is.
        prepare_mod(&local, &base, "m1").unwrap();
        assert_eq!(std::fs::metadata(game.join(STAMP)).unwrap().modified().ok(), first, "an unchanged folder is not rebuilt");
        // A file added to the folder is a change: rebuilt without it.
        put(&game.join("marker.txt"), "kept");
        prepare_mod(&local, &base, "m1").unwrap();
        assert!(!game.join("marker.txt").exists(), "any change in the folder rebuilds it");
        // A new file in the mod: rebuilt, the new file is there.
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units"), ("Data/Extra.ini", "extra")]);
        prepare_mod(&local, &base, "m1").unwrap();
        assert!(!game.join("marker.txt").exists(), "a changed file list rebuilds the folder");
        assert_eq!(std::fs::read_to_string(game.join("Data/Extra.ini")).unwrap(), "extra");
        // A removed file: rebuilt without it.
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units")]);
        prepare_mod(&local, &base, "m1").unwrap();
        assert!(!game.join("Data/Extra.ini").exists());
        // A new base file (the base changed): rebuilt too.
        put(&base.join("Data/Added.ini"), "x");
        prepare_mod(&local, &base, "m1").unwrap();
        assert!(game.join("Data/Added.ini").exists(), "the base's own files reach the folder");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// The stamp takes the asked mod's digests from its manifest (checked just before, no hash
    /// pass of its own); they are the digests of the files as they are, so the stamp is the one
    /// the hashing gave, and a switch to the same mod again reuses the folder.
    #[test]
    fn the_stamp_from_the_manifest_matches_the_hashed_one() {
        let base = base_install("manifest-stamp");
        let local = temp("manifest-stamp-hub");
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units"), ("Pictures/b.lit", "mod picture")]);
        write_manifest_now(&local, "m1");
        let game = prepare_mod(&local, &base, "m1").unwrap();
        let stamp = read_stamp(&game).unwrap();
        // The picture is compressed (`prepare_mod`): the stamp names it as laid, and its digest is the
        // one of the decoded bytes, as the manifest's and the hashing's are.
        let hashed: Vec<(String, String)> = vec![("Data/Units.ini".into(), crate::update::sha256_hex(b"mod units")), ("Pictures/b.lit.lz4".into(), crate::update::sha256_hex(b"mod picture"))];
        assert_eq!(stamp.layers.last().unwrap().files, hashed, "the stamp holds the digests of the decoded bytes");
        // A stamp that is kept is not rewritten: its mtime stays where it is set.
        let path = game.join(STAMP);
        let past = web_time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        std::fs::OpenOptions::new().write(true).open(&path).unwrap().set_modified(past).unwrap();
        let again = prepare_mod(&local, &base, "m1").unwrap();
        assert_eq!(again, game, "the same folder");
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), past, "the folder is reused, not built again");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_stamp_from_before_fingerprints_is_adopted_not_rebuilt() {
        let base = base_install("adopt");
        let local = temp("adopt-hub");
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units")]);
        let game = prepare_mod(&local, &base, "m1").unwrap();
        // Rewrite the stamp the way 1.3.13 wrote it: no `game` key.
        let path = game.join(STAMP);
        let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        json.as_object_mut().unwrap().remove("game");
        std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
        assert!(read_stamp(&game).unwrap().game.is_empty());
        // A marker in the folder survives only if the folder is not rebuilt.
        put(&game.join("marker.txt"), "kept");
        let again = prepare_mod(&local, &base, "m1").unwrap();
        assert_eq!(again, game);
        assert_eq!(std::fs::read_to_string(game.join("marker.txt")).unwrap(), "kept", "the folder is adopted, not rebuilt");
        assert!(!read_stamp(&game).unwrap().game.is_empty(), "the adopted stamp records the folder's fingerprint");
        // And the next start reuses it as any other built folder.
        prepare_mod(&local, &base, "m1").unwrap();
        assert_eq!(std::fs::read_to_string(game.join("marker.txt")).unwrap(), "kept");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn the_active_mod_falls_back_to_the_base_when_it_is_gone() {
        let base = base_install("active");
        let local = temp("active-hub");
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units")]);
        let game = use_mod(&local, &base, Some("m1")).unwrap();
        assert_eq!(active_install_dir_in(&local, &base), game);
        assert_eq!(active_mod(&local).as_deref(), Some("m1"));
        // The mod is removed from the record: the base is played and the choice is cleared.
        std::fs::write(local.join("installed.json"), b"{}").unwrap();
        assert_eq!(active_install_dir_in(&local, &base), base);
        assert_eq!(active_mod(&local), None);
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// The start's line is shown only when the load really waits: an active, installed mod over a base that converts.
    #[test]
    fn the_install_waits_only_for_an_active_mod_while_its_base_converts() {
        let base = base_install("waits");
        let local = temp("waits-hub");
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units")]);
        assert!(!install_waits_for_conversion(&local, &base), "no mod is active");
        write_active(&local, Some("m1")).unwrap();
        assert!(!install_waits_for_conversion(&local, &base), "the base is not converting");
        let held = crate::dt::stored::hold_converting_for_test(&base);
        assert!(install_waits_for_conversion(&local, &base), "the active mod waits for the base's conversion");
        drop(held);
        assert!(!install_waits_for_conversion(&local, &base), "the conversion ended");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_mod_that_does_not_build_is_not_played() {
        let base = base_install("broken");
        let local = temp("broken-hub");
        // Two files differing only in case cannot both exist where names ignore case (macOS).
        if !case_sensitive(&local) {
            let _ = std::fs::remove_dir_all(&base);
            let _ = std::fs::remove_dir_all(&local);
            return;
        }
        // Two files differing only in case: the mod cannot be laid over the base.
        install_mod(&local, "m1", "1.0", &[("Data/A.ini", "a"), ("data/a.ini", "b")]);
        write_active(&local, Some("m1")).unwrap();
        assert_eq!(active_install_dir_in(&local, &base), base);
        assert_eq!(active_mod(&local), None, "a choice that does not build is cleared");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn maps_that_need_a_mod_show_only_while_it_is_active() {
        let local = temp("gate-hub");
        let mut installed = Installed::default();
        installed.entries.insert(
            "rotten".into(),
            super::super::InstalledEntry {
                version: "1".into(),
                kind: Kind::Map,
                lang: String::new(),
                title: Text::default(),
                requires: vec!["evo".into()],
                files: vec!["maps/Hub_Rotten.DTm".into()],
                sources: Vec::new(),
            },
        );
        installed.entries.insert(
            "ford".into(),
            super::super::InstalledEntry { version: "1".into(), kind: Kind::Map, lang: String::new(), title: Text::default(), requires: vec![], files: vec!["maps/Hub_Ford.DTm".into()], sources: vec![] },
        );
        installed.entries.insert("evo".into(), super::super::InstalledEntry { version: "1".into(), kind: Kind::Mod, lang: String::new(), title: Text::default(), requires: vec![], files: vec![], sources: vec![] });
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(local.join("installed.json"), serde_json::to_vec(&installed).unwrap()).unwrap();
        assert!(MapGate::read(&local).group("Hub_Rotten.DTm", false) == MapGroup::Hidden, "no mod is active");
        assert!(MapGate::read(&local).group("hub_rotten", false) == MapGroup::Hidden, "the new-game list names a map by its stem, in any case");
        assert!(MapGate::read(&local).group("Hub_Ford.DTm", false) == MapGroup::Chosen, "a map for the base game shows with no mod");
        assert!(MapGate::read(&local).group("Install_Map.DTm", false) == MapGroup::Chosen, "a map the hub did not install is a base map");
        write_active(&local, Some("evo")).unwrap();
        assert_eq!(MapGate::read(&local).group("Hub_Rotten.DTm", false), MapGroup::Chosen);
        assert!(MapGate::read(&local).group("Hub_Ford.DTm", false) == MapGroup::Base, "a mod is chosen: the base game's hub map is under its group");
        write_active(&local, Some("other")).unwrap();
        assert!(MapGate::read(&local).group("Hub_Rotten.DTm", false) == MapGroup::Hidden, "another mod does not show it");
        let _ = std::fs::remove_dir_all(&local);
    }

    /// The rule of 1445 and 1445b, as a pure function: the base game shows the maps that need no
    /// mod; a chosen mod shows the maps that need it or a mod it is laid over, and the maps made
    /// for the base game under their own group; an identical copy of an install map is never shown
    /// twice.
    #[test]
    fn the_new_game_list_shows_the_base_maps_or_the_mods_maps_once() {
        let none: &[String] = &[];
        let evo = vec!["evolution".to_string()];
        let builds_on = vec!["evolution".to_string(), "dt-orders".to_string()];
        assert_eq!(map_group(none, none, false), MapGroup::Chosen, "base: a base map is in the list");
        assert_eq!(map_group(&evo, none, false), MapGroup::Hidden, "base: a map that needs Evolution is hidden");
        assert_eq!(map_group(&evo, &evo, false), MapGroup::Chosen, "Evolution chosen: its own map is in the list");
        assert_eq!(map_group(none, &evo, false), MapGroup::Base, "Evolution chosen: a base map is under the base game's group");
        assert_eq!(map_group(&["other".to_string()], &evo, false), MapGroup::Hidden, "Evolution chosen: another mod's map is hidden");
        assert_eq!(map_group(&evo, &builds_on, false), MapGroup::Chosen, "a mod built on Evolution: Evolution's map is in the list");
        assert_eq!(map_group(none, &builds_on, false), MapGroup::Base, "a mod built on Evolution: a base map is under the group");
        assert_eq!(map_group(none, none, true), MapGroup::Hidden, "base: the hub's copy of an install map is not listed twice");
        assert_eq!(map_group(&evo, &evo, true), MapGroup::Hidden, "Evolution chosen: an identical copy is not listed twice");
        assert_eq!(map_group(none, &evo, true), MapGroup::Hidden, "Evolution chosen: an identical base copy is not listed twice");
    }

    /// 1445c: with a mod played, an install map the base install has unchanged (the same file name
    /// ignoring case, the same bytes) is the base game's; one the mods add or change is the mod's.
    /// With no mod played every install map is in the main part.
    #[test]
    fn an_install_map_the_mod_leaves_unchanged_is_the_base_games() {
        let none: &[String] = &[];
        let evo = vec!["evolution".to_string()];
        assert_eq!(install_map_group(MapOrigin::Base, none), MapGroup::Chosen, "no mod: a base map is in the main part");
        assert_eq!(install_map_group(MapOrigin::Mod, none), MapGroup::Chosen, "no mod: a mod's map is in the main part");
        assert_eq!(install_map_group(MapOrigin::Base, &evo), MapGroup::Base, "Evolution: a base map it leaves is under the base game's group");
        assert_eq!(install_map_group(MapOrigin::Mod, &evo), MapGroup::Chosen, "Evolution: a map it adds or changes is in the main part");

        let dir = temp("origin");
        let base_dir = dir.join("base");
        put(&base_dir.join("Maps/Same.DTm"), "same bytes");
        put(&base_dir.join("Maps/Changed.DTm"), "base bytes");
        let played = dir.join("game");
        put(&played.join("Maps/same.dtm"), "same bytes");
        put(&played.join("Maps/Changed.DTm"), "mod bytes!");
        put(&played.join("Maps/New.DTm"), "new");
        let base = BaseMaps::new(&[base_dir.join("Maps/Same.DTm"), base_dir.join("Maps/Changed.DTm")]);
        assert_eq!(base.origin(&played.join("Maps/same.dtm")), MapOrigin::Base, "the same name ignoring case, the same bytes: unchanged");
        assert_eq!(base.origin(&played.join("Maps/Changed.DTm")), MapOrigin::Mod, "the same size, other bytes: the mod replaced it");
        assert_eq!(base.origin(&played.join("Maps/New.DTm")), MapOrigin::Mod, "a map the base does not have: the mod's");
        assert_eq!(base.origin(&played.join("Maps/missing.DTm")), MapOrigin::Mod, "a file that does not read is not the base's");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The duplicate check (1445): a hub map is the install's when an install map has the same
    /// bytes; the same size with other bytes, or a file that does not read, is not.
    #[test]
    fn a_hub_map_with_the_install_s_bytes_is_the_same_map() {
        let dir = temp("same-map");
        std::fs::create_dir_all(&dir).unwrap();
        let install = dir.join("install.DTm");
        let copy = dir.join("hub_copy.DTm");
        let other = dir.join("hub_other.DTm");
        std::fs::write(&install, b"map bytes").unwrap();
        std::fs::write(&copy, b"map bytes").unwrap();
        std::fs::write(&other, b"map bytez").unwrap();
        let maps = InstallMaps::new(std::slice::from_ref(&install));
        assert!(maps.has_same(&copy), "the same bytes");
        assert!(!maps.has_same(&other), "the same size, other bytes");
        assert!(!maps.has_same(&dir.join("missing.DTm")), "a file that does not read is not the install's");
        assert!(!InstallMaps::new(&[]).has_same(&copy), "no install maps: nothing is the same");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_mod_file_names_are_refused_by_the_catalog() {
        let entry = |kind: &str, name: &str| {
            format!(r#"{{"format":1,"entries":[{{"id":"m","kind":"{kind}","title":{{"ru":"x"}},"author":"a","version":"1","files":[{{"path":"p","name":"{name}","size":1,"sha256":"00000000000000000000000000000000000000000000000000000000000000ab"}}]}}]}}"#)
        };
        for bad in ["../x.ini", "a/../b.ini", "/abs.ini", "a\\b.ini", "a//b.ini", "a/", "", "a/CON/x.ini"] {
            assert!(Catalog::parse(entry("mod", bad).as_bytes()).is_err(), "{bad} must be refused");
        }
        assert!(Catalog::parse(entry("mod", "data/units.ini").as_bytes()).is_ok());
        assert!(Catalog::parse(entry("map", "data/units.ini").as_bytes()).is_err(), "a map's file is a name, not a path");
        assert!(safe_rel_name("Data/Units.ini"));
    }

    #[test]
    fn community_file_names_are_accepted_and_the_bad_ones_refused() {
        // Real names: brackets, apostrophes, commas, Cyrillic, dots inside, a long name.
        let good: Vec<String> = [
            "Слова автора(ПРОЧИТАТЬ ОБЯЗАТЕЛЬНО).txt",
            "Стражи Тиара'с.DTm",
            "Карта, часть 2 [новая].DTm",
            "a b.c",
            "Имя.с.точками.DTm",
            "CONSOLE.txt",
            "com10.dtm",
            ".hidden",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain(["é".repeat(127)])
        .collect();
        for name in &good {
            assert!(safe_name(name), "{name:?} must be accepted");
            assert!(safe_rel_name(&format!("data/{name}")), "{name:?} as a folder's file");
        }
        let bad: Vec<String> = [
            "", ".", "..", "a/b", "a\\b", "c:x", "a<b", "a>b", "a\"b", "a|b", "a?b", "a*b", "tab\there", "bell\u{7}",
            " lead.DTm", "trail.DTm ", "trail.", "CON", "con.txt", "Aux.DTm", "NUL", "COM1", "lpt9.txt", "LPT1.DTm",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain(["x".repeat(256), "é".repeat(128)])
        .collect();
        for name in &bad {
            assert!(!safe_name(name), "{name:?} must be refused");
            // A name with a `/` is a path of folders (checked on its own); the others as a folder's file.
            if !name.contains('/') {
                assert!(!safe_rel_name(&format!("data/{name}")), "{name:?} as a folder's file");
            }
        }
        assert!(!safe_rel_name("data/../x.ini"), "traversal in a mod's path");
        assert!(!safe_rel_name("data/CON/x.ini"), "a device name as a folder");
    }

    #[test]
    fn a_zip_unpacks_community_names_and_refuses_a_device_name() {
        let dir = temp("zip-names");
        let zip = zip_file(&dir, "names.zip", &[("Карта [1], часть 'два'.DTm", b"a"), ("Слова автора(ПРОЧИТАТЬ).txt", b"b")]);
        let names = unpack_zip(&zip, &dir.join("out")).unwrap();
        assert_eq!(names, ["Карта [1], часть 'два'.DTm", "Слова автора(ПРОЧИТАТЬ).txt"]);
        let bad = zip_file(&dir, "dev.zip", &[("NUL.txt", b"x")]);
        assert!(unpack_zip(&bad, &dir.join("out2")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn three_layers_each_replace_the_one_below_and_a_change_rebuilds_the_upper_folder() {
        // base → A (requires nothing) → B (requires A). B's file wins over A's, A's over the base's.
        let base = base_install("layers");
        put(&base.join("Pictures/a.lit"), "base-pic");
        let local = temp("layers-hub");
        install_mod(&local, "a", "1.0", &[("Data/Units.ini", "A units"), ("Pictures/a.lit", "A-pic"), ("Scripts/a.txt", "a")]);
        install_mod_on(&local, "b", "1.0", &["a"], &[("data/units.ini", "B units"), ("Scripts/b.txt", "b")]);
        let game = prepare_mod(&local, &base, "b").unwrap();
        assert_eq!(std::fs::read_to_string(game.join("Data/Units.ini")).unwrap(), "B units", "B's file overrides A's, keeping the base's spelling");
        assert_eq!(read_logical(&game, "Pictures/a.lit"), "A-pic", "A's file overrides the base's, B has none");
        assert_eq!(std::fs::read_to_string(game.join("Scripts/a.txt")).unwrap(), "a", "A's own file is there");
        assert_eq!(std::fs::read_to_string(game.join("Scripts/b.txt")).unwrap(), "b", "B's own file is there");
        assert_eq!(std::fs::read_to_string(base.join("Data/Units.ini")).unwrap(), "base units", "the base is not written");
        // A changes: B's folder is rebuilt, B's file still wins.
        put(&game.join("marker.txt"), "stale");
        install_mod(&local, "a", "1.1", &[("Data/Units.ini", "A units"), ("Pictures/a.lit", "A-pic-2"), ("Scripts/a.txt", "a2")]);
        let game = prepare_mod(&local, &base, "b").unwrap();
        assert!(!game.join("marker.txt").exists(), "a change in A rebuilds B's folder");
        assert_eq!(read_logical(&game, "Pictures/a.lit"), "A-pic-2");
        assert_eq!(std::fs::read_to_string(game.join("Scripts/a.txt")).unwrap(), "a2");
        assert_eq!(std::fs::read_to_string(game.join("Data/Units.ini")).unwrap(), "B units");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_missing_requirement_and_a_circle_are_refused_with_the_reason() {
        let base = base_install("chain-errors");
        let local = temp("chain-errors-hub");
        install_mod_on(&local, "b", "1.0", &["gone"], &[("x.ini", "b")]);
        let err = prepare_mod(&local, &base, "b").unwrap_err();
        assert!(err.contains("b needs gone"), "{err}");
        install_mod_on(&local, "gone", "1.0", &["c"], &[("y.ini", "g")]);
        install_mod_on(&local, "c", "1.0", &["gone"], &[("z.ini", "c")]);
        let err = prepare_mod(&local, &base, "gone").unwrap_err();
        assert!(err.contains("circle"), "{err}");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_mod_on_the_real_install_loads_with_a_changed_ini() {
        let Some(install) = std::env::var_os(crate::dt::install::ENV_VAR).map(PathBuf::from) else { return };
        let local = temp("real-hub");
        // The install keeps its ini files at its top (`_Global.ini`); the mod spells it in lower case.
        let global = std::fs::read(install.join("_Global.ini")).expect("the install has _Global.ini");
        let mut changed = global.clone();
        changed.extend_from_slice(b"\r\n");
        install_mod(&local, "real", "1.0", &[("_global.ini", "")]);
        std::fs::write(mod_files_dir(&local, "real").join("_global.ini"), &changed).unwrap();
        let game = prepare_mod(&local, &install, "real").unwrap();
        assert_eq!(std::fs::read(game.join("_Global.ini")).unwrap(), changed);
        assert_eq!(std::fs::read(install.join("_Global.ini")).unwrap(), global, "the install is not written");
        assert!(crate::dt::install::DtInstall::load(&game).is_ok(), "the merged folder loads as an install");
        let _ = std::fs::remove_dir_all(&local);
    }

    /// A zip of `entries` (name, bytes), written to `dir/name`.
    fn zip_file(dir: &Path, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        use std::io::Write;
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (entry, bytes) in entries {
            w.start_file(*entry, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap();
        path
    }

    #[test]
    fn a_good_zip_unpacks_at_its_paths_and_backslashes_count_as_folders() {
        let dir = temp("zip-good");
        let zip = zip_file(&dir, "pack.zip", &[("data/units.ini", b"units"), ("Pictures\\a.lit", b"pic"), ("empty/", b"")]);
        let out = dir.join("out");
        let names = unpack_zip(&zip, &out).unwrap();
        assert_eq!(names, ["data/units.ini", "Pictures/a.lit"]);
        assert_eq!(std::fs::read(out.join("data/units.ini")).unwrap(), b"units");
        assert_eq!(std::fs::read(out.join("Pictures/a.lit")).unwrap(), b"pic");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_zip_with_a_path_out_of_the_mod_is_refused_whole() {
        let dir = temp("zip-slip");
        for bad in ["../x.ini", "a/../../x.ini", "/abs.ini", "..\\x.ini", "a/CON/x.ini"] {
            let zip = zip_file(&dir, "bad.zip", &[("ok.ini", b"ok"), (bad, b"x")]);
            let out = dir.join("out");
            assert!(unpack_zip(&zip, &out).is_err(), "{bad} must be refused");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_zip_over_the_entry_or_size_limit_is_refused() {
        let dir = temp("zip-limits");
        let zip = zip_file(&dir, "big.zip", &[("a.ini", b"0123456789"), ("b.ini", b"0123456789")]);
        assert!(unpack_zip_within(&zip, &dir.join("e"), MAX_UNPACKED_BYTES, 1).is_err(), "two entries, one allowed");
        assert!(unpack_zip_within(&zip, &dir.join("s"), 15, MAX_ZIP_ENTRIES).is_err(), "twenty bytes, fifteen allowed");
        assert_eq!(unpack_zip_within(&zip, &dir.join("ok"), 20, MAX_ZIP_ENTRIES).unwrap().len(), 2);
        assert!(unpack_zip(&dir.join("missing.zip"), &dir.join("m")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A time for a mod's `last_played` marker: `n` seconds after a fixed start, so the tests
    /// order the markers without relying on the clock.
    fn played_at(n: u64) -> web_time::SystemTime {
        web_time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000 + n)
    }

    /// Sets mod `id`'s `last_played` marker to `at`.
    fn set_played(local: &Path, id: &str, at: web_time::SystemTime) {
        let path = mod_dir(local, id).join(LAST_PLAYED);
        std::fs::OpenOptions::new().write(true).open(&path).unwrap().set_modified(at).unwrap();
    }

    #[test]
    fn keeping_recent_game_folders_removes_the_others_and_never_the_saves() {
        let base = base_install("keep");
        let local = temp("keep-hub");
        install_mod(&local, "a", "1.0", &[("Data/Units.ini", "A units")]);
        install_mod(&local, "b", "1.0", &[("Data/Extra.ini", "B extra")]);
        let game_a = use_mod(&local, &base, Some("a")).unwrap();
        let game_b = use_mod(&local, &base, Some("b")).unwrap();
        set_played(&local, "a", played_at(1));
        set_played(&local, "b", played_at(2));
        // The saves of a's folder: the app's saves root, never inside a game folder.
        let saves_root = temp("keep-saves");
        let saves = crate::rules::save::saves_dir_of(&saves_root, &game_a);
        put(&saves.join("slot1.rzsave"), "a save");
        assert!(!saves.starts_with(&game_a), "saves are not inside the game folder");
        assert!(!saves.starts_with(mod_dir(&local, "a")), "nor inside the mod's folder");

        // One kept: b was played last (and is the active mod).
        keep_recent_game_folders(&local, 1);
        assert!(!game_a.exists(), "a's built folder goes");
        assert!(game_b.join("Data/Extra.ini").exists(), "b's built folder stays");
        assert!(mod_files_dir(&local, "a").join("Data/Units.ini").exists(), "a's files stay: a rebuilds on the next play");
        assert_eq!(std::fs::read_to_string(saves.join("slot1.rzsave")).unwrap(), "a save", "saves are not deleted");
        assert_eq!(std::fs::read_to_string(base.join("Data/Units.ini")).unwrap(), "base units", "the base is not touched");

        // Playing a again rebuilds its folder, with the same saves path (same folder path).
        let again = prepare_mod(&local, &base, "a").unwrap();
        assert_eq!(again, game_a);
        assert_eq!(crate::rules::save::saves_dir_of(&saves_root, &again), saves, "the saves key is the same after a rebuild");

        // The base game chosen: b, the last played, still stays; a stays gone.
        use_mod(&local, &base, None).unwrap();
        keep_recent_game_folders(&local, 1);
        assert!(game_b.join("Data/Extra.ini").exists(), "b stays: the last played mod");
        assert!(!game_a.exists(), "a's built folder stays gone");
        assert!(saves.join("slot1.rzsave").exists(), "saves still there");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
        let _ = std::fs::remove_dir_all(&saves_root);
    }

    #[test]
    fn the_two_last_played_mod_folders_are_kept() {
        let base = base_install("two");
        let local = temp("two-hub");
        install_mod(&local, "a", "1.0", &[("Data/Units.ini", "A units")]);
        install_mod(&local, "b", "1.0", &[("Data/Extra.ini", "B extra")]);
        install_mod(&local, "c", "1.0", &[("Data/More.ini", "C more")]);
        let game_a = use_mod(&local, &base, Some("a")).unwrap();
        let game_b = use_mod(&local, &base, Some("b")).unwrap();
        let game_c = use_mod(&local, &base, Some("c")).unwrap();
        // The markers say the order a, b, c whatever the clock did.
        set_played(&local, "a", played_at(1));
        set_played(&local, "b", played_at(2));
        set_played(&local, "c", played_at(3));

        keep_recent_game_folders(&local, 2);
        assert!(!game_a.exists(), "a was played first: its folder goes");
        assert!(game_b.join("Data/Extra.ini").exists(), "b, played second, stays");
        assert!(game_c.join("Data/More.ini").exists(), "c, played last and active, stays");
        assert!(mod_files_dir(&local, "a").join("Data/Units.ini").exists(), "a's files stay: a rebuilds on the next play");

        // The base game chosen: the two newest still stay.
        use_mod(&local, &base, None).unwrap();
        keep_recent_game_folders(&local, 2);
        assert!(game_b.join("Data/Extra.ini").exists(), "b stays with the base chosen");
        assert!(game_c.join("Data/More.ini").exists(), "c stays with the base chosen");
        assert_eq!(std::fs::read_to_string(base.join("Data/Units.ini")).unwrap(), "base units", "the base is not touched");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// The first start after an update: the owned base converts its plain pictures on a thread while the
    /// active mod is built over it (the backlog's "Bonus46.lit: No such file", the mod dropped to the base
    /// game). The build waits for the conversion, so it builds, every base picture reads the same bytes, and
    /// the base holds only the `.lz4` forms afterwards.
    #[test]
    fn a_mod_builds_over_a_base_whose_pictures_are_being_converted() {
        let base = temp("converting-base");
        let mut pictures = Vec::new();
        for i in 0..24 {
            let rel = format!("Graphics/Windows/Bonus{i}.lit");
            let text = format!("picture {i} {}", "x".repeat(512 * 1024));
            put(&base.join(&rel), &text);
            pictures.push((rel, text));
        }
        put(&base.join("Data/Units.ini"), "base units");
        let local = temp("converting-hub");
        install_mod(&local, "carry-test", "1.0", &[("Data/Extra.ini", "mod extra")]);
        // Owned on this thread, as the test's own folder (the conversion checks ownership where it starts).
        crate::dt::stored::set_test_owned(Some(base.clone()));
        crate::dt::stored::compress_owned_in_background(&base);
        crate::dt::stored::set_test_owned(None);
        let game = prepare_mod(&local, &base, "carry-test").expect("the build waits for the conversion, then builds");
        assert!(!crate::dt::stored::wait_converted(&base), "the conversion has ended");
        assert_eq!(read_logical(&game, "Data/Extra.ini"), "mod extra");
        for (rel, text) in &pictures {
            assert_eq!(read_logical(&game, rel), *text, "{rel} reads the same bytes");
            assert!(base.join(format!("{rel}.lz4")).is_file() && !base.join(rel).exists(), "{rel} is converted");
        }
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// The race of the backlog, made deterministic: a base picture is converted between the listing of the
    /// base and its stat (a conversion that started while the build runs, which the wait does not cover).
    /// The first build fails on the moved file; the mod is built once more after it, so the player keeps it.
    #[test]
    fn a_picture_converted_during_the_build_is_built_again() {
        let base = base_install("moved");
        put(&base.join("Graphics/Windows/Bonus0.lit"), "bonus picture");
        let local = temp("moved-hub");
        install_mod(&local, "carry-test", "1.0", &[("Data/Extra.ini", "mod extra")]);
        BEFORE_STAT.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(|path: &Path| {
                if path.ends_with("Graphics/Windows/Bonus0.lit") {
                    crate::dt::stored::compress_file(path).expect("the picture converts");
                }
            }))
        });
        let used = use_mod(&local, &base, Some("carry-test"));
        BEFORE_STAT.with(|hook| *hook.borrow_mut() = None);
        let game = used.expect("the build is tried again after the picture moved");
        assert_eq!(read_logical(&game, "Graphics/Windows/Bonus0.lit"), "bonus picture");
        assert_eq!(read_logical(&game, "Data/Extra.ini"), "mod extra");
        assert_eq!(active_mod(&local).as_deref(), Some("carry-test"), "the mod stays the one played");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// Writes mod `id`'s manifest from its files as they are now (what an install writes).
    fn write_manifest_now(local: &Path, id: &str) {
        let files = file_entries(&mod_files_dir(local, id)).unwrap();
        write_manifest(local, id, &Manifest { files, sources: Vec::new() }).unwrap();
    }

    /// The decoded bytes of the logical file `rel` in the merged folder `game` (a plain or a stored one).
    fn read_logical(game: &Path, rel: &str) -> String {
        String::from_utf8(crate::dt::stored::find_path(game, rel).unwrap().read().unwrap()).unwrap()
    }

    /// The names in `dir`, sorted.
    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    /// Writes the compressed picture `rel` of mod `id`'s files (a real `.lz4` frame, as the converter
    /// writes it) holding `text`.
    fn install_stored_picture(local: &Path, id: &str, rel: &str, text: &str) {
        let src = temp("stored-src");
        put(&src.join("picture.lit"), text);
        let lz4 = crate::dt::stored::compress_file(&src.join("picture.lit")).unwrap();
        let dst = mod_files_dir(local, id).join(rel);
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::rename(&lz4, &dst).unwrap();
        let _ = std::fs::remove_dir_all(&src);
    }

    /// Backlog 1609 part 2 (rule 6): a mod's plain `A.lit` replaces the base's stored `a.lit.lz4` in the
    /// merged folder (ignoring case), so the folder holds one form of the picture. The base keeps its file.
    /// The mod's picture is compressed at its play, so the folder holds the mod's `.lz4`.
    #[test]
    fn a_mods_picture_replaces_the_bases_stored_one() {
        let base = temp("stored-base");
        put(&base.join("Pictures/a.lit"), "picture");
        crate::dt::stored::compress_file(&base.join("Pictures/a.lit")).unwrap();
        assert!(base.join("Pictures/a.lit.lz4").is_file());
        let local = temp("stored-hub");
        install_mod(&local, "m1", "1.0", &[("Pictures/A.lit", "mod picture")]);
        let game = prepare_mod(&local, &base, "m1").unwrap();
        // The base's spelling of the name is kept (as for every file), and the folder holds one form.
        assert_eq!(names_in(&game.join("Pictures")), vec!["a.lit.lz4".to_string()], "the mod's stored picture is the only form");
        assert_eq!(read_logical(&game, "Pictures/a.lit"), "mod picture");
        // A second start keeps the folder as it is.
        prepare_mod(&local, &base, "m1").unwrap();
        assert_eq!(names_in(&game.join("Pictures")), vec!["a.lit.lz4".to_string()]);
        assert!(base.join("Pictures/a.lit.lz4").is_file(), "the base keeps its stored file");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// The other way round: a mod's stored `A.lit.lz4` replaces the base's plain picture, which is a hard
    /// link in the merged folder. The folder holds the mod's form only, and a lookup reads the mod's bytes.
    #[test]
    fn a_mods_stored_picture_replaces_the_bases_plain_one() {
        let base = base_install("stored-over-plain");
        let local = temp("stored-over-plain-hub");
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units")]);
        install_stored_picture(&local, "m1", "Pictures/A.lit.lz4", "mod picture");
        let game = prepare_mod(&local, &base, "m1").unwrap();
        assert_eq!(names_in(&game.join("Pictures")), vec!["A.lit.lz4".to_string()], "the base's plain link is gone");
        assert_eq!(read_logical(&game, "Pictures/a.lit"), "mod picture");
        assert_eq!(std::fs::read_to_string(base.join("Pictures/a.lit")).unwrap(), "picture", "the base keeps its file");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// Without hard links (the read-through layout) the same rule holds: the base's picture is not in the
    /// merged folder, and the lookup of the name reads the mod's stored picture, not the base's.
    #[test]
    fn without_hard_links_a_mods_stored_picture_is_the_one_read() {
        let base = base_install("stored-nolinks");
        let local = temp("stored-nolinks-hub");
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units")]);
        install_stored_picture(&local, "m1", "Pictures/A.lit.lz4", "mod picture");
        NO_LINKS.with(|n| n.set(true));
        let built = prepare_mod(&local, &base, "m1");
        NO_LINKS.with(|n| n.set(false));
        let game = built.unwrap();
        assert_eq!(names_in(&game.join("Pictures")), vec!["A.lit.lz4".to_string()]);
        let found = crate::dt::stored::find_path(&game, "Pictures/a.lit").unwrap();
        assert!(found.compressed && found.path.starts_with(&game), "the mod's stored picture, not the base's: {}", found.path.display());
        assert_eq!(found.read().unwrap(), b"mod picture");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// A mod installed before compression (plain pictures, a manifest of the plain files) is compressed at
    /// its first play: its folder is built once more, with the stored name, and its manifest still matches.
    /// Its next play keeps the folder. The kept zips of the older install are removed.
    #[test]
    fn a_plain_picture_of_an_older_install_is_compressed_at_its_play() {
        let base = base_install("convert");
        let local = temp("convert-hub");
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units"), ("Pictures/b.lit", "mod picture")]);
        write_manifest_now(&local, "m1");
        put(&mod_dir(&local, "m1").join("archives/0.zip"), "an old kept zip");
        let game = prepare_mod(&local, &base, "m1").unwrap();
        let files = mod_files_dir(&local, "m1");
        assert!(!files.join("Pictures/b.lit").exists() && files.join("Pictures/b.lit.lz4").is_file(), "compressed in the mod's folder");
        assert_eq!(read_logical(&game, "Pictures/b.lit"), "mod picture");
        assert!(damaged_files(&local, "m1").unwrap().is_empty(), "the manifest's logical names and digests still match");
        assert!(!mod_dir(&local, "m1").join("archives").exists(), "the kept zip of the older install goes");
        // The stamp was rebuilt once for the stored names; the next play keeps the folder.
        let path = game.join(STAMP);
        std::fs::OpenOptions::new().write(true).open(&path).unwrap().set_modified(played_at(7)).unwrap();
        prepare_mod(&local, &base, "m1").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), played_at(7), "the folder is kept");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// A stored picture that does not decode is damage: it is listed, and no folder is built from the mod.
    #[test]
    fn a_stored_picture_that_does_not_decode_is_damaged() {
        let base = base_install("stored-damaged");
        let local = temp("stored-damaged-hub");
        install_mod(&local, "m1", "1.0", &[("Pictures/b.lit", "mod picture")]);
        write_manifest_now(&local, "m1");
        prepare_mod(&local, &base, "m1").unwrap();
        std::fs::write(mod_files_dir(&local, "m1").join("Pictures/b.lit.lz4"), b"not a frame").unwrap();
        assert_eq!(damaged_files(&local, "m1").unwrap(), ["Pictures/b.lit"]);
        std::fs::remove_dir_all(mod_game_dir(&local, "m1")).unwrap();
        assert!(prepare_mod(&local, &base, "m1").is_err());
        assert!(!mod_game_dir(&local, "m1").exists(), "no folder is built from a damaged mod");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// Where the file system refuses hard links (Android's app storage), the merged folder holds only
    /// the mod's files and the folders: the base's files are not copied (a second game per mod), and
    /// every lookup that misses there reads the base. The mod's files still win, and listings show both.
    #[test]
    fn without_hard_links_the_base_is_read_through_not_copied() {
        NO_LINKS.with(|n| n.set(true));
        let base = base_install("nolinks");
        put(&base.join("Maps_Rus/Base.DTm"), "base map");
        put(&base.join("Pictures/b.lit"), "base picture b");
        crate::dt::stored::compress_file(&base.join("Pictures/b.lit")).unwrap();
        let local = temp("nolinks-hub");
        install_mod(&local, "m1", "1.0", &[("data/units.ini", "mod units"), ("Maps_Rus/Mod.DTm", "mod map")]);
        let game = prepare_mod(&local, &base, "m1").unwrap();
        NO_LINKS.with(|n| n.set(false));
        // Nothing of the base is in the merged folder, only its folders and the mod's files.
        assert!(!game.join("Pictures/a.lit").exists() && !game.join("Pictures/b.lit.lz4").exists(), "the base's files are not copied");
        assert!(game.join("Pictures").is_dir(), "the base's folders are there");
        // Lookups read through to the base; the mod's file wins.
        let find = |rel: &str| crate::dt::install::find_path(&game, rel).unwrap();
        assert_eq!(std::fs::read_to_string(find("Data/Units.ini")).unwrap(), "mod units");
        assert_eq!(std::fs::read_to_string(find("Pictures/A.LIT")).unwrap(), "picture", "the base's plain picture, ignoring case");
        let b = crate::dt::stored::find_path(&game, "Pictures/b.lit").unwrap();
        assert!(b.compressed && b.path.starts_with(&base), "the base's stored picture");
        assert_eq!(b.read().unwrap(), b"base picture b");
        let pictures: Vec<String> = crate::dt::stored::list(&game.join("Pictures")).into_iter().map(|(n, _)| n).collect();
        assert_eq!(pictures, vec!["a.lit".to_string(), "b.lit".to_string()], "a listing shows the base's pictures");
        let maps: Vec<String> = crate::dt::install::list_maps(&game).unwrap().into_iter().map(|m| m.name).collect();
        assert_eq!(maps, vec!["Base".to_string(), "Mod".to_string()], "the maps of both");
        // A second start keeps the folder (no rebuild loop on the files it does not hold).
        let stamp = std::fs::read(game.join(STAMP)).unwrap();
        NO_LINKS.with(|n| n.set(true));
        prepare_mod(&local, &base, "m1").unwrap();
        NO_LINKS.with(|n| n.set(false));
        assert_eq!(std::fs::read(game.join(STAMP)).unwrap(), stamp, "the folder is kept");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// A merged folder built before the read-through layout (its base files copied where links
    /// failed) is rebuilt once: its stamp has no layout.
    #[test]
    fn a_folder_of_the_old_layout_is_rebuilt_once() {
        let base = base_install("layout");
        let local = temp("layout-hub");
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units")]);
        let game = prepare_mod(&local, &base, "m1").unwrap();
        let mut stamp: serde_json::Value = serde_json::from_slice(&std::fs::read(game.join(STAMP)).unwrap()).unwrap();
        stamp.as_object_mut().unwrap().remove("layout");
        std::fs::write(game.join(STAMP), serde_json::to_vec(&stamp).unwrap()).unwrap();
        prepare_mod(&local, &base, "m1").unwrap();
        let again: serde_json::Value = serde_json::from_slice(&std::fs::read(game.join(STAMP)).unwrap()).unwrap();
        assert_eq!(again["layout"], LAYOUT, "rebuilt with the current layout");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_missing_file_in_the_merged_folder_forces_a_rebuild() {
        let base = base_install("gone-game");
        let local = temp("gone-game-hub");
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units")]);
        let game = prepare_mod(&local, &base, "m1").unwrap();
        // The antivirus takes the base's picture out of the merged folder (a link, not the base's file).
        std::fs::remove_file(game.join("Pictures/a.lit")).unwrap();
        prepare_mod(&local, &base, "m1").unwrap();
        assert_eq!(std::fs::read_to_string(game.join("Pictures/a.lit")).unwrap(), "picture", "the folder is built again");
        assert_eq!(std::fs::read_to_string(base.join("Pictures/a.lit")).unwrap(), "picture", "the base keeps its file");
        // A mod file taken from the folder: rebuilt too.
        std::fs::remove_file(game.join("Data/Units.ini")).unwrap();
        prepare_mod(&local, &base, "m1").unwrap();
        assert_eq!(std::fs::read_to_string(game.join("Data/Units.ini")).unwrap(), "mod units");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_damaged_mod_file_is_refused_and_no_folder_is_built() {
        let base = base_install("damaged");
        let local = temp("damaged-hub");
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units"), ("Scripts/New.txt", "new")]);
        write_manifest_now(&local, "m1");
        assert!(damaged_files(&local, "m1").unwrap().is_empty(), "an intact mod is not damaged");
        assert!(prepare_mod(&local, &base, "m1").is_ok());
        std::fs::remove_file(mod_files_dir(&local, "m1").join("Data/Units.ini")).unwrap();
        std::fs::remove_dir_all(mod_game_dir(&local, "m1")).unwrap();
        assert_eq!(damaged_files(&local, "m1").unwrap(), ["Data/Units.ini"]);
        let err = prepare_mod(&local, &base, "m1").unwrap_err();
        assert!(err.contains("Data/Units.ini"), "{err}");
        assert!(!mod_game_dir(&local, "m1").exists(), "no folder is built from a damaged mod");
        // A changed file (same name, other bytes) is damaged too.
        put(&mod_files_dir(&local, "m1").join("Scripts/New.txt"), "changed!");
        assert_eq!(damaged_files(&local, "m1").unwrap(), ["Data/Units.ini", "Scripts/New.txt"]);
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn a_mod_without_a_manifest_is_damaged_only_where_its_files_do_not_read() {
        let base = base_install("legacy");
        let local = temp("legacy-hub");
        install_mod(&local, "m1", "1.0", &[("Data/Units.ini", "mod units")]);
        assert!(read_manifest(&local, "m1").is_none());
        assert!(damaged_files(&local, "m1").unwrap().is_empty(), "readable files are trusted");
        assert!(prepare_mod(&local, &base, "m1").is_ok());
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
    }
}