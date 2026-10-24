//! Unpacks the game from an archive the player downloaded (7z or zip), for the phones: they have
//! no 7z tool, and the game comes as a 7z archive plus a Community Update patch as a zip.
//!
//! [`find_archives`] classifies the archives of some folders by their listings (nothing is
//! extracted): a game archive holds the install's files ([`install::listing_is_install`]), a
//! patch holds some of them ([`install::is_install_path`]). [`unpack`] writes the game archives,
//! then the patches over them, straight into the install folder. Nothing here needs macroquad.

use super::install::{is_install_path, listing_is_install};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// Bytes copied at a time: the progress is reported after each piece.
const CHUNK: usize = 256 * 1024;

/// What an archive holds: a whole game, or a patch that goes over the game's files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Game,
    Patch,
}

/// An archive found in a folder that holds the game or a patch for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    /// The file name, for the player.
    pub name: String,
    pub kind: Kind,
    /// The top folder all the archive's entries lie in, dropped when unpacking; empty when none.
    pub strip: String,
    /// The uncompressed size of its files: the progress's total.
    pub size: u64,
}

/// One entry of an archive's listing: its path as the archive writes it, whether it is a folder,
/// and its uncompressed size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

/// What a listing says about its archive: its kind (None: nothing of the game in it), the top
/// folder to drop, and the uncompressed size of its files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub kind: Option<Kind>,
    pub strip: String,
    pub size: u64,
}

/// Why an unpacking stopped. A full disk or a folder that refuses writes is named, for the
/// player; anything else is text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnpackError {
    /// The disk is full or the folder refuses the writes: the folder written to.
    NoRoomOrAccess(PathBuf),
    /// Anything else, as text (with the archive's name where it is known).
    Other(String),
}

impl std::fmt::Display for UnpackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnpackError::NoRoomOrAccess(dir) => write!(f, "no room or no access to the folder {}", dir.display()),
            UnpackError::Other(text) => f.write_str(text),
        }
    }
}

/// A write failure under `dir`: [`UnpackError::NoRoomOrAccess`] for a full disk or a refused
/// folder, else its text.
fn write_error(e: &io::Error, dir: &Path) -> UnpackError {
    match e.kind() {
        io::ErrorKind::StorageFull | io::ErrorKind::PermissionDenied => UnpackError::NoRoomOrAccess(dir.to_path_buf()),
        _ => UnpackError::Other(e.to_string()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    SevenZ,
    Zip,
}

fn format_of(path: &Path) -> Option<Format> {
    let ext = path.extension()?.to_str()?;
    if ext.eq_ignore_ascii_case("7z") {
        Some(Format::SevenZ)
    } else if ext.eq_ignore_ascii_case("zip") {
        Some(Format::Zip)
    } else {
        None
    }
}

/// The archives of `dirs` (not their subfolders) that hold the game or a patch for it, sorted by
/// name within each folder. Each is classified by its listing. An archive that cannot be read is
/// logged and skipped, as is one that holds neither.
pub fn find_archives(dirs: &[PathBuf]) -> Vec<Found> {
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_file() && format_of(p).is_some()).collect();
        files.sort();
        for path in files {
            match list(&path) {
                Ok(entries) => {
                    let plan = plan(&entries);
                    if let Some(kind) = plan.kind {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        found.push(Found { path, name, kind, strip: plan.strip, size: plan.size });
                    }
                }
                Err(e) => crate::diag!("unpack: {}: {e}", path.display()),
            }
        }
    }
    found
}

/// Classifies a listing: the entries that are unsafe (see [`components`]) are left out, the top
/// folder is dropped when every entry lies in one (and it is not the install's own folder), and
/// the rest is a game when the install's check passes on it, a patch when any of it is one of
/// the install's files, else nothing.
pub fn plan(entries: &[Entry]) -> Plan {
    let parts: Vec<(Vec<&str>, &Entry)> = entries.iter().filter_map(|e| Some((components(&e.path)?, e))).filter(|(p, _)| !p.is_empty()).collect();
    let strip = top_folder(&parts).unwrap_or_default();
    let skip = usize::from(!strip.is_empty());
    let mut paths = Vec::new();
    let mut size = 0;
    for (p, e) in &parts {
        let rest = &p[skip..];
        if rest.is_empty() {
            continue;
        }
        if !e.is_dir {
            size += e.size;
        }
        paths.push(rest.join("/"));
    }
    let kind = if listing_is_install(&paths) {
        Some(Kind::Game)
    } else if paths.iter().any(|p| is_install_path(p)) {
        Some(Kind::Patch)
    } else {
        None
    };
    Plan { kind, strip, size }
}

/// The folder every entry lies in, when there is one: no file at the top, and the folder is not
/// one the install itself uses (`Maps_Rus`, `Rus_Units.ini`...), so a patch that holds only
/// `Maps_Rus/` keeps its layout.
fn top_folder(parts: &[(Vec<&str>, &Entry)]) -> Option<String> {
    let mut top: Option<&str> = None;
    for (p, e) in parts {
        if p.len() == 1 && !e.is_dir {
            return None;
        }
        match top {
            None => top = Some(p[0]),
            Some(t) if t == p[0] => {}
            Some(_) => return None,
        }
    }
    let t = top?;
    (!is_install_path(t)).then(|| t.to_string())
}

/// The parts of an archive's path, split at `/` or `\`, with empty and `.` parts dropped. None
/// for a path that must not be written: absolute, a drive prefix (`C:`), or climbing with `..`.
fn components(path: &str) -> Option<Vec<&str>> {
    if path.starts_with(['/', '\\']) {
        return None;
    }
    let mut out = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            p if p.contains(':') => return None,
            p => out.push(p),
        }
    }
    Some(out)
}

/// Where an entry goes under the install folder (a relative path), or None when it is not
/// written: an unsafe path, or the top folder itself.
fn target(name: &str, strip: &str) -> Option<PathBuf> {
    let parts = components(name)?;
    let rest = if strip.is_empty() {
        &parts[..]
    } else {
        match parts.split_first() {
            Some((first, rest)) if *first == strip => rest,
            _ => return None,
        }
    };
    if rest.is_empty() {
        return None;
    }
    Some(rest.iter().collect())
}

/// The entries of an archive, read from its listing (7z: its header; zip: its central directory).
fn list(path: &Path) -> Result<Vec<Entry>, String> {
    match format_of(path).ok_or("not a 7z or zip archive")? {
        Format::SevenZ => {
            let reader = sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty()).map_err(|e| e.to_string())?;
            Ok(reader.archive().files.iter().map(|f| Entry { path: f.name().to_string(), is_dir: f.is_directory(), size: f.size() }).collect())
        }
        Format::Zip => {
            let file = File::open(path).map_err(|e| e.to_string())?;
            let mut zip = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| e.to_string())?;
            let mut out = Vec::with_capacity(zip.len());
            for i in 0..zip.len() {
                let f = zip.by_index(i).map_err(|e| e.to_string())?;
                let Ok(name) = f.name().map(|n| n.into_owned()) else { continue };
                out.push(Entry { path: name, is_dir: f.is_dir(), size: f.size() });
            }
            Ok(out)
        }
    }
}

/// Unpacks the game: the game archives first, then the patches over them (their files overwrite
/// the game's). Each archive's top folder is dropped, so its files land in `dest`, which is
/// created when missing; files already there are overwritten, none is deleted. `progress` gets
/// the bytes written so far and the total of the archives' uncompressed sizes. Entries with an
/// unsafe path are skipped. Stops at the first error, which is returned as text.
pub fn unpack(archives: &[Found], dest: &Path, progress: &dyn Fn(u64, u64)) -> Result<(), UnpackError> {
    fs::create_dir_all(dest).map_err(|e| write_error(&e, dest))?;
    let total: u64 = archives.iter().map(|a| a.size).sum();
    let mut done = 0u64;
    for kind in [Kind::Game, Kind::Patch] {
        for archive in archives.iter().filter(|a| a.kind == kind) {
            unpack_one(archive, dest, &mut |n| {
                done += n;
                progress(done, total);
            })
            .map_err(|e| match e {
                UnpackError::Other(text) => UnpackError::Other(format!("{}: {text}", archive.name)),
                room => room,
            })?;
        }
    }
    Ok(())
}

/// Writes one archive's files under `dest`, calling `written` with each piece's size.
fn unpack_one(archive: &Found, dest: &Path, written: &mut dyn FnMut(u64)) -> Result<(), UnpackError> {
    let other = |e: &dyn std::fmt::Display| UnpackError::Other(e.to_string());
    match format_of(&archive.path).ok_or_else(|| UnpackError::Other("not a 7z or zip archive".into()))? {
        Format::SevenZ => {
            let mut reader = sevenz_rust2::ArchiveReader::open(&archive.path, sevenz_rust2::Password::empty()).map_err(|e| other(&e))?;
            reader
                .for_each_entries(|entry, input| {
                    let r = match (target(entry.name(), &archive.strip), entry.is_directory()) {
                        (Some(rel), true) => fs::create_dir_all(dest.join(rel)),
                        (Some(rel), false) => write_file(input, &dest.join(rel), written),
                        // Not written: the reader must still be drained, the next entry follows it.
                        (None, _) => io::copy(input, &mut io::sink()).map(drop),
                    };
                    // The io error goes back as it is, so its kind can be named below.
                    r.map_err(|e| sevenz_rust2::Error::Io(e, "unpacking".into()))?;
                    Ok(true)
                })
                .map_err(|e| match e {
                    sevenz_rust2::Error::Io(io, _) => write_error(&io, dest),
                    other_error => other(&other_error),
                })
        }
        Format::Zip => {
            let file = File::open(&archive.path).map_err(|e| other(&e))?;
            let mut zip = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| other(&e))?;
            for i in 0..zip.len() {
                let mut f = zip.by_index(i).map_err(|e| other(&e))?;
                let Ok(name) = f.name().map(|n| n.into_owned()) else { continue };
                let r = match (target(&name, &archive.strip), f.is_dir()) {
                    (Some(rel), true) => fs::create_dir_all(dest.join(rel)),
                    (Some(rel), false) => write_file(&mut f, &dest.join(rel), written),
                    (None, _) => Ok(()),
                };
                r.map_err(|e| match write_error(&e, dest) {
                    UnpackError::Other(text) => UnpackError::Other(format!("{name}: {text}")),
                    room => room,
                })?;
            }
            Ok(())
        }
    }
}

/// Streams one file to `path` (its folder made first), in pieces of [`CHUNK`] bytes.
fn write_file(input: &mut dyn Read, path: &Path, written: &mut dyn FnMut(u64)) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = BufWriter::new(File::create(path)?);
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        out.write_all(&buf[..n])?;
        written(n as u64);
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, size: u64) -> Entry {
        Entry { path: path.into(), is_dir: false, size }
    }

    fn dir(path: &str) -> Entry {
        Entry { path: path.into(), is_dir: true, size: 0 }
    }

    #[test]
    fn a_game_under_one_top_folder_is_classified_and_stripped() {
        let plan = plan(&[dir("DiscordTimes/"), dir("DiscordTimes/Maps_Rus"), file("DiscordTimes/Maps_Rus/a.DTm", 10), file("DiscordTimes/Rus_Units.ini", 5)]);
        assert_eq!(plan, Plan { kind: Some(Kind::Game), strip: "DiscordTimes".into(), size: 15 });
    }

    #[test]
    fn a_game_without_a_top_folder_is_not_stripped() {
        let plan = plan(&[dir("Maps_Rus"), file("Maps_Rus/a.DTm", 1), file("Rus_Units.ini", 2)]);
        assert_eq!(plan.kind, Some(Kind::Game));
        assert_eq!(plan.strip, "");
    }

    #[test]
    fn a_patch_holding_only_the_maps_folder_keeps_its_layout() {
        let plan = plan(&[dir("Maps_Rus"), file("Maps_Rus/b.DTm", 3)]);
        assert_eq!(plan, Plan { kind: Some(Kind::Patch), strip: String::new(), size: 3 });
    }

    #[test]
    fn a_patch_under_its_own_folder_is_stripped_and_is_a_patch() {
        let plan = plan(&[file("Community Update 1.2/Rus_Spells.ini", 4)]);
        assert_eq!(plan, Plan { kind: Some(Kind::Patch), strip: "Community Update 1.2".into(), size: 4 });
    }

    #[test]
    fn an_archive_with_neither_is_ignored() {
        assert_eq!(plan(&[file("readme.txt", 1), dir("docs"), file("docs/x.pdf", 9)]).kind, None);
        assert_eq!(plan(&[]).kind, None);
    }

    #[test]
    fn a_file_at_the_top_blocks_the_strip() {
        // Without the strip, `DiscordTimes/Rus_Units.ini` is not the install's path: nothing to unpack.
        let blocked = plan(&[file("notes.txt", 1), file("DiscordTimes/Rus_Units.ini", 1)]);
        assert_eq!(blocked.strip, "");
        assert_eq!(blocked.kind, None);
        // At the top, the unit list is one of the install's files: a patch.
        let top = plan(&[file("notes.txt", 1), file("Rus_Units.ini", 1)]);
        assert_eq!(top.kind, Some(Kind::Patch));
    }

    #[test]
    fn unsafe_entries_are_left_out_of_the_plan_and_the_targets() {
        let entries = [file("../evil.ini", 1), file("/abs/Rus_Units.ini", 1), file("C:/x/Rus_Units.ini", 1), file("a/../../b", 1)];
        assert_eq!(plan(&entries).kind, None);
        for bad in ["../evil.ini", "/etc/passwd", "\\\\server\\share", "C:\\x", "a/../../b", "DiscordTimes/../../x"] {
            assert_eq!(target(bad, "DiscordTimes"), None, "{bad}");
            assert_eq!(target(bad, ""), None, "{bad}");
        }
    }

    #[test]
    fn targets_drop_the_top_folder_and_the_folder_itself() {
        assert_eq!(target("DiscordTimes/Maps_Rus/a.DTm", "DiscordTimes"), Some(PathBuf::from("Maps_Rus/a.DTm")));
        assert_eq!(target("DiscordTimes/", "DiscordTimes"), None);
        assert_eq!(target("DiscordTimes", "DiscordTimes"), None);
        assert_eq!(target("Other/x.ini", "DiscordTimes"), None);
        assert_eq!(target("Rus_Units.ini", ""), Some(PathBuf::from("Rus_Units.ini")));
        assert_eq!(target("win\\style\\x.ini", ""), Some(PathBuf::from("win/style/x.ini")));
    }

    /// A fresh folder for a test, removed by the test at its end.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("razdor-unpack-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A zip of stored (uncompressed) entries: `(name, contents)`; a name ending in `/` is a folder.
    fn make_zip(path: &Path, entries: &[(&str, &str)]) {
        use std::io::Write as _;
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, contents) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, opts).unwrap();
            } else {
                zip.start_file(*name, opts).unwrap();
                zip.write_all(contents.as_bytes()).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn zip_game_and_patch_are_found_and_unpacked_in_order() {
        let s = Scratch::new("zip-order");
        let downloads = s.0.join("Download");
        fs::create_dir_all(&downloads).unwrap();
        make_zip(&downloads.join("game.zip"), &[
            ("DiscordTimes/", ""),
            ("DiscordTimes/Maps_Rus/", ""),
            ("DiscordTimes/Maps_Rus/a.DTm", "map"),
            ("DiscordTimes/Rus_Units.ini", "units"),
            ("DiscordTimes/Rus_Spells.ini", "game spells"),
            ("../escaped.txt", "must not appear"),
        ]);
        make_zip(&downloads.join("patch.zip"), &[("Update/Rus_Spells.ini", "patched spells")]);
        make_zip(&downloads.join("notes.zip"), &[("readme.txt", "hello")]);
        fs::write(downloads.join("broken.7z"), b"not an archive").unwrap();

        let found = find_archives(std::slice::from_ref(&downloads));
        let kinds: Vec<(&str, Kind)> = found.iter().map(|f| (f.name.as_str(), f.kind)).collect();
        assert_eq!(kinds, [("game.zip", Kind::Game), ("patch.zip", Kind::Patch)]);

        let dest = s.0.join("DiscordTimes");
        let total = std::cell::Cell::new(0u64);
        unpack(&[found[1].clone(), found[0].clone()], &dest, &|_, t| total.set(t)).unwrap();
        assert_eq!(read(&dest.join("Maps_Rus/a.DTm")), "map");
        assert_eq!(read(&dest.join("Rus_Units.ini")), "units");
        assert_eq!(read(&dest.join("Rus_Spells.ini")), "patched spells", "the patch goes over the game");
        assert!(!s.0.join("escaped.txt").exists() && !dest.join("escaped.txt").exists());
        assert!(!dest.join("DiscordTimes").exists(), "the top folder is dropped");
        // "map" + "units" + "game spells" in the game, "patched spells" in the patch; the escaped entry is not counted.
        assert_eq!(total.get(), 3 + 5 + 11 + 14);
    }

    #[test]
    fn progress_reaches_the_total() {
        let s = Scratch::new("progress");
        make_zip(&s.0.join("g.zip"), &[("Maps_Rus/a.DTm", "0123456789"), ("Rus_Units.ini", "units")]);
        let found = find_archives(std::slice::from_ref(&s.0));
        let last = std::cell::Cell::new((0u64, 0u64));
        unpack(&found, &s.0.join("out"), &|d, t| last.set((d, t))).unwrap();
        assert_eq!(last.get(), (15, 15));
        assert_eq!(read(&s.0.join("out/Maps_Rus/a.DTm")), "0123456789");
    }

    #[test]
    fn a_seven_zip_game_is_found_and_unpacked() {
        let s = Scratch::new("7z");
        let mut w = sevenz_rust2::ArchiveWriter::create(s.0.join("discord.7z")).unwrap();
        w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_directory("DiscordTimes"), None::<&[u8]>).unwrap();
        w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file("DiscordTimes/Maps_Rus/a.DTm"), Some(&b"map"[..])).unwrap();
        w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file("DiscordTimes/Rus_Units.ini"), Some(&b"units"[..])).unwrap();
        w.finish().unwrap();

        let found = find_archives(std::slice::from_ref(&s.0));
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].kind, found[0].strip.as_str()), (Kind::Game, "DiscordTimes"));
        let dest = s.0.join("install");
        unpack(&found, &dest, &|_, _| {}).unwrap();
        assert_eq!(read(&dest.join("Maps_Rus/a.DTm")), "map");
        assert_eq!(read(&dest.join("Rus_Units.ini")), "units");
    }

    #[test]
    fn a_full_disk_or_a_refused_folder_is_named_and_other_failures_are_text() {
        let dir = Path::new("/install");
        assert_eq!(write_error(&io::Error::from(io::ErrorKind::PermissionDenied), dir), UnpackError::NoRoomOrAccess(dir.into()));
        assert_eq!(write_error(&io::Error::from(io::ErrorKind::StorageFull), dir), UnpackError::NoRoomOrAccess(dir.into()));
        assert!(matches!(write_error(&io::Error::from(io::ErrorKind::InvalidData), dir), UnpackError::Other(_)));
    }

    #[test]
    fn folders_are_scanned_without_recursion() {
        let s = Scratch::new("flat");
        fs::create_dir_all(s.0.join("sub")).unwrap();
        make_zip(&s.0.join("sub/game.zip"), &[("Rus_Units.ini", "u")]);
        assert!(find_archives(std::slice::from_ref(&s.0)).is_empty());
        assert_eq!(find_archives(&[s.0.join("sub")]).len(), 1);
    }
}
