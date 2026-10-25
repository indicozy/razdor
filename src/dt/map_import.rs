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

/// The largest map: a picked `.DTm`, or one entry of an archive. A map is a few hundred KB.
pub const MAX_PICKED_MAP_BYTES: u64 = 16 << 20;
/// The largest picked archive.
pub const MAX_PICKED_ARCHIVE_BYTES: u64 = 64 << 20;
/// The decompressed bytes of the maps an archive may give in all (a zip bomb is refused).
const MAX_ARCHIVE_BYTES: u64 = 64 << 20;
/// The most maps one archive may give.
pub const MAX_MAPS: usize = 256;
/// An archive with more entries than this is refused (its list alone would be large).
const MAX_ENTRIES: usize = 4096;

/// The size limit of a picked file: an archive's, or a map's.
pub fn picked_limit(name: &str) -> u64 {
    if extension_is(name, "zip") {
        MAX_PICKED_ARCHIVE_BYTES
    } else {
        MAX_PICKED_MAP_BYTES
    }
}

/// Reads the file at `path` if it is at most `limit` bytes; `Ok(None)` when it is bigger (its
/// size, or what it holds as it is read: the size the system reports is not trusted alone).
pub fn read_limited(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map(|m| m.len()).unwrap_or(0) > limit {
        return Ok(None);
    }
    let mut data = Vec::new();
    file.take(limit + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
    Ok((data.len() as u64 <= limit).then_some(data))
}

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
    /// A picked file bigger than its limit (its file name).
    TooBig(String),
}

/// Windows' reserved device names: no file may take one, with or without an extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// The base name of a map inside `name` (a file name, or an archive entry's path, whose folders
/// are dropped): `Name.DTm`, the extension in the game's own spelling. `None` unless the base
/// name ends in `.dtm` (any case) and is a plain, safe file name: one path component, no drive
/// or device spelling (`C:`, `CON`), no characters Windows refuses, no control characters, and
/// no trailing dot or space.
pub fn map_file_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default().trim_start();
    if base.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')) || base.ends_with(['.', ' ']) {
        return None;
    }
    let (stem, ext) = base.rsplit_once('.')?;
    if !ext.eq_ignore_ascii_case(MAP_EXTENSION) {
        return None;
    }
    let stem = stem.trim();
    let device = stem.split('.').next().unwrap_or_default().to_ascii_uppercase();
    if stem.is_empty() || stem.starts_with('.') || RESERVED.contains(&device.as_str()) {
        return None;
    }
    let file_name = format!("{stem}.{MAP_EXTENSION}");
    let mut parts = Path::new(&file_name).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(_)), None) => Some(file_name),
        _ => None,
    }
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
    read_archive_within(bytes, MAX_ARCHIVE_BYTES, MAX_MAPS)
}

/// [`read_archive`] with its budget: the decompressed bytes of the `.DTm` entries read may total
/// `budget` at most, and the archive gives at most `max_maps` maps. Only the entries named
/// `.DTm` are decompressed, each through a `take()` so that a size the entry lies about cannot
/// get past the budget.
fn read_archive_within(bytes: &[u8], budget: u64, max_maps: usize) -> Result<Vec<Map>, ImportError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| ImportError::BadArchive(e.to_string()))?;
    if zip.len() > MAX_ENTRIES {
        return Err(ImportError::BadArchive(format!("{} files", zip.len())));
    }
    let mut maps: Vec<Map> = Vec::new();
    let mut total: u64 = 0;
    for index in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(index) else { continue };
        if entry.is_dir() {
            continue;
        }
        let Some(file_name) = entry.name().ok().and_then(|n| map_file_name(&n)) else { continue };
        // One byte more than what is left, so an entry that goes over the budget shows it.
        let left = budget.saturating_sub(total).min(MAX_PICKED_MAP_BYTES);
        let mut data = Vec::new();
        if entry.by_ref().take(left + 1).read_to_end(&mut data).is_err() {
            continue;
        }
        total += data.len() as u64;
        if total > budget {
            return Err(ImportError::BadArchive(format!("the maps inside are more than {} MiB", budget >> 20)));
        }
        if data.len() as u64 > MAX_PICKED_MAP_BYTES {
            continue;
        }
        let Ok(scenario) = Scenario::from_file_bytes(&data) else { continue };
        if maps.iter().any(|m| m.file_name.eq_ignore_ascii_case(&file_name)) {
            continue;
        }
        if maps.len() >= max_maps {
            return Err(ImportError::BadArchive(format!("more than {max_maps} maps")));
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
        // Only a file directly in the maps folder: a name that would go elsewhere is refused.
        if target.parent() != Some(folder) {
            return Err(format!("{}: not a file of the maps folder", map.file_name));
        }
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

    #[test]
    fn names_that_could_leave_the_folder_or_break_windows_are_refused_or_reduced() {
        // Refused: a drive, a colon, a device name, a trailing dot or space, a control character.
        for name in ["C:evil.DTm", "x:y.DTm", "CON.DTm", "nul.DTm", "com1.x.DTm", "x.DTm.", "x.DTm ", "bad\u{1}.DTm", "a<b.DTm", ".DTm", "dir/"] {
            assert_eq!(map_file_name(name), None, "{name:?} must be refused");
        }
        // Reduced to their base name, which is a plain name: no folder survives.
        assert_eq!(map_file_name("..\\x.DTm").as_deref(), Some("x.DTm"));
        assert_eq!(map_file_name("a/../../x.DTm").as_deref(), Some("x.DTm"));
        assert_eq!(map_file_name("C:\\dir\\ok.dtm").as_deref(), Some("ok.DTm"));
        // Normal names still come through.
        assert_eq!(map_file_name("Name With Spaces.dtm").as_deref(), Some("Name With Spaces.DTm"));
        assert_eq!(map_file_name("Console.DTm").as_deref(), Some("Console.DTm"), "a name that only starts like a device is fine");
    }

    #[test]
    fn an_archive_whose_maps_total_over_the_budget_is_refused() {
        let zip = zip_of(&[("a.DTm", &map_bytes("a")), ("b.DTm", &map_bytes("b"))]);
        assert!(matches!(read_archive_within(&zip, 10, MAX_MAPS), Err(ImportError::BadArchive(_))));
        assert_eq!(read_archive_within(&zip, 1 << 20, MAX_MAPS).unwrap().len(), 2);
    }

    #[test]
    fn an_archive_with_more_maps_than_the_cap_is_refused() {
        let zip = zip_of(&[("a.DTm", &map_bytes("a")), ("b.DTm", &map_bytes("b")), ("c.DTm", &map_bytes("c"))]);
        assert!(matches!(read_archive_within(&zip, 1 << 20, 2), Err(ImportError::BadArchive(_))));
        assert_eq!(read_archive_within(&zip, 1 << 20, 3).unwrap().len(), 3);
    }

    #[test]
    fn a_picked_file_over_its_limit_is_not_read() {
        let s = Scratch::new("limit");
        let path = s.0.join("picked.DTm");
        std::fs::write(&path, [7u8; 10]).unwrap();
        assert_eq!(read_limited(&path, 5).unwrap(), None);
        assert_eq!(read_limited(&path, 10).unwrap().map(|b| b.len()), Some(10));
        assert_eq!(picked_limit("pack.zip"), MAX_PICKED_ARCHIVE_BYTES);
        assert_eq!(picked_limit("map.DTm"), MAX_PICKED_MAP_BYTES);
    }

    #[test]
    fn a_map_is_written_only_directly_in_the_folder() {
        let s = Scratch::new("outside");
        let folder = s.0.join("Maps_Rus");
        let outside = Map { file_name: "../escaped.DTm".into(), bytes: map_bytes("x"), scenario: Scenario::default() };
        assert!(write_maps(&folder, &[outside], true).is_err());
        assert!(!s.0.join("escaped.DTm").exists());
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
