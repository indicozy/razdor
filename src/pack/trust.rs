//! Which package files a save may open. A save names its package by id, version and hash, and
//! the path it was read from; that path is a hint at most. Razdor opens only a package the player
//! opened itself (`razdor --pack`, recorded in `packages.json` in Razdor's data folder), or one
//! in the Hub's `packages/` folder, and a candidate is checked against the save's hash when it is
//! opened (`ui::pack_play`). A save never makes Razdor read a path of its own.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::files::refused_path;
use super::open::Opened;
use crate::rules::save::ScenarioRef;

/// The list's file name, in Razdor's data folder ([`crate::platform::app_dir`]).
pub const LIST_FILE: &str = "packages.json";
/// The Hub's folder of installed packages, under the Hub's folder ([`crate::hub::local_dir`]).
pub const HUB_PACKAGES_DIR: &str = "packages";
/// The most a list file may hold (read back at a load; a larger one is ignored).
const MAX_LIST_BYTES: u64 = 1 << 20;
/// The most packages the list keeps (the newest first).
const MAX_LISTED: usize = 512;
/// The most packages read from the Hub's folder for one load.
const MAX_HUB_PACKAGES: usize = 256;

/// A package the player opened: its canonical path, id, version and content hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub id: String,
    pub version: String,
    pub hash: u64,
}

impl Entry {
    /// The entry of an opened package.
    pub fn from_opened(opened: &Opened) -> Entry {
        Entry { path: opened.path.display().to_string(), id: opened.id.clone(), version: opened.version.clone(), hash: opened.hash }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct List {
    packages: Vec<Entry>,
}

/// `path` as a local canonical path: refused when it is a network or device path (before the
/// file system is touched), and refused again when it resolves to one. `Err` names the problem.
pub fn canonical_local(path: &Path) -> Result<PathBuf, String> {
    if let Some(why) = refused_path(&path.to_string_lossy()) {
        return Err(format!("{} is {why}: a package is read from this computer only", path.display()));
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| format!("no such package folder or file: {}", path.display()))?;
    match refused_path(&canonical.to_string_lossy()) {
        Some(why) => Err(format!("{} is {why}: a package is read from this computer only", path.display())),
        None => Ok(canonical),
    }
}

/// The packages in the list `dir/packages.json`, newest first. A missing, too large or unreadable
/// list is empty; an entry whose path is not local is dropped.
pub fn read(dir: &Path) -> Vec<Entry> {
    let path = dir.join(LIST_FILE);
    if std::fs::metadata(&path).map_or(true, |m| m.len() > MAX_LIST_BYTES) {
        return Vec::new();
    }
    let list: List = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    list.packages.into_iter().filter(|e| refused_path(&e.path).is_none()).collect()
}

/// Records a package the player opened in `dir/packages.json`, replacing an entry at the same
/// path. The path must be local: the entry is stored canonical.
pub fn remember(dir: &Path, entry: Entry) -> Result<(), String> {
    let path = canonical_local(Path::new(&entry.path))?;
    let entry = Entry { path: path.display().to_string(), ..entry };
    let mut packages = read(dir);
    packages.retain(|e| e.path != entry.path);
    packages.insert(0, entry);
    packages.truncate(MAX_LISTED);
    let json = serde_json::to_vec_pretty(&List { packages }).map_err(|e| format!("{LIST_FILE}: {e}"))?;
    crate::fsutil::write_atomically(&dir.join(LIST_FILE), &json).map_err(|e| format!("{LIST_FILE}: {e}"))
}

/// The packages in the Hub's folder `local/packages/` (folders and `.rzpack` files), as local
/// canonical paths. Nothing is read from them here; a candidate is opened and checked by hash.
pub fn hub_packages(local: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(local.join(HUB_PACKAGES_DIR)) else { return Vec::new() };
    let mut out: Vec<PathBuf> = read
        .flatten()
        .filter_map(|e| canonical_local(&e.path()).ok())
        .filter(|p| p.is_dir() || p.extension().is_some_and(|x| x.eq_ignore_ascii_case("rzpack")))
        .collect();
    out.sort();
    out.truncate(MAX_HUB_PACKAGES);
    out
}

/// The paths a save's package may be read from, in the order they are tried: the list's entries
/// with the save's id, version and hash, the entry whose stored path equals the save's path
/// first (a text comparison only: the save's path is never canonicalised or touched), then the
/// others; then the Hub's packages. Nothing else. Each candidate is opened and must match the
/// save ([`Opened::same_package`]).
pub fn candidates(list: &[Entry], origin: &ScenarioRef, hub: &[PathBuf]) -> Vec<PathBuf> {
    let ScenarioRef::Pack { id, version, hash, path, .. } = origin else { return Vec::new() };
    let matching: Vec<&Entry> = list.iter().filter(|e| e.id == *id && e.version == *version && e.hash == *hash).collect();
    let mut out: Vec<PathBuf> = Vec::new();
    for e in matching.iter().filter(|e| e.path == *path).chain(matching.iter()) {
        let p = PathBuf::from(&e.path);
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out.extend(hub.iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::open::test_support::{opened, story_package};
    use crate::pack::Files;

    fn temp(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-trust-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pack_origin(path: &str, hash: u64) -> ScenarioRef {
        ScenarioRef::Pack { id: "story-test".into(), version: "0.1.0".into(), hash, path: path.into(), map: "m".into() }
    }

    fn entry(path: &str, hash: u64) -> Entry {
        Entry { path: path.into(), id: "story-test".into(), version: "0.1.0".into(), hash }
    }

    #[test]
    fn a_save_whose_path_is_not_in_the_list_finds_no_candidate() {
        let origin = pack_origin("/elsewhere/evil.rzpack", 7);
        assert!(candidates(&[], &origin, &[]).is_empty(), "the save's own path is never read");
        // A listed package with another hash is no match either.
        let list = [entry("/packs/story", 8)];
        assert!(candidates(&list, &origin, &[]).is_empty());
    }

    #[test]
    fn a_listed_package_with_the_saves_hash_is_the_candidate_and_the_save_path_is_not() {
        let dir = std::env::temp_dir();
        let listed = dir.join("razdor-trust-listed.rzpack");
        let origin = pack_origin("/elsewhere/evil.rzpack", 7);
        let list = [entry(&listed.display().to_string(), 7)];
        assert_eq!(candidates(&list, &origin, &[]), vec![PathBuf::from(&list[0].path)]);
    }

    #[test]
    fn a_listed_entry_equal_to_the_save_path_goes_first() {
        let origin = pack_origin("/packs/b", 7);
        let list = [entry("/packs/a", 7), entry("/packs/b", 7), entry("/packs/c", 9)];
        assert_eq!(candidates(&list, &origin, &[]), vec![PathBuf::from("/packs/b"), PathBuf::from("/packs/a")]);
        // No equal path: the matching entries keep the list's order.
        let origin = pack_origin("/packs/x", 7);
        assert_eq!(candidates(&list, &origin, &[]), vec![PathBuf::from("/packs/a"), PathBuf::from("/packs/b")]);
    }

    #[test]
    fn a_save_path_is_compared_as_text_and_never_opened() {
        // A device-style hint only orders the list's entries as text: it is never opened, and
        // the candidates are the list's stored paths alone.
        let origin = pack_origin(r"\\?\UNC\evil-host\share\pkg.rzpack", 7);
        let list = [entry("/packs/a", 7)];
        assert_eq!(candidates(&list, &origin, &[]), vec![PathBuf::from("/packs/a")]);
    }

    #[test]
    fn a_unc_save_path_is_never_a_candidate() {
        let origin = pack_origin(r"\\evil-host\share\pkg.rzpack", 7);
        let hub = vec![PathBuf::from("/hub/packages/pkg")];
        let cands = candidates(&[], &origin, &hub);
        assert!(cands.iter().all(|p| refused_path(&p.to_string_lossy()).is_none()), "{cands:?}");
        assert_eq!(cands, hub, "only the Hub's folder");
        assert!(canonical_local(Path::new(r"\\evil-host\share\pkg.rzpack")).is_err());
    }

    #[test]
    fn a_network_or_device_package_is_not_remembered() {
        let dir = temp("refuse");
        let mut opened = opened(&story_package());
        opened.path = PathBuf::from(r"\\evil-host\share\pkg.rzpack");
        assert!(remember(&dir, Entry::from_opened(&opened)).is_err());
        assert!(read(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_trusted_package_loads_from_the_list() {
        let dir = temp("loads");
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/hello-story");
        let files = Files::from_dir(&example).unwrap();
        let hash = crate::pack::open::package_hash(&files);
        let manifest = crate::pack::Package::parse(files).0.manifest.expect("the example has a manifest");
        let canonical = std::fs::canonicalize(&example).unwrap();
        let e = Entry { path: canonical.display().to_string(), id: manifest.id.clone(), version: manifest.version.clone(), hash };
        remember(&dir, e.clone()).unwrap();
        assert_eq!(read(&dir), vec![e.clone()]);

        // The save names the same package by a path of its own: the list's path is the candidate.
        let origin = ScenarioRef::Pack { id: e.id.clone(), version: e.version.clone(), hash, path: "relative/hint.rzpack".into(), map: "m".into() };
        let cands = candidates(&read(&dir), &origin, &[]);
        assert_eq!(cands, vec![canonical.clone()]);
        let loaded = Files::load(&cands[0]).unwrap();
        assert_eq!(crate::pack::open::package_hash(&loaded), hash);

        // Remembering it again keeps one entry.
        remember(&dir, e).unwrap();
        assert_eq!(read(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_list_that_is_too_large_is_ignored() {
        let dir = temp("big");
        std::fs::write(dir.join(LIST_FILE), vec![b' '; MAX_LIST_BYTES as usize + 1]).unwrap();
        assert!(read(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_hub_folder_lists_its_packages_only() {
        let local = temp("hub");
        let packages = local.join(HUB_PACKAGES_DIR);
        std::fs::create_dir_all(packages.join("story")).unwrap();
        std::fs::write(packages.join("other.rzpack"), b"zip").unwrap();
        std::fs::write(packages.join("notes.txt"), b"text").unwrap();
        let found = hub_packages(&local);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|p| p.is_dir() || p.extension().is_some_and(|x| x == "rzpack")));
        assert!(hub_packages(&temp("none")).is_empty());
        let _ = std::fs::remove_dir_all(&local);
    }
}
