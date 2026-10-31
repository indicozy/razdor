//! Installed mods as layers over the base install. A mod's files live under
//! `<hub>/mods/<id>/files/` at their paths inside the mod. [`prepare_mod`] builds the mod's
//! merged game folder `<hub>/mods/<id>/game/`: every base file hard-linked (a copy where links
//! fail), then each mod file laid over the base file it replaces. The game reads the active
//! mod's folder ([`active_install_dir`]); the choice is `<hub>/active.json`.
//!
//! The base install is never written: a link is removed before a file replaces it. A folder
//! is rebuilt when the base's files (names, sizes, times), the mod's version or its file list
//! and contents change (`stamp.json` in the folder).

use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{read_installed, safe_id, safe_rel_name, Installed, Kind, Text};

/// The file naming the active mod, under the hub folder.
const ACTIVE: &str = "active.json";
/// The file recording how a merged game folder was built, inside it.
const STAMP: &str = "stamp.json";

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

/// Makes `id` the mod the game plays from now on (`None`: the base game), after its folder is
/// built. Returns the folder the game reads.
pub fn use_mod(local: &Path, base: &Path, id: Option<&str>) -> Result<PathBuf, String> {
    match id {
        None => {
            write_active(local, None)?;
            Ok(base.to_path_buf())
        }
        Some(id) => {
            let dir = prepare_mod(local, base, id)?;
            write_active(local, Some(id))?;
            Ok(dir)
        }
    }
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

fn active_install_dir_in(local: &Path, base: &Path) -> PathBuf {
    let Some(id) = active_mod(local) else { return base.to_path_buf() };
    if read_installed(local).entries.get(&id).is_none_or(|e| e.kind != Kind::Mod) {
        crate::diag!("hub: the active mod {id} is not installed; the base game is played");
        let _ = write_active(local, None);
        return base.to_path_buf();
    }
    match prepare_mod(local, base, &id) {
        Ok(dir) => dir,
        Err(e) => {
            crate::diag!("hub: the active mod {id} does not build ({e}); the base game is played");
            let _ = write_active(local, None);
            base.to_path_buf()
        }
    }
}

/// Whether a hub map is listed in the new-game list (the rule of 1445). With no mod played
/// (`chain` empty) the base game's maps: a hub map that needs no mod. With a mod played, its
/// `chain` (the mod and the mods it is laid over, [`layer_chain`]): the hub maps that need one of
/// them; a map made for the base game is not the mod's. A map the install already has (the same
/// bytes, `same_as_install`) is listed once, from the install, so it is never listed here.
pub fn map_listed(requires: &[String], chain: &[String], same_as_install: bool) -> bool {
    let for_this = if chain.is_empty() { requires.is_empty() } else { requires.iter().any(|r| chain.contains(r)) };
    for_this && !same_as_install
}

/// The maps of the install, for the duplicate check of [`map_listed`]: each file's size, read
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

/// The maps of the hub's folder the new-game list shows, by [`map_listed`]: the mod played and the
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

    /// Whether the map `name` (its file name, or the stem the new-game list gives) is listed,
    /// given whether it is the install's (`same_as_install`, see [`InstallMaps::has_same`]).
    pub fn listed(&self, name: &str, same_as_install: bool) -> bool {
        let requires = self.needs.get(&stem_key(name)).map_or(&[][..], Vec::as_slice);
        map_listed(requires, &self.chain, same_as_install)
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
}

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
    let installed = read_installed(local);
    let order = layer_chain(&installed, id)?;
    let mut layers = Vec::with_capacity(order.len());
    let mut stamps = Vec::with_capacity(order.len());
    for lid in &order {
        let version = installed.entries[lid].version.clone();
        let files = list_mod_files(&mod_files_dir(local, lid))?;
        let mut digests = Vec::with_capacity(files.len());
        for (rel, path) in &files {
            let bytes = std::fs::read(path).map_err(|e| format!("{lid}: {rel}: {e}"))?;
            digests.push((rel.clone(), crate::update::sha256_hex(&bytes)));
        }
        stamps.push(LayerStamp { id: lid.clone(), version, files: digests });
        layers.push(files);
    }
    let stamp = Stamp { base: base.display().to_string(), base_files: base_fingerprint(base)?, layers: stamps };
    let game = mod_game_dir(local, id);
    if read_stamp(&game).as_ref() == Some(&stamp) {
        return Ok(game);
    }
    let tmp = mod_dir(local, id).join("game.tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    let built = build(base, &layers, &tmp).and_then(|()| {
        let json = serde_json::to_vec_pretty(&stamp).map_err(|e| e.to_string())?;
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

/// Removes mod `id`'s built game folder (and a half-built one). Its `files/` stay, so playing
/// the mod again rebuilds it. Saves are never in it (`save::saves_dir_of`).
pub fn drop_game_folder(local: &Path, id: &str) {
    if !safe_id(id) {
        return;
    }
    let _ = std::fs::remove_dir_all(mod_game_dir(local, id));
    let _ = std::fs::remove_dir_all(mod_dir(local, id).join("game.tmp"));
}

/// Keeps only the built game folder of `keep` (`None`: the base game, so none): every other
/// mod's game folder under `mods/` goes. Each is a full copy of the base where the hub is on
/// another disk than the install, so only the one in use is kept.
pub fn keep_only_game_folder(local: &Path, keep: Option<&str>) {
    let Ok(read) = std::fs::read_dir(local.join("mods")) else { return };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if Some(name.as_str()) != keep && entry.path().is_dir() {
            drop_game_folder(local, &name);
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

/// Builds the merged folder `tmp` from `base` and the layers: each layer's files (`(path in the
/// mod, source)`) laid in turn, a later layer's file replacing an earlier one at the same path
/// (ignoring case).
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
        if std::fs::hard_link(&path, &target).is_err() {
            std::fs::copy(&path, &target).map_err(|e| format!("{rel}: {e}"))?;
        }
    }
    for mod_files in layers {
        let mut seen = HashSet::new();
        for (rel, src) in mod_files {
            if !seen.insert(rel.to_ascii_lowercase()) {
                return Err(format!("{rel}: two files of one mod differ only in case"));
            }
            let target = resolve(&mut known, rel);
            let dst = tmp.join(&target);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{target}: {e}"))?;
            }
            // Breaks the link to the base's file (or an earlier layer's copy) first; the base keeps its own.
            let _ = std::fs::remove_file(&dst);
            std::fs::copy(src, &dst).map_err(|e| format!("{target}: {e}"))?;
        }
    }
    Ok(())
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

/// The mod's files under `files_dir`, as (path in the mod, file), sorted. A name that is not
/// safe is an error; a folder with no files is an empty mod.
fn list_mod_files(files_dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    if !files_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut all = Vec::new();
    walk(files_dir, files_dir, &mut all)?;
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for (rel, path, is_dir) in all {
        if is_dir {
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

/// A digest of the base's files: their names, sizes and modification times.
fn base_fingerprint(base: &Path) -> Result<String, String> {
    let mut all = Vec::new();
    walk(base, base, &mut all)?;
    let mut lines = Vec::new();
    for (rel, path, is_dir) in all {
        if is_dir {
            continue;
        }
        let meta = std::fs::metadata(&path).map_err(|e| format!("{rel}: {e}"))?;
        let time = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
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
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
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
        put(&game.join("marker.txt"), "kept");
        prepare_mod(&local, &base, "m1").unwrap();
        assert!(game.join("marker.txt").exists(), "an unchanged folder is not rebuilt");
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
            },
        );
        installed.entries.insert(
            "ford".into(),
            super::super::InstalledEntry { version: "1".into(), kind: Kind::Map, lang: String::new(), title: Text::default(), requires: vec![], files: vec!["maps/Hub_Ford.DTm".into()] },
        );
        installed.entries.insert("evo".into(), super::super::InstalledEntry { version: "1".into(), kind: Kind::Mod, lang: String::new(), title: Text::default(), requires: vec![], files: vec![] });
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(local.join("installed.json"), serde_json::to_vec(&installed).unwrap()).unwrap();
        assert!(!MapGate::read(&local).listed("Hub_Rotten.DTm", false), "no mod is active");
        assert!(!MapGate::read(&local).listed("hub_rotten", false), "the new-game list names a map by its stem, in any case");
        assert!(MapGate::read(&local).listed("Hub_Ford.DTm", false), "a map for the base game shows with no mod");
        assert!(MapGate::read(&local).listed("Install_Map.DTm", false), "a map the hub did not install is a base map");
        write_active(&local, Some("evo")).unwrap();
        assert!(MapGate::read(&local).listed("Hub_Rotten.DTm", false));
        assert!(!MapGate::read(&local).listed("Hub_Ford.DTm", false), "a mod is chosen: the base game's hub map is hidden");
        write_active(&local, Some("other")).unwrap();
        assert!(!MapGate::read(&local).listed("Hub_Rotten.DTm", false), "another mod does not show it");
        let _ = std::fs::remove_dir_all(&local);
    }

    /// The rule of 1445, as a pure function: the base game shows the maps that need no mod; a
    /// chosen mod shows the maps that need it or a mod it is laid over; an identical copy of an
    /// install map is never listed twice.
    #[test]
    fn the_new_game_list_shows_the_base_maps_or_the_mods_maps_once() {
        let none: &[String] = &[];
        let evo = vec!["evolution".to_string()];
        let builds_on = vec!["evolution".to_string(), "dt-orders".to_string()];
        assert!(map_listed(none, none, false), "base: a base map is listed");
        assert!(!map_listed(&evo, none, false), "base: a map that needs Evolution is hidden");
        assert!(map_listed(&evo, &evo, false), "Evolution chosen: its own map is listed");
        assert!(!map_listed(none, &evo, false), "Evolution chosen: a base map is hidden (the base copy of the map)");
        assert!(!map_listed(&["other".to_string()], &evo, false), "Evolution chosen: another mod's map is hidden");
        assert!(map_listed(&evo, &builds_on, false), "a mod built on Evolution: Evolution's map is listed");
        assert!(!map_listed(none, none, true), "base: the hub's copy of an install map is not listed twice");
        assert!(!map_listed(&evo, &evo, true), "Evolution chosen: an identical copy is not listed twice");
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
        assert_eq!(std::fs::read_to_string(game.join("Pictures/a.lit")).unwrap(), "A-pic", "A's file overrides the base's, B has none");
        assert_eq!(std::fs::read_to_string(game.join("Scripts/a.txt")).unwrap(), "a", "A's own file is there");
        assert_eq!(std::fs::read_to_string(game.join("Scripts/b.txt")).unwrap(), "b", "B's own file is there");
        assert_eq!(std::fs::read_to_string(base.join("Data/Units.ini")).unwrap(), "base units", "the base is not written");
        // A changes: B's folder is rebuilt, B's file still wins.
        put(&game.join("marker.txt"), "stale");
        install_mod(&local, "a", "1.1", &[("Data/Units.ini", "A units"), ("Pictures/a.lit", "A-pic-2"), ("Scripts/a.txt", "a2")]);
        let game = prepare_mod(&local, &base, "b").unwrap();
        assert!(!game.join("marker.txt").exists(), "a change in A rebuilds B's folder");
        assert_eq!(std::fs::read_to_string(game.join("Pictures/a.lit")).unwrap(), "A-pic-2");
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

    #[test]
    fn keeping_one_game_folder_removes_the_others_and_never_the_saves() {
        let base = base_install("keep");
        let local = temp("keep-hub");
        install_mod(&local, "a", "1.0", &[("Data/Units.ini", "A units")]);
        install_mod(&local, "b", "1.0", &[("Data/Extra.ini", "B extra")]);
        let game_a = prepare_mod(&local, &base, "a").unwrap();
        let game_b = prepare_mod(&local, &base, "b").unwrap();
        // The saves of a's folder: the app's saves root, never inside a game folder.
        let saves_root = temp("keep-saves");
        let saves = crate::rules::save::saves_dir_of(&saves_root, &game_a);
        put(&saves.join("slot1.rzsave"), "a save");
        assert!(!saves.starts_with(&game_a), "saves are not inside the game folder");
        assert!(!saves.starts_with(mod_dir(&local, "a")), "nor inside the mod's folder");

        keep_only_game_folder(&local, Some("b"));
        assert!(!game_a.exists(), "a's built folder goes");
        assert!(game_b.join("Data/Extra.ini").exists(), "b's built folder stays");
        assert!(mod_files_dir(&local, "a").join("Data/Units.ini").exists(), "a's files stay: a rebuilds on the next play");
        assert_eq!(std::fs::read_to_string(saves.join("slot1.rzsave")).unwrap(), "a save", "saves are not deleted");
        assert_eq!(std::fs::read_to_string(base.join("Data/Units.ini")).unwrap(), "base units", "the base is not touched");

        // Playing a again rebuilds its folder, with the same saves path (same folder path).
        let again = prepare_mod(&local, &base, "a").unwrap();
        assert_eq!(again, game_a);
        assert_eq!(crate::rules::save::saves_dir_of(&saves_root, &again), saves, "the saves key is the same after a rebuild");

        keep_only_game_folder(&local, None);
        assert!(!game_a.exists() && !game_b.exists(), "the base game keeps no mod folder");
        assert!(saves.join("slot1.rzsave").exists(), "saves still there");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&local);
        let _ = std::fs::remove_dir_all(&saves_root);
    }
}
