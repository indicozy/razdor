//! Moving saves in and out of the app's folder (Razdor's own, a player's request): the saves
//! folder goes out as one zip, a save or a saves zip comes back in. On Android the folder is
//! `Android/data/...`, which the Files app cannot open, so the player exports and imports through
//! the system's picker (`ui::transfer`); the files themselves are written here.
//!
//! What an archive may hold: the save files (`.rzsave`), the json files of the saves folder
//! (`settings.json`, `marks.json`, ...) and `pinned.txt`, in folders. Anything else, a name that
//! climbs out of the folder (`..`, a leading `/` or a drive letter) or an archive over [`LIMIT`]
//! is refused before a byte is written. A name already there is never overwritten: the new file
//! gets a number (`name (2).rzsave`).

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};

use crate::i18n::tr;
use crate::trf;

/// The most an import takes: the archive's files together, and a single save.
pub const LIMIT: u64 = 512 * 1024 * 1024;

/// The pins of a save folder (`rules::save`): the names of the pinned saves, one per line.
const PINS: &str = "pinned.txt";

/// Whether a file name is one the saves folder keeps: a save, a json file or the pins.
pub fn is_save_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    name == PINS || lower.ends_with(".rzsave") || lower.ends_with(".json")
}

/// The path parts of an archive entry name, or why it is not allowed: no empty part, no `.` or
/// `..`, no leading `/`, no backslash, no drive letter or NUL. A folder's trailing `/` is taken off.
fn relative_parts(name: &str) -> Result<Vec<&str>, String> {
    let bad = name.is_empty() || name.starts_with('/') || name.contains(['\\', ':', '\0']);
    let parts: Vec<&str> = name.trim_end_matches('/').split('/').collect();
    if bad || parts.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
        return Err(tr("The archive has a file name that is not allowed.").to_string());
    }
    Ok(parts)
}

/// `YYYY-MM-DD` (UTC) of `secs` seconds since 1970, for the export's file names.
pub fn date_stamp(secs: u64) -> String {
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm), as `ui::saves` reads it.
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02}")
}

/// The save files under `dir` as (name in the archive, path), folders' names joined with `/`,
/// sorted. A missing folder has none.
fn collect(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(dir) else { return Ok(()) };
    let mut list: Vec<fs::DirEntry> = entries.filter_map(Result::ok).collect();
    list.sort_by_key(|e| e.file_name());
    for entry in list {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else { continue };
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        if kind.is_dir() {
            collect(&entry.path(), &rel, out)?;
        } else if kind.is_file() && is_save_file(&name) {
            out.push((rel, entry.path()));
        }
    }
    Ok(())
}

/// Zips the save files of the saves folder `root` into the file `dest`, folders kept. Returns how
/// many files went in; with none, no file is written.
pub fn export_saves(root: &Path, dest: &Path) -> Result<usize, String> {
    let mut files = Vec::new();
    collect(root, "", &mut files)?;
    if files.is_empty() {
        return Ok(0);
    }
    let file = File::create(dest).map_err(|e| trf!("the file cannot be written: {why}", why = e.to_string()))?;
    let mut zip = zip::ZipWriter::new(BufWriter::new(file));
    for (name, path) in &files {
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        let mut src = File::open(path).map_err(|e| trf!("the save cannot be read: {why}", why = e.to_string()))?;
        io::copy(&mut src, &mut zip).map_err(|e| trf!("the file cannot be written: {why}", why = e.to_string()))?;
    }
    zip.finish().map_err(|e| trf!("the file cannot be written: {why}", why = e.to_string()))?;
    Ok(files.len())
}

/// The file `name` in `dir`, made new: the first name free of it, `name`, then `stem (2).ext`,
/// `stem (3).ext`, ... Created as it is taken, so no file is overwritten.
pub fn create_unique(dir: &Path, name: &str) -> io::Result<(PathBuf, File)> {
    let path = Path::new(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut n = 1u32;
    loop {
        let file_name = if n == 1 { name.to_string() } else { format!("{stem} ({n}){ext}") };
        let target = dir.join(&file_name);
        match fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(file) => return Ok((target, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Imports the saves zip `archive` into the saves folder `root`, folders kept. Every entry is
/// checked first (names, kinds, the size) and nothing is written when one is refused. Returns
/// how many `.rzsave` saves came in.
pub fn import_archive(archive: &Path, root: &Path) -> Result<usize, String> {
    import_archive_within(archive, root, LIMIT)
}

fn import_archive_within(archive: &Path, root: &Path, limit: u64) -> Result<usize, String> {
    let file = File::open(archive).map_err(|e| trf!("the archive cannot be read: {why}", why = e.to_string()))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| trf!("the archive cannot be read: {why}", why = e.to_string()))?;
    // Pass 1: the whole archive is looked at before anything is written.
    let mut plan: Vec<(usize, Vec<String>, u64)> = Vec::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| trf!("the archive cannot be read: {why}", why = e.to_string()))?;
        let name = entry.name().map_err(|e| trf!("the archive cannot be read: {why}", why = e.to_string()))?.to_string();
        let parts = relative_parts(&name)?;
        if entry.is_dir() {
            continue;
        }
        let last = parts.last().copied().unwrap_or_default();
        if !is_save_file(last) {
            return Err(tr("The file is not a Razdor save or saves archive.").to_string());
        }
        total += entry.size();
        if total > limit {
            return Err(tr("The file is bigger than the limit of 512 MB.").to_string());
        }
        plan.push((i, parts.iter().map(|p| p.to_string()).collect(), entry.size()));
    }
    if plan.is_empty() {
        return Err(tr("The file is not a Razdor save or saves archive.").to_string());
    }
    // Pass 2: the files are written, each under a name not taken yet.
    let mut saves = 0;
    for (i, parts, size) in plan {
        let mut entry = zip.by_index(i).map_err(|e| trf!("the archive cannot be read: {why}", why = e.to_string()))?;
        let mut dir = root.to_path_buf();
        for part in &parts[..parts.len() - 1] {
            dir.push(part);
        }
        let last = parts.last().map(String::as_str).unwrap_or_default();
        fs::create_dir_all(&dir).map_err(|e| trf!("the file cannot be written: {why}", why = e.to_string()))?;
        let (target, mut out) = create_unique(&dir, last).map_err(|e| trf!("the file cannot be written: {why}", why = e.to_string()))?;
        let copied = io::copy(&mut (&mut entry).take(size + 1), &mut out);
        if copied.ok() != Some(size) {
            drop(out);
            let _ = fs::remove_file(&target);
            return Err(tr("The archive cannot be read: its data does not match its size.").to_string());
        }
        if last.to_ascii_lowercase().ends_with(".rzsave") {
            saves += 1;
        }
    }
    Ok(saves)
}

/// Copies the single save `src` (named `name` by the picker) into the folder `dir`, under a name
/// not taken there. Only a `.rzsave` within [`LIMIT`]. Returns the file written.
pub fn import_save(src: &Path, name: &str, dir: &Path) -> Result<PathBuf, String> {
    import_save_within(src, name, dir, LIMIT)
}

fn import_save_within(src: &Path, name: &str, dir: &Path, limit: u64) -> Result<PathBuf, String> {
    // The picker's name may carry folder parts: only the last one is kept.
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    if !base.to_ascii_lowercase().ends_with(".rzsave") {
        return Err(tr("The file is not a Razdor save or saves archive.").to_string());
    }
    let size = fs::metadata(src).map_err(|e| trf!("the save cannot be read: {why}", why = e.to_string()))?.len();
    if size > limit {
        return Err(tr("The file is bigger than the limit of 512 MB.").to_string());
    }
    fs::create_dir_all(dir).map_err(|e| trf!("the file cannot be written: {why}", why = e.to_string()))?;
    let mut input = File::open(src).map_err(|e| trf!("the save cannot be read: {why}", why = e.to_string()))?;
    let (target, mut out) = create_unique(dir, base).map_err(|e| trf!("the file cannot be written: {why}", why = e.to_string()))?;
    if let Err(e) = io::copy(&mut input, &mut out) {
        drop(out);
        let _ = fs::remove_file(&target);
        return Err(trf!("the file cannot be written: {why}", why = e.to_string()));
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A scratch folder for one test (removed by the test that makes it).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-transfer-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A saves folder like the game's: two installs' saves, a settings file, the pins and a stray file.
    fn fake_saves(root: &Path) {
        let manual = root.join("Discord Times-1a2b3c4d").join("manual");
        let auto = root.join("Discord Times-1a2b3c4d").join("auto");
        fs::create_dir_all(&manual).unwrap();
        fs::create_dir_all(&auto).unwrap();
        fs::write(manual.join("Start.rzsave"), b"save one").unwrap();
        fs::write(manual.join(PINS), "Start.rzsave\n").unwrap();
        fs::write(auto.join("Battle - Foe.rzsave"), b"save two").unwrap();
        fs::write(root.join("settings.json"), b"{}").unwrap();
        fs::write(root.join("stray.log"), b"not a save").unwrap();
    }

    #[test]
    fn export_then_import_round_trips_the_saves_folders() {
        let dir = scratch("roundtrip");
        let (src, dst, zip_path) = (dir.join("src"), dir.join("dst"), dir.join("out.zip"));
        fake_saves(&src);
        assert_eq!(export_saves(&src, &zip_path).unwrap(), 4, "the saves, the pins and the settings; not the stray file");
        assert_eq!(import_archive(&zip_path, &dst).unwrap(), 2);
        let manual = dst.join("Discord Times-1a2b3c4d").join("manual");
        assert_eq!(fs::read(manual.join("Start.rzsave")).unwrap(), b"save one");
        assert_eq!(fs::read(dst.join("Discord Times-1a2b3c4d").join("auto").join("Battle - Foe.rzsave")).unwrap(), b"save two");
        assert_eq!(fs::read(dst.join("settings.json")).unwrap(), b"{}");
        assert!(!dst.join("stray.log").exists());
        // The same archive again: nothing is overwritten, the copies get numbers.
        assert_eq!(import_archive(&zip_path, &dst).unwrap(), 2);
        assert_eq!(fs::read(manual.join("Start (2).rzsave")).unwrap(), b"save one");
        assert_eq!(fs::read(manual.join("Start.rzsave")).unwrap(), b"save one");
        assert!(manual.join("pinned (2).txt").is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_saves_folder_exports_nothing() {
        let dir = scratch("empty");
        fs::create_dir_all(dir.join("saves")).unwrap();
        let zip_path = dir.join("out.zip");
        assert_eq!(export_saves(&dir.join("saves"), &zip_path).unwrap(), 0);
        assert!(!zip_path.exists());
        assert_eq!(export_saves(&dir.join("missing"), &zip_path).unwrap(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_clash_gets_the_next_number() {
        let dir = scratch("clash");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Save.rzsave"), b"old").unwrap();
        let (first, _) = create_unique(&dir, "Save.rzsave").unwrap();
        let (second, _) = create_unique(&dir, "Save.rzsave").unwrap();
        assert_eq!(first.file_name().unwrap(), "Save (2).rzsave");
        assert_eq!(second.file_name().unwrap(), "Save (3).rzsave");
        assert_eq!(fs::read(dir.join("Save.rzsave")).unwrap(), b"old", "the file there is not touched");
        let (bare, _) = create_unique(&dir, "pinned").unwrap();
        assert_eq!(bare.file_name().unwrap(), "pinned");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_single_save_lands_in_its_folder_under_a_free_name() {
        let dir = scratch("single");
        let src = dir.join("picked-1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&src, b"a save").unwrap();
        // The folder the Load window lists for an install: its saves folder's manual one.
        let manual = dir.join("saves").join("Discord Times-1a2b3c4d").join("manual");
        let first = import_save(&src, "Night raid.rzsave", &manual).unwrap();
        assert_eq!(first, manual.join("Night raid.rzsave"));
        let second = import_save(&src, "Night raid.rzsave", &manual).unwrap();
        assert_eq!(second, manual.join("Night raid (2).rzsave"));
        assert_eq!(fs::read(&second).unwrap(), b"a save");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_single_file_that_is_not_a_save_is_refused() {
        let dir = scratch("notsave");
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("picked-2");
        fs::write(&src, b"text").unwrap();
        assert!(import_save(&src, "notes.txt", &dir.join("manual")).is_err());
        assert!(!dir.join("manual").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A zip with the given entries (name, bytes), written raw so the names are as given.
    fn zip_with(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn a_name_that_climbs_out_of_the_folder_is_refused_and_nothing_is_written() {
        let dir = scratch("slip");
        fs::create_dir_all(&dir).unwrap();
        let root = dir.join("saves");
        for evil in ["../escape.rzsave", "a/../../escape.rzsave", "/abs.rzsave", "C:/abs.rzsave", "a\\..\\b.rzsave"] {
            let archive = dir.join("evil.zip");
            zip_with(&archive, &[("Install/manual/ok.rzsave", b"fine"), (evil, b"bad")]);
            assert!(import_archive(&archive, &root).is_err(), "{evil} must be refused");
            assert!(!root.join("Install").join("manual").join("ok.rzsave").exists(), "{evil}: nothing is written before the check");
        }
        assert!(!dir.join("escape.rzsave").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_of_other_files_is_refused() {
        let dir = scratch("other");
        fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("other.zip");
        zip_with(&archive, &[("readme.txt", b"hello")]);
        assert!(import_archive(&archive, &dir.join("saves")).is_err());
        assert!(!dir.join("saves").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_size_cap_refuses_a_big_archive_and_a_big_save() {
        let dir = scratch("cap");
        fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("big.zip");
        zip_with(&archive, &[("manual/a.rzsave", &[0u8; 64])]);
        assert!(import_archive_within(&archive, &dir.join("saves"), 32).is_err());
        assert!(!dir.join("saves").exists());
        assert_eq!(import_archive_within(&archive, &dir.join("saves"), 64).unwrap(), 1);
        let src = dir.join("picked");
        fs::write(&src, [0u8; 64]).unwrap();
        assert!(import_save_within(&src, "big.rzsave", &dir.join("manual"), 32).is_err());
        assert!(import_save_within(&src, "big.rzsave", &dir.join("manual"), 64).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_export_name_takes_the_utc_date() {
        assert_eq!(date_stamp(0), "1970-01-01");
        assert_eq!(date_stamp(1_700_000_000), "2023-11-14");
        assert_eq!(date_stamp(1_709_251_199), "2024-02-29");
    }
}
