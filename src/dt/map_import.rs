//! Importing maps into the install's maps folder («Добавить карту…» in the new-game window).
//!
//! A picked `.DTm` is one map; a picked `.zip` gives every `.DTm` inside it, at any folder depth
//! (a campaign, or several maps). Only the file name of an entry counts: its folders never do,
//! so no entry can write outside the maps folder. The files are copied, so the original may be
//! deleted afterwards. Pure: it reads bytes and writes into a folder the caller names; the
//! window and the platform's picker are in `ui::map_import`.

use super::dtm::Scenario;
use super::install::{find_path, MAPS_DIR, MAP_EXTENSION};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

/// The largest map read out of an archive: a map is a few hundred KB, so the rest is not one.
const MAX_MAP_BYTES: u64 = 16 << 20;
/// An archive with more entries than this is refused (its list alone would be large).
const MAX_ENTRIES: usize = 4096;

/// A map read from a picked file: its bytes as they are (copied on import) and what they parse to.
#[derive(Clone, Debug)]
pub struct Map {
    /// The file name the map is written under (`Name.DTm`, its base name only).
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub scenario: Scenario,
}

/// Why a picked file gave no map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// Neither a `.DTm` nor a `.zip`.
    Unsupported,
    /// A `.DTm` that does not parse (its file name).
    NotAMap(String),
    /// A `.zip` that cannot be read (why).
    BadArchive(String),
    /// A `.zip` with no map in it.
    NoMaps,
}

/// The base name of a map inside `name` (a file name or an archive entry's path): `Name.DTm`,
/// the extension in the game's own spelling. `None` unless the name ends in `.dtm` (any case)
/// and has a base name of its own.
pub fn map_file_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    let (stem, ext) = base.rsplit_once('.')?;
    if !ext.eq_ignore_ascii_case(MAP_EXTENSION) || stem.trim().is_empty() || stem.contains('\0') || stem.starts_with('.') {
        return None;
    }
    Some(format!("{}.{MAP_EXTENSION}", stem.trim()))
}

fn extension_is(name: &str, ext: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case(ext))
}

/// The maps in a picked file named `name` with contents `bytes`.
pub fn read_picked(name: &str, bytes: &[u8]) -> Result<Vec<Map>, ImportError> {
    if extension_is(name, "zip") {
        return read_archive(bytes);
    }
    if !extension_is(name, MAP_EXTENSION) {
        return Err(ImportError::Unsupported);
    }
    let file_name = map_file_name(name).ok_or_else(|| ImportError::NotAMap(name.to_string()))?;
    let scenario = Scenario::from_file_bytes(bytes).map_err(|_| ImportError::NotAMap(name.to_string()))?;
    Ok(vec![Map { file_name, bytes: bytes.to_vec(), scenario }])
}

/// Every `.DTm` in the archive that parses as a map. An entry that is not one is skipped, and a
/// name seen before in the archive is skipped (the first one stays).
fn read_archive(bytes: &[u8]) -> Result<Vec<Map>, ImportError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| ImportError::BadArchive(e.to_string()))?;
    if zip.len() > MAX_ENTRIES {
        return Err(ImportError::BadArchive(format!("{} files", zip.len())));
    }
    let mut maps: Vec<Map> = Vec::new();
    for index in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(index) else { continue };
        if entry.is_dir() || entry.size() > MAX_MAP_BYTES {
            continue;
        }
        let Some(file_name) = entry.name().ok().and_then(|n| map_file_name(&n)) else { continue };
        let mut data = Vec::new();
        if entry.by_ref().take(MAX_MAP_BYTES + 1).read_to_end(&mut data).is_err() || data.len() as u64 > MAX_MAP_BYTES {
            continue;
        }
        let Ok(scenario) = Scenario::from_file_bytes(&data) else { continue };
        if maps.iter().any(|m| m.file_name.eq_ignore_ascii_case(&file_name)) {
            continue;
        }
        maps.push(Map { file_name, bytes: data, scenario });
    }
    if maps.is_empty() {
        return Err(ImportError::NoMaps);
    }
    Ok(maps)
}

/// The maps folder of the install at `install_dir` (matched ignoring case, as the game reads it).
pub fn maps_folder(install_dir: &Path) -> PathBuf {
    find_path(install_dir, MAPS_DIR).unwrap_or_else(|_| install_dir.join(MAPS_DIR))
}

/// The names of the files in `folder` that `maps` would replace (matched ignoring case, as the
/// game's folder may be on Windows or Android), spelled as the folder has them.
pub fn conflicts(folder: &Path, maps: &[Map]) -> Vec<String> {
    let existing = existing_names(folder);
    maps.iter().filter_map(|m| existing.iter().find(|e| e.eq_ignore_ascii_case(&m.file_name)).cloned()).collect()
}

fn existing_names(folder: &Path) -> Vec<String> {
    std::fs::read_dir(folder).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()
}

/// Copies `maps` into `folder` (created if missing). A map whose name is taken replaces that file
/// only with `replace`; otherwise it is left out. A copy goes through a temporary file that is
/// renamed over the target, so a failed copy leaves no half map. Returns the names written.
pub fn write_maps(folder: &Path, maps: &[Map], replace: bool) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let mut written = Vec::new();
    for (n, map) in maps.iter().enumerate() {
        // An existing file of the same name (in another case) is the one replaced.
        let existing = existing_names(folder).into_iter().find(|e| e.eq_ignore_ascii_case(&map.file_name));
        if existing.is_some() && !replace {
            continue;
        }
        let target = folder.join(existing.unwrap_or_else(|| map.file_name.clone()));
        let temp = folder.join(format!(".import-{}-{n}.tmp", std::process::id()));
        let copied = std::fs::write(&temp, &map.bytes).and_then(|()| std::fs::rename(&temp, &target));
        if let Err(e) = copied {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("{}: {e}", map.file_name));
        }
        written.push(target.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::Scenario;
    use std::io::Write;

    /// A minimal map that parses: a `w × h` grass scenario, as its payload.
    fn map_bytes(title: &str) -> Vec<u8> {
        let mut s = Scenario::default();
        s.header.width = 4;
        s.header.height = 4;
        s.terrain = vec![0; 16];
        s.title = title.to_string();
        s.to_payload()
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// A scratch folder of the test, removed with it (never the install's).
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("razdor-map-import-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_dtm_file_is_one_map_under_its_base_name() {
        let maps = read_picked("Downloads/My Map.dtm", &map_bytes("A")).unwrap();
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].file_name, "My Map.DTm");
        assert_eq!(maps[0].scenario.title, "A");
        assert_eq!(maps[0].bytes, map_bytes("A"), "the bytes are copied as they are");
    }

    #[test]
    fn a_file_that_is_not_a_map_is_refused_by_name() {
        assert!(matches!(read_picked("broken.DTm", b"not a map at all"), Err(ImportError::NotAMap(n)) if n == "broken.DTm"));
        assert!(matches!(read_picked("notes.txt", b"hello"), Err(ImportError::Unsupported)));
    }

    #[test]
    fn a_zip_gives_every_map_at_any_depth_and_skips_the_rest() {
        let zip = zip_of(&[
            ("campaign/first.DTm", &map_bytes("1")),
            ("campaign/deeper/second.dtm", &map_bytes("2")),
            ("readme.txt", b"not a map"),
            ("campaign/fake.DTm", b"garbage"),
            ("images/", b""),
        ]);
        let maps = read_picked("campaign.zip", &zip).unwrap();
        let names: Vec<&str> = maps.iter().map(|m| m.file_name.as_str()).collect();
        assert_eq!(names, ["first.DTm", "second.DTm"]);
        assert_eq!(maps[1].scenario.title, "2");
    }

    #[test]
    fn a_zip_with_no_map_is_refused() {
        let zip = zip_of(&[("readme.txt", b"nothing here")]);
        assert!(matches!(read_picked("empty.zip", &zip), Err(ImportError::NoMaps)));
        assert!(matches!(read_picked("bad.zip", b"not a zip"), Err(ImportError::BadArchive(_))));
    }

    #[test]
    fn zip_slip_names_keep_only_their_base_name() {
        let zip = zip_of(&[("../../../Windows/evil.DTm", &map_bytes("e")), ("..\\..\\up.dtm", &map_bytes("u"))]);
        let maps = read_picked("slip.zip", &zip).unwrap();
        let names: Vec<&str> = maps.iter().map(|m| m.file_name.as_str()).collect();
        assert_eq!(names, ["evil.DTm", "up.DTm"]);
    }

    #[test]
    fn a_name_with_no_base_name_is_no_map() {
        assert_eq!(map_file_name(".DTm"), None);
        assert_eq!(map_file_name("dir/"), None);
        assert_eq!(map_file_name("a/b.txt"), None);
        assert_eq!(map_file_name("a/B.dtm").as_deref(), Some("B.DTm"));
    }

    #[test]
    fn a_name_already_in_the_folder_is_a_conflict_ignoring_case() {
        let s = Scratch::new("conflicts");
        std::fs::write(s.0.join("Taken.DTm"), b"old").unwrap();
        let maps = read_picked("taken.dtm", &map_bytes("new")).unwrap();
        let fresh = read_picked("fresh.dtm", &map_bytes("new")).unwrap();
        assert_eq!(conflicts(&s.0, &maps), vec!["Taken.DTm".to_string()]);
        assert!(conflicts(&s.0, &fresh).is_empty());
    }

    #[test]
    fn writing_keeps_a_taken_name_unless_asked_to_replace() {
        let s = Scratch::new("write");
        std::fs::write(s.0.join("Taken.DTm"), b"old").unwrap();
        let zip = zip_of(&[("Taken.DTm", &map_bytes("new")), ("Fresh.DTm", &map_bytes("fresh"))]);
        let maps = read_picked("pack.zip", &zip).unwrap();

        let written = write_maps(&s.0, &maps, false).unwrap();
        assert_eq!(written, ["Fresh.DTm"]);
        assert_eq!(std::fs::read(s.0.join("Taken.DTm")).unwrap(), b"old");

        let written = write_maps(&s.0, &maps, true).unwrap();
        assert_eq!(written, ["Taken.DTm", "Fresh.DTm"]);
        assert_eq!(std::fs::read(s.0.join("Taken.DTm")).unwrap(), map_bytes("new"));
        let leftovers: Vec<_> = std::fs::read_dir(&s.0).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".tmp")).collect();
        assert!(leftovers.is_empty(), "no temporary file is left behind");
    }

    #[test]
    fn a_replaced_map_keeps_the_folder_spelling_of_its_name() {
        let s = Scratch::new("spelling");
        std::fs::write(s.0.join("Name.dtm"), b"old").unwrap();
        let maps = read_picked("Name.DTm", &map_bytes("new")).unwrap();
        assert_eq!(write_maps(&s.0, &maps, true).unwrap(), ["Name.dtm"]);
        let names: Vec<String> = existing_names(&s.0);
        assert_eq!(names, ["Name.dtm"], "one file, not two spellings of it");
    }

    /// The install's own maps read back through an archive: the zip path on real files
    /// (skipped without `RAZDOR_DT_DIR`; the bytes are read at runtime, never committed).
    #[test]
    fn an_install_map_survives_a_zip_round_trip() {
        let Some(dir) = std::env::var_os(super::super::install::ENV_VAR) else { return };
        let Ok(dt) = super::super::install::DtInstall::load(Path::new(&dir)) else { return };
        let Some(entry) = dt.maps.first() else { return };
        let bytes = std::fs::read(&entry.path).unwrap();
        let zip = zip_of(&[("pack/copy.DTm", &bytes)]);
        let maps = read_picked("pack.zip", &zip).unwrap();
        assert_eq!(maps[0].bytes, bytes);
        assert_eq!(maps[0].scenario.width(), entry.load().unwrap().width());
    }
}
