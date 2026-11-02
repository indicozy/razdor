//! Content packages in the Hub (`kind = "package"`). A catalog entry installs its `.rzpack` into
//! `<hub>/packages/<id>-<version>.rzpack` after its SHA-256 and size are checked, then its
//! `razdor-pack check`; an older version goes only once the new one is there. A file the player
//! imports (`import_package`) is checked the same way but is marked as not checked by the Hub.
//! Both are recorded in `installed.json`, and both are remembered in the trusted list
//! (`pack::trust`) when the hub has a trust folder. The card plays the installed file
//! ([`Hub::play_path`]), as `razdor --pack` does.
//!
//! An unlisted entry (`status = "unlisted"`) is hidden from the list, but a copy the player has
//! stays shown and plays.

use std::path::{Path, PathBuf};

use super::{safe_path, Catalog, Entry, Hub, Installed, InstalledEntry, Kind, Status, Text};
use crate::pack::{self, trust, Diagnostic, Files, Package};

/// The most a package file may be: the read limit of `pack::files` (the catalog's rule is 50 MB).
pub const MAX_PACKAGE_BYTES: u64 = pack::files::MAX_PACKAGE_BYTES;

/// Where the card's report button sends the player: the Discord invite of the player server,
/// whose reports thread the owner reads. The thread's own link is not in the repository.
pub const REPORTS_LINK: &str = "https://discord.gg/wmMksmWFDM";

/// The file name of a package in `<hub>/packages/`: `<id>-<version>.rzpack`.
pub fn package_file(id: &str, version: &str) -> String {
    format!("{id}-{version}.rzpack")
}

/// The package file's path relative to the hub folder (what `installed.json` records).
fn package_rel(id: &str, version: &str) -> String {
    format!("{}/{}", trust::HUB_PACKAGES_DIR, package_file(id, version))
}

/// Whether an entry is listed: an unlisted one is hidden, unless the player has it installed.
pub fn listed(e: &Entry, installed: &Installed) -> bool {
    e.status != Status::Unlisted || installed.has(&e.id)
}

/// The catalog with the packages imported from files added to its list (a record with its own
/// entry, whose id the catalog does not have). For the window only: nothing is installed from it.
pub fn with_imported(c: &Catalog, installed: &Installed) -> Catalog {
    let mut out = c.clone();
    for (id, rec) in &installed.entries {
        if let Some(e) = rec.entry.as_ref().filter(|_| out.entry(id).is_none()) {
            out.entries.push(e.clone());
        }
    }
    out
}

/// Why a package file is not imported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// The file cannot be read, written or is not a package (the reason).
    Failed(String),
    /// The package does not pass `razdor-pack check`: its diagnostics, in the order found.
    Refused(Vec<Diagnostic>),
}

/// The entry a package's card shows: its manifest and its texts in both languages (a missing
/// Russian title falls back to the English one, then to the id).
fn imported_entry(p: &Package) -> Option<Entry> {
    let m = p.manifest.as_ref()?;
    let text = |key: Option<&str>| -> Text {
        let get = |lang: &str| key.and_then(|k| p.texts.get(lang)?.get(k)).cloned().unwrap_or_default();
        let (ru, en) = (get("ru"), get("en"));
        Text { ru: if ru.is_empty() { en.clone() } else { ru }, en }
    };
    let mut title = text(Some(&m.title));
    if title.ru.is_empty() {
        title.ru = m.id.clone();
    }
    Some(Entry {
        id: m.id.clone(),
        kind: Kind::Package,
        title,
        author: m.author.clone(),
        version: m.version.clone(),
        description: text(m.description.as_deref()),
        about: Text::default(),
        languages: Vec::new(),
        machine_translated: false,
        requires: Vec::new(),
        source_url: String::new(),
        added: String::new(),
        status: Status::Unknown,
        status_note: Text::default(),
        preview: None,
        picture: None,
        base: m.base.clone(),
        rating: None,
        files: Vec::new(),
    })
}

impl Hub {
    /// Imports the `.rzpack` at `file` (the Hub's «Импорт пакета…», or a file given on the command
    /// line). It is checked as `razdor-pack check` checks it: an error refuses it, with the
    /// diagnostics. It is kept in `<hub>/packages/` under its id and version, and its card says it
    /// is not checked by the Hub. Returns the package's id.
    pub fn import_package(&self, file: &Path) -> Result<String, ImportError> {
        let failed = |why: String| ImportError::Failed(why);
        if !file.extension().is_some_and(|x| x.eq_ignore_ascii_case("rzpack")) {
            return Err(failed(format!("{}: not a .rzpack file", file.display())));
        }
        let size = std::fs::metadata(file).map(|m| m.len()).map_err(|e| failed(format!("{}: {e}", file.display())))?;
        if size > MAX_PACKAGE_BYTES {
            return Err(failed(format!("{}: bigger than the {} MB a package may be", file.display(), MAX_PACKAGE_BYTES >> 20)));
        }
        let files = Files::load(file).map_err(failed)?;
        let (package, _) = Package::parse(files.clone());
        let report = pack::check(&package);
        if report.has_errors() {
            return Err(ImportError::Refused(report.diagnostics));
        }
        let entry = imported_entry(&package).ok_or_else(|| failed(format!("{}: no manifest", file.display())))?;
        let bytes = std::fs::read(file).map_err(|e| failed(format!("{}: {e}", file.display())))?;
        let rel = self.store_package(&entry.id, &entry.version, &bytes, &files).map_err(failed)?;
        let mut installed = self.installed();
        installed.entries.insert(
            entry.id.clone(),
            InstalledEntry {
                version: entry.version.clone(),
                kind: Kind::Package,
                lang: String::new(),
                title: entry.title.clone(),
                requires: Vec::new(),
                files: vec![rel],
                unchecked: true,
                entry: Some(entry.clone()),
            },
        );
        self.save_installed(&installed).map_err(failed)?;
        Ok(entry.id)
    }

    /// The catalog's package `e`, its file in `part` (downloaded and checked by size and SHA-256):
    /// it must pass the check and be the id and version the catalog names. Written under its own
    /// name; returns the path the record keeps.
    pub(super) fn write_package(&self, e: &Entry, part: &Path) -> Result<Vec<String>, String> {
        let files = Files::load(part)?;
        let (package, _) = Package::parse(files.clone());
        let report = pack::check(&package);
        if report.has_errors() {
            return Err(format!("{}: the package does not pass the check ({} errors)", e.id, report.errors()));
        }
        let Some(m) = package.manifest.as_ref() else { return Err(format!("{}: no manifest", e.id)) };
        if m.id != e.id || m.version != e.version {
            return Err(format!("{}: the file is {} {}, the catalog says {} {}", e.id, m.id, m.version, e.id, e.version));
        }
        let bytes = std::fs::read(part).map_err(|err| format!("{}: {err}", e.id))?;
        Ok(vec![self.store_package(&e.id, &e.version, &bytes, &files)?])
    }

    /// Writes a checked package's bytes atomically under `<hub>/packages/`, and remembers it in the
    /// trusted list. Returns its path relative to the hub.
    fn store_package(&self, id: &str, version: &str, bytes: &[u8], files: &Files) -> Result<String, String> {
        let rel = package_rel(id, version);
        let path = self.local.join(&rel);
        std::fs::create_dir_all(self.local.join(trust::HUB_PACKAGES_DIR)).map_err(|e| format!("{rel}: {e}"))?;
        crate::editor::files::write_atomically(&path, bytes).map_err(|e| format!("{rel}: {e}"))?;
        if let Some(dir) = &self.trust_dir {
            let entry = trust::Entry { path: path.display().to_string(), id: id.to_string(), version: version.to_string(), hash: pack::open::package_hash(files) };
            if let Err(e) = trust::remember(dir, entry) {
                crate::diag!("packages: {e}");
            }
        }
        Ok(rel)
    }

    /// The installed package `id`'s file, to play (`pack::open`); None when the player has no
    /// package of that id.
    pub fn play_path(&self, id: &str) -> Option<PathBuf> {
        let rec = self.installed().entries.get(id).filter(|r| r.kind == Kind::Package)?.clone();
        let rel = rec.files.first().filter(|r| safe_path(r))?;
        Some(self.local.join(rel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-hubpack-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn example_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/hello-story")
    }

    /// The example package as a `.rzpack` file in `dir`.
    fn example_file(dir: &Path) -> PathBuf {
        let packed = crate::pack::archive::pack(&example_dir()).expect("the example packs");
        let file = dir.join("hello-story-as-sent.rzpack");
        std::fs::write(&file, &packed.bytes).unwrap();
        file
    }

    fn entry_for(file: &Path, sha: &str, size: u64) -> Entry {
        let files = Files::load(file).unwrap();
        let (package, _) = Package::parse(files);
        let mut e = imported_entry(&package).unwrap();
        e.kind = Kind::Package;
        e.files = vec![super::super::FileRef { path: "packages/hello-story-0.1.0.rzpack".into(), name: "hello-story-0.1.0.rzpack".into(), lang: None, size, sha256: sha.into(), archive: false }];
        e.status = Status::Full;
        e
    }

    fn hub_with_folder(tag: &str, dir: &Path) -> Hub {
        Hub { source: super::super::Source::Dir(dir.to_path_buf()), local: temp(tag), trust_dir: None }
    }

    #[test]
    fn an_import_is_kept_and_marked_unchecked_and_plays() {
        let dir = temp("import");
        let file = example_file(&dir);
        let hub = Hub { source: super::super::Source::Mock, local: temp("import-hub"), trust_dir: None };
        let id = hub.import_package(&file).expect("the example is a good package");
        assert_eq!(id, "hello-story");
        let rec = hub.installed().entries.get("hello-story").cloned().expect("recorded");
        assert!(rec.unchecked, "an imported package is marked not checked by the Hub");
        assert_eq!(rec.kind, Kind::Package);
        assert_eq!(rec.files, vec!["packages/hello-story-0.1.0.rzpack".to_string()]);
        assert_eq!(rec.entry.as_ref().map(|e| e.kind), Some(Kind::Package));
        let played = hub.play_path("hello-story").expect("the installed copy plays");
        assert!(played.is_file());
        assert_eq!(std::fs::read(&played).unwrap(), std::fs::read(&file).unwrap(), "the file is kept as it was");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&hub.local);
    }

    #[test]
    fn a_package_with_a_check_error_is_refused_with_its_diagnostics() {
        let dir = temp("refused");
        let bad = dir.join("broken.rzpack");
        let files = {
            let mut f = Files::default();
            f.insert("package.toml", b"id = \"broken\"\n".to_vec());
            f
        };
        std::fs::write(&bad, crate::pack::archive::zip(&files).unwrap()).unwrap();
        let hub = Hub { source: super::super::Source::Mock, local: temp("refused-hub"), trust_dir: None };
        match hub.import_package(&bad) {
            Err(ImportError::Refused(d)) => assert!(d.iter().any(|x| x.severity == crate::pack::Severity::Error), "{d:?}"),
            other => panic!("a broken package is refused, got {other:?}"),
        }
        assert!(!hub.installed().has("broken"), "nothing is recorded");
        assert!(!hub.local.join("packages").exists() || std::fs::read_dir(hub.local.join("packages")).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&hub.local);
    }

    #[test]
    fn a_catalog_install_with_a_bad_hash_is_refused_and_writes_nothing() {
        let dir = temp("badhash");
        let file = example_file(&dir);
        let bytes = std::fs::read(&file).unwrap();
        // A folder hub: the catalog names the package with a wrong SHA-256.
        let hubdir = temp("badhash-src");
        std::fs::create_dir_all(hubdir.join("packages")).unwrap();
        std::fs::copy(&file, hubdir.join("packages/hello-story-0.1.0.rzpack")).unwrap();
        let good = crate::update::sha256_hex(&bytes);
        let bad = format!("{}{}", &good[..63], if good.ends_with('0') { '1' } else { '0' });
        let e = entry_for(&file, &bad, bytes.len() as u64);
        let catalog = Catalog { format: 1, entries: vec![e] };
        let hub = hub_with_folder("badhash-hub", &hubdir);
        let err = hub.install(&catalog, "hello-story", crate::i18n::Lang::En).expect_err("the wrong hash is refused");
        assert!(err.contains("does not match the catalog"), "{err}");
        assert!(!hub.installed().has("hello-story"));
        assert!(!hub.local.join("packages/hello-story-0.1.0.rzpack").exists());
        for d in [dir, hubdir, hub.local.clone()] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_catalog_install_with_the_right_hash_is_kept_and_plays() {
        let dir = temp("goodhash");
        let file = example_file(&dir);
        let bytes = std::fs::read(&file).unwrap();
        let hubdir = temp("goodhash-src");
        std::fs::create_dir_all(hubdir.join("packages")).unwrap();
        std::fs::copy(&file, hubdir.join("packages/hello-story-0.1.0.rzpack")).unwrap();
        let e = entry_for(&file, &crate::update::sha256_hex(&bytes), bytes.len() as u64);
        let catalog = Catalog { format: 1, entries: vec![e] };
        let hub = hub_with_folder("goodhash-hub", &hubdir);
        assert_eq!(hub.install(&catalog, "hello-story", crate::i18n::Lang::En).unwrap(), vec!["hello-story".to_string()]);
        let rec = hub.installed().entries.get("hello-story").cloned().unwrap();
        assert!(!rec.unchecked, "a catalog package is the Hub's, checked");
        assert!(hub.play_path("hello-story").is_some_and(|p| p.is_file()));
        for d in [dir, hubdir, hub.local.clone()] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn an_unlisted_entry_is_hidden_but_a_copy_the_player_has_stays_listed() {
        let mut e = entry_for(&example_dir(), &"0".repeat(64), 1);
        e.status = Status::Unlisted;
        let none = Installed::default();
        assert!(!listed(&e, &none), "an unlisted entry is hidden from the list");
        let mut have = Installed::default();
        have.entries.insert(
            e.id.clone(),
            InstalledEntry { version: e.version.clone(), kind: Kind::Package, lang: String::new(), title: e.title.clone(), requires: vec![], files: vec![], unchecked: false, entry: None },
        );
        assert!(listed(&e, &have), "a package the player has still shows, and plays");
        e.status = Status::Full;
        assert!(listed(&e, &none));
    }

    #[test]
    fn an_imported_package_joins_the_list_once() {
        let dir = temp("joins");
        let file = example_file(&dir);
        let hub = Hub { source: super::super::Source::Mock, local: temp("joins-hub"), trust_dir: None };
        hub.import_package(&file).unwrap();
        let installed = hub.installed();
        let base = Catalog { format: 1, entries: vec![] };
        let shown = with_imported(&base, &installed);
        assert_eq!(shown.entries.len(), 1);
        assert_eq!(with_imported(&shown, &installed).entries.len(), 1, "an entry already listed is not added twice");
        for d in [dir, hub.local.clone()] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
