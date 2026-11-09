//! Moving saves in and out of the app's folder (Razdor's own, a player's request): the saves
//! folder goes out as one zip, saves come back in. On Android the folder is `Android/data/...`,
//! which the Files app cannot open, so the player exports and imports through the system's picker
//! (`ui::transfer`); the files themselves are written here.
//!
//! What a saves folder holds, and all an archive may hold: the json files at its root that the game
//! writes ([`ROOT_JSON`]), and under an install's folder (`<install>/manual`, `auto`, `maps`, see
//! `rules::save`) the save files (`.rzsave`) and the pins (`pinned.txt`, in `manual` and `auto`).
//! The folders may also sit at the root when no install is played. Anything else, a name that climbs
//! out of the folder (`..`, a leading `/`, a drive letter) or an archive over [`LIMIT`] is refused
//! before a byte is written.
//!
//! An import never overwrites: a save with the same bytes as one already there is skipped, one that
//! differs gets the next number (`name (2).rzsave`); a root json file that is there is kept (the
//! player's settings), and pins are merged into the pinned list of their folder. A failure midway
//! undoes what the import wrote.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};

use crate::i18n::tr;
use crate::rules::save::{self, AUTO_DIR, MANUAL_DIR, MAPS_DIR, PINS_FILE};

/// The most an import takes: the archive's files together, and a single save.
pub const LIMIT: u64 = 512 * 1024 * 1024;

/// The json files at a saves folder's root, as the game writes them: `settings.json`
/// (`ui::language`), `audio.json` (`ui::audio`), `marks.json` (`rules::marks`).
pub const ROOT_JSON: [&str; 3] = ["settings.json", "audio.json", "marks.json"];

/// The folders that hold saves under an install's folder (or at the root without an install).
const SAVE_FOLDERS: [&str; 3] = [MANUAL_DIR, AUTO_DIR, MAPS_DIR];

/// The kinds of file a saves folder holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A root json file (see [`ROOT_JSON`]).
    Json,
    /// A save (`.rzsave`) in one of [`SAVE_FOLDERS`].
    Save,
    /// The pins of a `manual` or `auto` folder.
    Pins,
}

/// Whether `parts` (the path of a file under a saves folder) is a file the folder holds, and which.
pub fn classify(parts: &[&str]) -> Option<Kind> {
    match parts {
        [name] if ROOT_JSON.contains(name) => Some(Kind::Json),
        [folder, name] => in_folder(folder, name),
        [install, folder, name] if save::is_install_folder(install) => in_folder(folder, name),
        _ => None,
    }
}

/// Whether `parts` (the path of a folder under a saves folder) is one the folder holds.
fn classify_dir(parts: &[&str]) -> bool {
    match parts {
        [folder] => SAVE_FOLDERS.contains(folder) || save::is_install_folder(folder),
        [install, folder] => save::is_install_folder(install) && SAVE_FOLDERS.contains(folder),
        _ => false,
    }
}

fn in_folder(folder: &str, name: &str) -> Option<Kind> {
    if !SAVE_FOLDERS.contains(&folder) {
        None
    } else if name.to_ascii_lowercase().ends_with(".rzsave") {
        Some(Kind::Save)
    } else if name == PINS_FILE && (folder == MANUAL_DIR || folder == AUTO_DIR) {
        Some(Kind::Pins)
    } else {
        None
    }
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

/// A failure's detail goes to the log; the player gets the fixed text (`text`, translated).
fn failed(text: &str, detail: impl std::fmt::Display) -> String {
    crate::diag!("transfer: {text}: {detail}");
    text.to_string()
}

/// The save files under `dir` that a saves folder holds, as (name in the archive, path), with the
/// folders' names joined by `/`, sorted. A missing folder has none.
fn collect(dir: &Path, parts: &mut Vec<String>, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut list: Vec<fs::DirEntry> = entries.filter_map(Result::ok).collect();
    list.sort_by_key(|e| e.file_name());
    for entry in list {
        let Ok(kind) = entry.file_type() else { continue };
        parts.push(entry.file_name().to_string_lossy().into_owned());
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        if kind.is_dir() {
            if classify_dir(&refs) {
                collect(&entry.path(), parts, out);
            }
        } else if kind.is_file() && classify(&refs).is_some() {
            out.push((parts.join("/"), entry.path()));
        }
        parts.pop();
    }
}

/// Zips the files of the saves folder `root` that it holds into the file `dest`, folders kept.
/// Returns how many files went in; with none, no file is written.
pub fn export_saves(root: &Path, dest: &Path) -> Result<usize, String> {
    let mut files = Vec::new();
    collect(root, &mut Vec::new(), &mut files);
    if files.is_empty() {
        return Ok(0);
    }
    let file = File::create(dest).map_err(|e| failed(tr("the file cannot be written"), e))?;
    let mut zip = zip::ZipWriter::new(BufWriter::new(file));
    for (name, path) in &files {
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(name, options).map_err(|e| failed(tr("the file cannot be written"), e))?;
        let mut src = File::open(path).map_err(|e| failed(tr("the save cannot be read"), e))?;
        io::copy(&mut src, &mut zip).map_err(|e| failed(tr("the save cannot be read"), e))?;
    }
    zip.finish().map_err(|e| failed(tr("the file cannot be written"), e))?;
    Ok(files.len())
}

/// What an import did: the saves written, the saves already there (the same bytes), and whether a
/// root json file that differs was kept as it was (the player's own settings).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub saves: usize,
    pub already: usize,
    pub settings_kept: bool,
}

/// What an import wrote, so a failed one can take it back.
#[derive(Default)]
struct Undo {
    /// Files created, in order.
    files: Vec<PathBuf>,
    /// Folders created, parents first.
    dirs: Vec<PathBuf>,
    /// Pinned lists changed: the file and its bytes before (`None`: it did not exist).
    pins: Vec<(PathBuf, Option<Vec<u8>>)>,
}

impl Undo {
    fn rollback(self) {
        for (path, before) in self.pins.into_iter().rev() {
            let _ = match before {
                Some(bytes) => fs::write(&path, bytes),
                None => fs::remove_file(&path),
            };
        }
        for file in self.files.iter().rev() {
            let _ = fs::remove_file(file);
        }
        for dir in self.dirs.iter().rev() {
            let _ = fs::remove_dir(dir);
        }
    }
}

/// Makes `dir` and the folders above it that are missing, recording them for the undo.
fn make_dirs(dir: &Path, undo: &mut Undo) -> Result<(), String> {
    let mut missing = Vec::new();
    let mut p = dir;
    while !p.exists() {
        missing.push(p.to_path_buf());
        match p.parent() {
            Some(parent) => p = parent,
            None => break,
        }
    }
    let made = fs::create_dir_all(dir);
    undo.dirs.extend(missing.into_iter().rev());
    made.map_err(|e| failed(tr("the file cannot be written"), e))
}

/// The outcome of [`put_exact`].
enum Put {
    /// Written.
    New,
    /// A file of that name with the same bytes is there.
    Same,
    /// A file of that name with other bytes is there (kept).
    Other,
}

/// Writes `data` to `path` when no file is there; otherwise reports whether the one there has the
/// same bytes. Created as it is written, so nothing is overwritten.
fn put_exact(path: &Path, data: &[u8], undo: &mut Undo) -> Result<Put, String> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut out) => {
            undo.files.push(path.to_path_buf());
            out.write_all(data).map_err(|e| failed(tr("the file cannot be written"), e))?;
            Ok(Put::New)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let existing = fs::read(path).map_err(|e| failed(tr("the file cannot be read"), e))?;
            Ok(if existing == data { Put::Same } else { Put::Other })
        }
        Err(e) => Err(failed(tr("the file cannot be written"), e)),
    }
}

/// The name of the `n`th copy of a file called `name`: `name` itself for the first (`n` 1), else
/// `stem (n).ext`. The import and the move (`migration`) name a clashing save this way.
pub fn numbered_name(name: &str, n: u32) -> String {
    if n == 1 {
        return name.to_string();
    }
    let path = Path::new(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    format!("{stem} ({n}){ext}")
}

/// Places a save in `dir` under `name`, or the first free numbered name (`stem (2).ext`, ...) whose
/// file differs. Returns how it was placed and the name it has there.
fn place_save(dir: &Path, name: &str, data: &[u8], undo: &mut Undo) -> Result<(Put, String), String> {
    let mut n = 1u32;
    loop {
        let file = numbered_name(name, n);
        match put_exact(&dir.join(&file), data, undo)? {
            Put::Other => n += 1,
            placed => return Ok((placed, file)),
        }
    }
}

/// Adds `names` to the pinned list in `dir` (kept in the order it has, the new ones last).
fn merge_pins(dir: &Path, names: &[String], undo: &mut Undo) -> Result<(), String> {
    let path = dir.join(PINS_FILE);
    let before = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(failed(tr("the file cannot be read"), e)),
    };
    let mut lines: Vec<String> = before
        .as_deref()
        .map(|b| String::from_utf8_lossy(b).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();
    let mut changed = false;
    for name in names {
        if !lines.contains(name) {
            lines.push(name.clone());
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }
    undo.pins.push((path.clone(), before));
    let mut text = lines.join("\n");
    text.push('\n');
    fs::write(&path, text).map_err(|e| failed(tr("the file cannot be written"), e))
}

/// One file of an archive that an import takes: its index, its folder under the root, its name.
struct Entry {
    index: usize,
    dir: Vec<String>,
    name: String,
    kind: Kind,
    size: u64,
}

/// Imports the saves zip `archive` into the saves folder `root`. Every entry is checked first (its
/// name, whether the folder holds it, the size) and nothing is written when one is refused. Returns
/// what came in; a failure midway takes back what was written.
pub fn import_archive(archive: &Path, root: &Path) -> Result<Imported, String> {
    import_archive_within(archive, root, LIMIT)
}

fn import_archive_within(archive: &Path, root: &Path, limit: u64) -> Result<Imported, String> {
    let file = File::open(archive).map_err(|e| failed(tr("the archive cannot be read"), e))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| failed(tr("the archive cannot be read"), e))?;
    // Pass 1: the whole archive is looked at before anything is written.
    let mut plan: Vec<Entry> = Vec::new();
    let mut total = 0u64;
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|e| failed(tr("the archive cannot be read"), e))?;
        let raw = entry.name().map_err(|e| failed(tr("the archive cannot be read"), e))?.to_string();
        let parts = relative_parts(&raw)?;
        if entry.is_dir() {
            if !classify_dir(&parts) {
                return Err(tr("The archive has a file that a saves folder does not hold.").to_string());
            }
            continue;
        }
        let Some(kind) = classify(&parts) else {
            return Err(tr("The archive has a file that a saves folder does not hold.").to_string());
        };
        total += entry.size();
        if total > limit {
            return Err(tr("The file is bigger than the limit of 512 MB.").to_string());
        }
        let (name, dir) = parts.split_last().expect("a file name has a part");
        plan.push(Entry {
            index,
            dir: dir.iter().map(|p| p.to_string()).collect(),
            name: name.to_string(),
            kind,
            size: entry.size(),
        });
    }
    if plan.is_empty() {
        return Err(tr("The file is not a Razdor save or saves archive.").to_string());
    }
    let mut undo = Undo::default();
    match apply_archive(&mut zip, &plan, root, &mut undo) {
        Ok(imported) => Ok(imported),
        Err(e) => {
            undo.rollback();
            Err(e)
        }
    }
}

/// Reads entry `e` of the archive, checked against its declared size.
fn read_entry<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, e: &Entry) -> Result<Vec<u8>, String> {
    let mut entry = zip.by_index(e.index).map_err(|err| failed(tr("the archive cannot be read"), err))?;
    let mut data = Vec::new();
    entry.by_ref().take(e.size + 1).read_to_end(&mut data).map_err(|err| failed(tr("the archive cannot be read"), err))?;
    if data.len() as u64 != e.size {
        return Err(tr("The archive cannot be read: its data does not match its size.").to_string());
    }
    Ok(data)
}

/// The writing of an archive's plan: the saves and the root json first, then the pins, which name
/// the saves by their final names.
fn apply_archive<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, plan: &[Entry], root: &Path, undo: &mut Undo) -> Result<Imported, String> {
    let mut imported = Imported::default();
    // (folder joined by `/`, name in the archive) -> the name it has in the folder now.
    let mut finals: HashMap<(String, String), String> = HashMap::new();
    for e in plan.iter().filter(|e| e.kind != Kind::Pins) {
        let data = read_entry(zip, e)?;
        match e.kind {
            Kind::Json => {
                make_dirs(root, undo)?;
                if let Put::Other = put_exact(&root.join(&e.name), &data, undo)? {
                    imported.settings_kept = true;
                }
            }
            _ => {
                let dir = dir_path(root, &e.dir);
                make_dirs(&dir, undo)?;
                let (placed, name) = place_save(&dir, &e.name, &data, undo)?;
                match placed {
                    Put::Same => imported.already += 1,
                    _ => imported.saves += 1,
                }
                finals.insert((e.dir.join("/"), e.name.clone()), name);
            }
        }
    }
    for e in plan.iter().filter(|e| e.kind == Kind::Pins) {
        let data = read_entry(zip, e)?;
        let text = String::from_utf8_lossy(&data);
        let names: Vec<String> = text
            .lines()
            .map(str::trim)
            .filter_map(|n| finals.get(&(e.dir.join("/"), n.to_string())).cloned())
            .collect();
        if !names.is_empty() {
            let dir = dir_path(root, &e.dir);
            make_dirs(&dir, undo)?;
            merge_pins(&dir, &names, undo)?;
        }
    }
    Ok(imported)
}

fn dir_path(root: &Path, parts: &[String]) -> PathBuf {
    parts.iter().fold(root.to_path_buf(), |p, part| p.join(part))
}

/// Copies the single save `src` (named `name` by the picker) into the folder `dir`, as
/// [`import_archive`] places a save: the same bytes already there count as there. Only a `.rzsave`
/// within [`LIMIT`].
pub fn import_save(src: &Path, name: &str, dir: &Path) -> Result<Imported, String> {
    import_save_within(src, name, dir, LIMIT)
}

fn import_save_within(src: &Path, name: &str, dir: &Path, limit: u64) -> Result<Imported, String> {
    // The picker's name may carry folder parts: only the last one is kept.
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    if !base.to_ascii_lowercase().ends_with(".rzsave") {
        return Err(tr("The file is not a Razdor save or saves archive.").to_string());
    }
    let size = fs::metadata(src).map_err(|e| failed(tr("the save cannot be read"), e))?.len();
    if size > limit {
        return Err(tr("The file is bigger than the limit of 512 MB.").to_string());
    }
    let data = fs::read(src).map_err(|e| failed(tr("the save cannot be read"), e))?;
    let mut undo = Undo::default();
    let placed = make_dirs(dir, &mut undo).and_then(|()| place_save(dir, base, &data, &mut undo));
    match placed {
        Ok((Put::Same, _)) => Ok(Imported { already: 1, ..Imported::default() }),
        Ok(_) => Ok(Imported { saves: 1, ..Imported::default() }),
        Err(e) => {
            undo.rollback();
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch folder for one test (removed by the test that makes it).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-transfer-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const INSTALL: &str = "Discord Times-1a2b3c4d";

    /// A saves folder like the game's: an install's saves, the settings file, the pins and a stray file.
    fn fake_saves(root: &Path) {
        let manual = root.join(INSTALL).join("manual");
        let auto = root.join(INSTALL).join("auto");
        fs::create_dir_all(&manual).unwrap();
        fs::create_dir_all(&auto).unwrap();
        fs::write(manual.join("Start.rzsave"), b"save one").unwrap();
        fs::write(manual.join(PINS_FILE), "Start.rzsave\n").unwrap();
        fs::write(auto.join("Battle - Foe.rzsave"), b"save two").unwrap();
        fs::write(root.join("settings.json"), b"{}").unwrap();
        fs::write(root.join("stray.log"), b"not a save").unwrap();
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
    fn export_then_import_round_trips_and_a_second_import_adds_nothing() {
        let dir = scratch("roundtrip");
        let (src, dst, zip_path) = (dir.join("src"), dir.join("dst"), dir.join("out.zip"));
        fake_saves(&src);
        assert_eq!(export_saves(&src, &zip_path).unwrap(), 4, "the saves, the pins and the settings; not the stray file");
        let first = import_archive(&zip_path, &dst).unwrap();
        assert_eq!(first, Imported { saves: 2, already: 0, settings_kept: false });
        let manual = dst.join(INSTALL).join("manual");
        assert_eq!(fs::read(manual.join("Start.rzsave")).unwrap(), b"save one");
        assert_eq!(fs::read(dst.join(INSTALL).join("auto").join("Battle - Foe.rzsave")).unwrap(), b"save two");
        assert_eq!(fs::read(dst.join("settings.json")).unwrap(), b"{}");
        assert_eq!(fs::read_to_string(manual.join(PINS_FILE)).unwrap(), "Start.rzsave\n", "the pin is merged in");
        assert!(!dst.join("stray.log").exists());
        // The same archive again: the same bytes are already there, nothing is added.
        assert_eq!(import_archive(&zip_path, &dst).unwrap(), Imported { saves: 0, already: 2, settings_kept: false });
        assert!(!manual.join("Start (2).rzsave").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_differing_save_gets_a_number_and_its_pin_follows_it() {
        let dir = scratch("differs");
        let (src, dst, zip_path) = (dir.join("src"), dir.join("dst"), dir.join("out.zip"));
        fake_saves(&src);
        export_saves(&src, &zip_path).unwrap();
        let manual = dst.join(INSTALL).join("manual");
        fs::create_dir_all(&manual).unwrap();
        fs::write(manual.join("Start.rzsave"), b"my own game").unwrap();
        fs::write(manual.join(PINS_FILE), "Mine.rzsave\n").unwrap();
        fs::write(dst.join("settings.json"), b"{\"mine\":1}").unwrap();
        let imported = import_archive(&zip_path, &dst).unwrap();
        assert_eq!(imported, Imported { saves: 2, already: 0, settings_kept: true });
        assert_eq!(fs::read(manual.join("Start.rzsave")).unwrap(), b"my own game", "the save there is not touched");
        assert_eq!(fs::read(manual.join("Start (2).rzsave")).unwrap(), b"save one");
        assert_eq!(fs::read_to_string(manual.join(PINS_FILE)).unwrap(), "Mine.rzsave\nStart (2).rzsave\n", "the pin follows the numbered save");
        assert_eq!(fs::read(dst.join("settings.json")).unwrap(), b"{\"mine\":1}", "the settings there are kept");
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
    fn a_single_save_lands_in_its_folder_and_a_clash_gets_the_next_number() {
        let dir = scratch("single");
        fs::create_dir_all(&dir).unwrap();
        let manual = dir.join("saves").join(INSTALL).join("manual");
        let (first, second) = (dir.join("picked-1"), dir.join("picked-2"));
        fs::write(&first, b"a save").unwrap();
        fs::write(&second, b"another save").unwrap();
        assert_eq!(import_save(&first, "Night raid.rzsave", &manual).unwrap(), Imported { saves: 1, ..Imported::default() });
        assert_eq!(fs::read(manual.join("Night raid.rzsave")).unwrap(), b"a save");
        assert_eq!(import_save(&second, "Night raid.rzsave", &manual).unwrap(), Imported { saves: 1, ..Imported::default() });
        assert_eq!(fs::read(manual.join("Night raid (2).rzsave")).unwrap(), b"another save");
        assert_eq!(import_save(&first, "Night raid.rzsave", &manual).unwrap(), Imported { already: 1, ..Imported::default() });
        assert!(!manual.join("Night raid (3).rzsave").exists(), "the same bytes as the second one are not written again");
        assert_eq!(import_save(&second, "Night raid.rzsave", &manual).unwrap(), Imported { already: 1, ..Imported::default() });
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_single_file_that_is_not_a_save_is_refused() {
        let dir = scratch("notsave");
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("picked-3");
        fs::write(&src, b"text").unwrap();
        assert!(import_save(&src, "notes.txt", &dir.join("manual")).is_err());
        assert!(!dir.join("manual").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_that_climbs_out_of_the_folder_is_refused_and_nothing_is_written() {
        let dir = scratch("slip");
        fs::create_dir_all(&dir).unwrap();
        let root = dir.join("saves");
        for evil in ["../escape.rzsave", "Discord Times-1a2b3c4d/manual/../../escape.rzsave", "/abs.rzsave", "C:/abs.rzsave", "Discord Times-1a2b3c4d\\manual\\b.rzsave"] {
            let archive = dir.join("evil.zip");
            zip_with(&archive, &[("Discord Times-1a2b3c4d/manual/ok.rzsave", b"fine"), (evil, b"bad")]);
            assert!(import_archive(&archive, &root).is_err(), "{evil} must be refused");
            assert!(!root.exists(), "{evil}: nothing is written before the check");
        }
        assert!(!dir.join("escape.rzsave").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_files_a_saves_folder_holds_are_accepted() {
        let refused: [&str; 9] = [
            "other.json",
            "readme.txt",
            "a.rzsave",
            "x-1a2b3c4G/manual/a.rzsave",
            "Discord Times-1a2b3c4d/other/a.rzsave",
            "Discord Times-1a2b3c4d/manual/a.txt",
            "Discord Times-1a2b3c4d/maps/pinned.txt",
            "Discord Times-1a2b3c4d/manual/sub/a.rzsave",
            "Discord Times-1a2b3c4d/manual/settings.json",
        ];
        for name in refused {
            assert_eq!(classify(&name.split('/').collect::<Vec<_>>()), None, "{name} is not a saves file");
            let dir = scratch("known");
            fs::create_dir_all(&dir).unwrap();
            let archive = dir.join("a.zip");
            zip_with(&archive, &[("Discord Times-1a2b3c4d/manual/ok.rzsave", b"fine"), (name, b"x")]);
            assert!(import_archive(&archive, &dir.join("saves")).is_err(), "{name} must be refused");
            assert!(!dir.join("saves").exists(), "{name}: nothing is written");
            let _ = fs::remove_dir_all(&dir);
        }
        let accepted = [
            "settings.json",
            "audio.json",
            "marks.json",
            "Discord Times-1a2b3c4d/manual/a.rzsave",
            "Discord Times-1a2b3c4d/auto/b.rzsave",
            "Discord Times-1a2b3c4d/maps/c.rzsave",
            "Discord Times-1a2b3c4d/manual/pinned.txt",
            "Discord Times-1a2b3c4d/auto/pinned.txt",
            "manual/x.rzsave",
        ];
        for name in accepted {
            assert!(classify(&name.split('/').collect::<Vec<_>>()).is_some(), "{name} is a saves file");
        }
    }

    #[test]
    fn the_root_json_names_are_the_ones_the_game_writes() {
        assert!(include_str!("rules/marks.rs").contains("\"marks.json\""));
        assert!(include_str!("ui/audio.rs").contains("\"audio.json\""));
        assert!(include_str!("ui/language.rs").contains("\"settings.json\""));
    }

    #[test]
    fn an_install_folder_is_the_name_the_saves_folder_gives_it() {
        let folder = save::saves_dir_of(Path::new("/root"), Path::new("/games/Discord Times Community Update"));
        assert!(save::is_install_folder(folder.file_name().unwrap().to_str().unwrap()));
        assert!(!save::is_install_folder("Discord Times"));
        assert!(!save::is_install_folder("Discord Times-1A2B3C4D"));
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
    fn a_failure_midway_takes_back_what_was_written() {
        let dir = scratch("rollback");
        let root = dir.join("saves");
        fs::create_dir_all(root.join(INSTALL)).unwrap();
        // `auto` is a file, so the second save cannot be written: the first one must be taken back.
        fs::write(root.join(INSTALL).join("auto"), b"in the way").unwrap();
        let archive = dir.join("a.zip");
        zip_with(&archive, &[("Discord Times-1a2b3c4d/manual/a.rzsave", b"first"), ("Discord Times-1a2b3c4d/auto/b.rzsave", b"second")]);
        assert!(import_archive(&archive, &root).is_err());
        assert!(!root.join(INSTALL).join("manual").exists(), "the folder made for the first save is removed");
        assert_eq!(fs::read(root.join(INSTALL).join("auto")).unwrap(), b"in the way");
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
        assert_eq!(import_archive_within(&archive, &dir.join("saves"), 64).unwrap().saves, 1);
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
