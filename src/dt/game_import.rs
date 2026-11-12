//! Importing a game archive into the install («Обновить игру из архива…», `ui::game_import`): a
//! whole game replaces the install folder, a patch's files go over its files. Nothing here needs
//! macroquad.
//!
//! The archive is always unpacked first, into a folder beside the install (`<install>.new`), and
//! the install is touched only after that worked. A game then swaps folders (the install moves to
//! `<install>.old`, the new folder takes its name, the old one is deleted); a patch moves its files
//! over the install's. A failed import removes `<install>.new` and leaves the install as it was.
//! The saves are in `platform::app_dir`, never beside the install, so nothing here reads or
//! writes them. Only the phones' unpack folder is ever written (see [`target`]): a player's own
//! copy elsewhere is never replaced.

use super::install::is_install;
use super::unpack::{self, Found, Kind, UnpackError, MAX_UNPACKED_BYTES};
use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The largest archive file accepted, as it is on disk (a game is hundreds of MB).
pub const MAX_ARCHIVE_BYTES: u64 = 2 << 30;

/// The room kept free beyond an archive's files when it is unpacked (the staging copy is written
/// while the install is still there).
pub const SPACE_MARGIN: u64 = 64 << 20;

/// Why an import did not happen. The install is not changed by any of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The file holds neither the game nor a patch for it (its name).
    NotGame(String),
    /// The archive's listing cannot be read: the reason.
    Unreadable(String),
    /// The archive or what it unpacks to is over the budget (its name).
    TooBig(String),
    /// A patch, and there is no game in the folder to patch.
    NoInstall,
    /// The archive unpacked, but its folder is not a Discord Times install.
    NotInstall,
    /// The unpacking failed (its error, with the archive's name where known).
    Unpack(UnpackError),
    /// The file is neither a 7z nor a zip archive (by its name or its first bytes).
    NotArchive,
    /// The swap or the moving of the files failed: the reason.
    Io(String),
    /// The archive's files do not fit on the disk: the bytes needed (with the margin) and free.
    NoSpace { needed: u64, free: u64 },
    /// The game folder's download failed (`gdrive`): the reason.
    Download(String),
    /// The game folder's download was cancelled (its files kept for a retry).
    Cancelled,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotGame(name) => write!(f, "{name} holds neither the game nor a patch for it"),
            Error::Unreadable(why) => write!(f, "the archive cannot be read: {why}"),
            Error::TooBig(name) => write!(f, "{name} is too big"),
            Error::NoInstall => f.write_str("there is no game to patch"),
            Error::NotInstall => f.write_str("the archive does not hold a Discord Times install"),
            Error::Unpack(e) => write!(f, "{e}"),
            Error::NotArchive => f.write_str("not a 7z or zip archive"),
            Error::Io(why) => write!(f, "{why}"),
            Error::NoSpace { needed, free } => write!(f, "{needed} bytes are needed and {free} are free"),
            Error::Download(why) => write!(f, "the download failed: {why}"),
            Error::Cancelled => f.write_str("the download was cancelled"),
        }
    }
}

/// What an import did: the whole game was replaced, or this many files were written over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    Replaced,
    Patched(usize),
}

/// The budget's verdict on an archive of `archive` bytes unpacking to `unpacked` bytes.
pub fn over_budget(name: &str, archive: u64, unpacked: u64) -> Option<Error> {
    (archive > MAX_ARCHIVE_BYTES || unpacked > MAX_UNPACKED_BYTES).then(|| Error::TooBig(name.to_string()))
}

/// The archive at `path`, checked before anything is written: it must hold the game or a patch
/// for it, and be within the budget. Only its listing is read. `name` is the archive's name for
/// the player (a phone's picked file is a copy in the cache, with another name).
pub fn examine(path: &Path, name: &str) -> Result<Found, Error> {
    let archive = fs::metadata(path).map(|m| m.len()).map_err(|e| Error::Unreadable(e.to_string()))?;
    classify(unpack::inspect(path), archive, name)
}

/// [`examine`] of an archive that is open already (a phone's picked document, read in place):
/// its listing is read from the file's start.
pub fn examine_file(file: &mut File, name: &str) -> Result<Found, Error> {
    if !unpack::is_archive(file, name) {
        return Err(Error::NotArchive);
    }
    let archive = file.metadata().map(|m| m.len()).map_err(|e| Error::Unreadable(e.to_string()))?;
    classify(unpack::inspect_file(file, name), archive, name)
}

fn classify(listing: Result<Option<Found>, String>, archive: u64, name: &str) -> Result<Found, Error> {
    let mut found = match listing {
        Ok(Some(found)) => found,
        Ok(None) => return Err(Error::NotGame(name.to_string())),
        Err(e) => return Err(Error::Unreadable(e)),
    };
    found.name = name.to_string();
    match over_budget(name, archive, found.size) {
        Some(e) => Err(e),
        None => Ok(found),
    }
}

/// Whether an error of a write is the disk running out of room (`ENOSPC`, or the StorageFull kind).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn is_out_of_room(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::StorageFull || e.raw_os_error() == Some(28)
}

/// Refuses an archive of `archive` bytes (None: unknown, not refused) when its files with
/// [`SPACE_MARGIN`] do not fit in the free room of `dir` (`free`: [`free_space`], or a test's). A
/// room that cannot be read is not refused either.
pub fn check_room(archive: Option<u64>, dir: &Path, free: &dyn Fn(&Path) -> Option<u64>) -> Result<(), Error> {
    let Some(archive) = archive else { return Ok(()) };
    let Some(free) = free(dir) else { return Ok(()) };
    let needed = archive.saturating_add(SPACE_MARGIN);
    if needed > free {
        return Err(Error::NoSpace { needed, free });
    }
    Ok(())
}

/// Whether `file` can seek, so it can be read as an archive (a pipe or a stream cannot).
pub fn is_seekable(file: &mut File) -> bool {
    file.seek(SeekFrom::End(0)).is_ok() && file.seek(SeekFrom::Start(0)).is_ok()
}

/// The folder an import unpacks into first: `<install>.new`, beside the install (the same disk,
/// so the swap is a rename).
pub fn staging_dir(install: &Path) -> PathBuf {
    sibling(install, "new")
}

/// The folder the install moves to during a swap: `<install>.old`.
pub fn old_dir(install: &Path) -> PathBuf {
    sibling(install, "old")
}

fn sibling(install: &Path, ext: &str) -> PathBuf {
    let mut name = install.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(ext);
    install.with_file_name(name)
}

/// The install folder an import may write to: the phones' unpack folder (`unpack_dir`) when it
/// is the current install or there is no install yet. Any other install (a player's own copy,
/// a `RAZDOR_DT_DIR`, a Steam or GOG folder) is never written: None.
pub fn target(install: Option<&Path>, unpack_dir: Option<&Path>) -> Option<PathBuf> {
    let unpack_dir = unpack_dir?;
    match install {
        None => Some(unpack_dir.to_path_buf()),
        Some(dir) if same_folder(dir, unpack_dir) => Some(unpack_dir.to_path_buf()),
        Some(_) => None,
    }
}

fn same_folder(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Imports `found` into the install folder `target` (see [`target`]). `progress` gets the bytes
/// unpacked so far and the total. Every failure leaves `target` as it was.
pub fn import(found: &Found, target: &Path, progress: &dyn Fn(u64, u64)) -> Result<Done, Error> {
    let mut unpack = |staging: &Path, progress: &dyn Fn(u64, u64)| unpack::unpack(std::slice::from_ref(found), staging, progress);
    stage_and_apply(found, target, progress, &free_space, &mut unpack)
}

/// [`import`] of an archive that is open already (see [`examine_file`]): read from `file`.
pub fn import_file(file: &mut File, found: &Found, target: &Path, progress: &dyn Fn(u64, u64)) -> Result<Done, Error> {
    let mut unpack = |staging: &Path, progress: &dyn Fn(u64, u64)| unpack::unpack_file(file, found, staging, progress);
    stage_and_apply(found, target, progress, &free_space, &mut unpack)
}

/// Writes an archive into a staging folder, reporting its progress (see [`stage_and_apply`]).
type Unpacker<'a> = dyn FnMut(&Path, &dyn Fn(u64, u64)) -> Result<(), UnpackError> + 'a;

/// The import's work: a stale staging folder goes, the room is checked (`free` gives the free
/// bytes of a folder: [`free_space`], or a test's), `unpack` writes the archive into the staging
/// folder, then the staged files replace the install or are written over it.
fn stage_and_apply(
    found: &Found,
    target: &Path,
    progress: &dyn Fn(u64, u64),
    free: &dyn Fn(&Path) -> Option<u64>,
    unpack: &mut Unpacker<'_>,
) -> Result<Done, Error> {
    if over_budget(&found.name, 0, found.size).is_some() {
        return Err(Error::TooBig(found.name.clone()));
    }
    if found.kind == Kind::Patch && !target.is_dir() {
        return Err(Error::NoInstall);
    }
    // Refused before anything is deleted or written.
    check_space(found, target, free)?;
    let staging = staging_dir(target);
    // A folder left by an import that did not finish.
    let _ = fs::remove_dir_all(&staging);
    let result = unpack(&staging, progress).map_err(Error::Unpack).and_then(|()| apply(found, target, &staging));
    // After a swap the staging folder is the install (renamed away); after a patch it is empty;
    // after a failure it holds the unpacked files. Whatever is left goes.
    let _ = fs::remove_dir_all(&staging);
    result
}

/// The unpacked `staging` folder put in place: a whole game replaces `target`, a patch goes over it.
fn apply(found: &Found, target: &Path, staging: &Path) -> Result<Done, Error> {
    match found.kind {
        Kind::Game => import_folder(staging, target),
        Kind::Patch => move_over(staging, target).map(Done::Patched).map_err(|e| Error::Io(e.to_string())),
    }
}

/// [`check_room`] for an import into `target`: the room of the disk the staging folder goes to.
fn check_space(found: &Found, target: &Path, free: &dyn Fn(&Path) -> Option<u64>) -> Result<(), Error> {
    let dir = match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    check_room(Some(found.size), dir, free)
}

/// The bytes free to the player on the disk of `dir` (`statvfs`), or None when they cannot be read
/// (Windows: not checked here).
pub fn free_space(dir: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        // SAFETY: `path` is a NUL-terminated string that lives across the call, and `stat` is a
        // statvfs record that the call fills in.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
            return None;
        }
        Some(wide(stat.f_bavail).saturating_mul(wide(stat.f_frsize)))
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        None
    }
}

/// A field of the platform's statvfs record as a u64 (the field widths differ by platform).
#[cfg(unix)]
fn wide<T: Into<u64>>(v: T) -> u64 {
    v.into()
}

/// A whole game that is already unpacked in `staged` (an archive's unpacking, or the folder the
/// game download fills, `gdrive`) replaces the install folder `target`: the install check, then
/// the swap. Every failure leaves `target` as it was, and `staged` for the caller to keep or drop.
pub fn import_folder(staged: &Path, target: &Path) -> Result<Done, Error> {
    if !is_install(staged) {
        return Err(Error::NotInstall);
    }
    swap_in(target, staged).map_err(|e| Error::Io(e.to_string()))?;
    Ok(Done::Replaced)
}

/// Puts `staged` in place of `install`: the install moves to [`old_dir`], `staged` takes its
/// name, and the old folder is deleted. A folder left by an interrupted swap is removed first. If
/// the second rename fails, the old folder goes back, so the install is never left missing.
pub fn swap_in(install: &Path, staged: &Path) -> io::Result<()> {
    let old = old_dir(install);
    if old.exists() {
        fs::remove_dir_all(&old)?;
    }
    let had_install = install.exists();
    if had_install {
        fs::rename(install, &old)?;
    }
    if let Err(e) = fs::rename(staged, install) {
        if had_install {
            let _ = fs::rename(&old, install);
        }
        return Err(e);
    }
    if had_install {
        // The new install is in place; a copy that cannot be deleted now is removed by the next swap.
        if let Err(e) = fs::remove_dir_all(&old) {
            crate::diag!("game import: the old install {} is not removed: {e}", old.display());
        }
    }
    Ok(())
}

/// Puts back an install that an interrupted swap left moved to [`old_dir`]: when `install` is
/// missing and its old copy is there, the copy takes the name again. Called at the start, before
/// the install is looked for.
pub fn recover(install: &Path) {
    let old = old_dir(install);
    if !install.exists() && old.is_dir() {
        if let Err(e) = fs::rename(&old, install) {
            crate::diag!("game import: {} is not put back: {e}", old.display());
        }
    }
}

/// Moves every file under `from` to the same place under `to`, replacing the files there, and
/// returns how many. A file that cannot be renamed (another disk) is copied and the source
/// removed.
fn move_over(from: &Path, to: &Path) -> io::Result<usize> {
    let mut files = Vec::new();
    collect_files(from, from, &mut files)?;
    for rel in &files {
        let src = from.join(rel);
        let dst = to.join(rel);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::rename(&src, &dst).is_err() {
            fs::copy(&src, &dst)?;
            fs::remove_file(&src)?;
        }
    }
    Ok(files.len())
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_files(root, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_path_buf());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::{Cursor, Write as _};
    use std::time::{Duration, SystemTime};

    /// A fresh folder for a test, removed by the test at its end.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("razdor-game-import-{}-{name}", std::process::id()));
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

    /// Every file and folder under `dir` with its bytes (folders as empty entries with a `/`).
    fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            let Ok(entries) = fs::read_dir(dir) else { return };
            for e in entries.flatten() {
                let path = e.path();
                let rel = path.strip_prefix(root).unwrap().display().to_string();
                if path.is_dir() {
                    out.insert(format!("{rel}/"), Vec::new());
                    walk(root, &path, out);
                } else {
                    out.insert(rel, fs::read(&path).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(dir, dir, &mut out);
        out
    }

    /// A zip of stored entries: `(name, contents)`; a name ending in `/` is a folder.
    fn zip_bytes(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, contents) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, opts).unwrap();
            } else {
                zip.start_file(*name, opts).unwrap();
                zip.write_all(contents.as_bytes()).unwrap();
            }
        }
        zip.finish().unwrap().into_inner()
    }

    fn make_zip(path: &Path, entries: &[(&str, &str)]) {
        fs::write(path, zip_bytes(entries)).unwrap();
    }

    /// An install's files: a maps folder, the unit list, a spell list and a picture.
    fn fake_install(dir: &Path, tag: &str) {
        fs::create_dir_all(dir.join("Maps_Rus")).unwrap();
        fs::write(dir.join("Maps_Rus/a.DTm"), format!("map a {tag}")).unwrap();
        fs::write(dir.join("Rus_Units.ini"), format!("units {tag}")).unwrap();
        fs::write(dir.join("Rus_Spells.ini"), format!("spells {tag}")).unwrap();
        fs::create_dir_all(dir.join("Graphics")).unwrap();
        fs::write(dir.join("Graphics/knight.lit"), format!("picture {tag}")).unwrap();
    }

    fn game_zip(path: &Path, tag: &str) {
        make_zip(path, &[
            ("DiscordTimes/", ""),
            ("DiscordTimes/Maps_Rus/", ""),
            ("DiscordTimes/Maps_Rus/b.DTm", &format!("map b {tag}")),
            ("DiscordTimes/Rus_Units.ini", &format!("units {tag}")),
            ("DiscordTimes/Rus_Spells.ini", &format!("spells {tag}")),
        ]);
    }

    fn found_of(path: &Path) -> Found {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        examine(path, &name).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn a_game_replaces_the_install_and_removes_the_old_one() {
        let s = Scratch::new("replace");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "old");
        let archive = s.0.join("Download/mod.zip");
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        game_zip(&archive, "new");

        let done = import(&found_of(&archive), &install, &|_, _| {}).unwrap();
        assert_eq!(done, Done::Replaced);
        assert_eq!(fs::read_to_string(install.join("Rus_Units.ini")).unwrap(), "units new");
        assert!(install.join("Maps_Rus/b.DTm").is_file());
        assert!(!install.join("Maps_Rus/a.DTm").exists(), "the old install's files are gone");
        assert!(!install.join("Graphics").exists());
        assert!(!old_dir(&install).exists() && !staging_dir(&install).exists());
    }

    #[test]
    fn a_patch_is_written_over_the_install_and_keeps_the_rest() {
        let s = Scratch::new("patch");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "game");
        let archive = s.0.join("patch.zip");
        make_zip(&archive, &[("Rus_Spells.ini", "patched spells"), ("Maps_Rus/c.DTm", "map c")]);

        assert_eq!(import(&found_of(&archive), &install, &|_, _| {}).unwrap(), Done::Patched(2));
        assert_eq!(fs::read_to_string(install.join("Rus_Spells.ini")).unwrap(), "patched spells");
        assert_eq!(fs::read_to_string(install.join("Maps_Rus/c.DTm")).unwrap(), "map c");
        assert_eq!(fs::read_to_string(install.join("Maps_Rus/a.DTm")).unwrap(), "map a game");
        assert_eq!(fs::read_to_string(install.join("Rus_Units.ini")).unwrap(), "units game");
        assert!(!staging_dir(&install).exists(), "the staging folder is gone");
    }

    #[test]
    fn a_broken_archive_leaves_the_old_install_byte_identical() {
        let s = Scratch::new("broken");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "old");
        let before = tree(&install);
        // A stored entry whose bytes no longer match its CRC: the listing reads, the unpacking fails.
        let mut bytes = zip_bytes(&[("DiscordTimes/Rus_Units.ini", "units new"), ("DiscordTimes/Maps_Rus/b.DTm", "map b new")]);
        let at = bytes.windows(9).position(|w| w == b"units new").unwrap();
        bytes[at] = b'X';
        let archive = s.0.join("broken.zip");
        fs::write(&archive, bytes).unwrap();

        let found = found_of(&archive);
        assert!(matches!(import(&found, &install, &|_, _| {}), Err(Error::Unpack(_))));
        assert_eq!(tree(&install), before, "the install is untouched");
        assert!(!staging_dir(&install).exists() && !old_dir(&install).exists());
    }

    #[test]
    fn a_folder_that_is_not_an_install_is_refused_and_the_install_kept() {
        let s = Scratch::new("not-install");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "old");
        let before = tree(&install);
        // The listing names the unit list as a folder: the install's check fails once unpacked.
        let archive = s.0.join("odd.zip");
        make_zip(&archive, &[("Maps_Rus/", ""), ("Maps_Rus/b.DTm", "map b"), ("Rus_Units.ini/", "")]);

        assert_eq!(import(&found_of(&archive), &install, &|_, _| {}), Err(Error::NotInstall));
        assert_eq!(tree(&install), before);
        assert!(!staging_dir(&install).exists());
    }

    #[test]
    fn saves_beside_the_install_are_not_touched() {
        // The phones' layout: the saves are in the app's folder, the install in its own folder.
        let s = Scratch::new("saves");
        let app = s.0.join("Razdor");
        let install = app.join("DiscordTimes");
        fake_install(&install, "old");
        fs::create_dir_all(app.join("saves")).unwrap();
        fs::write(app.join("saves/game.rzsave"), b"the player's save").unwrap();
        let saves_before = tree(&app.join("saves"));
        let archive = s.0.join("game.zip");
        game_zip(&archive, "new");

        import(&found_of(&archive), &install, &|_, _| {}).unwrap();
        assert_eq!(tree(&app.join("saves")), saves_before);
        assert_eq!(fs::read(app.join("saves/game.rzsave")).unwrap(), b"the player's save");
    }

    #[test]
    fn an_archive_with_nothing_of_the_game_is_refused_and_nothing_is_written() {
        let s = Scratch::new("nothing");
        let archive = s.0.join("notes.zip");
        make_zip(&archive, &[("readme.txt", "hello"), ("docs/x.pdf", "pdf")]);
        let before = tree(&s.0);
        assert_eq!(examine(&archive, "notes.zip"), Err(Error::NotGame("notes.zip".into())));
        assert_eq!(tree(&s.0), before, "examining writes nothing");
    }

    #[test]
    fn a_patch_without_an_install_is_refused_and_nothing_is_created() {
        let s = Scratch::new("no-install");
        let archive = s.0.join("patch.zip");
        make_zip(&archive, &[("Rus_Spells.ini", "patched")]);
        let install = s.0.join("DiscordTimes");
        assert_eq!(import(&found_of(&archive), &install, &|_, _| {}), Err(Error::NoInstall));
        assert!(!install.exists() && !staging_dir(&install).exists());
    }

    #[test]
    fn a_game_imports_into_a_missing_install() {
        let s = Scratch::new("fresh");
        let install = s.0.join("DiscordTimes");
        let archive = s.0.join("game.zip");
        game_zip(&archive, "new");
        assert_eq!(import(&found_of(&archive), &install, &|_, _| {}), Ok(Done::Replaced));
        assert_eq!(fs::read_to_string(install.join("Rus_Units.ini")).unwrap(), "units new");
    }

    #[test]
    fn a_failed_swap_puts_the_old_install_back() {
        let s = Scratch::new("swap-fail");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "old");
        let before = tree(&install);
        // The staged folder does not exist: the second rename fails after the install moved away.
        assert!(swap_in(&install, &s.0.join("missing.new")).is_err());
        assert_eq!(tree(&install), before);
        assert!(!old_dir(&install).exists());
    }

    #[test]
    fn an_interrupted_swap_is_put_back_at_the_start() {
        let s = Scratch::new("recover");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "old");
        let before = tree(&install);
        fs::rename(&install, old_dir(&install)).unwrap();
        recover(&install);
        assert_eq!(tree(&install), before);
        // Nothing to put back: nothing happens.
        recover(&install);
        assert_eq!(tree(&install), before);
    }

    #[test]
    fn the_install_is_written_only_in_the_phones_unpack_folder() {
        let unpack = Path::new("/storage/emulated/0/DiscordTimes");
        assert_eq!(target(None, Some(unpack)), Some(unpack.to_path_buf()), "no install yet: the unpack folder");
        assert_eq!(target(Some(unpack), Some(unpack)), Some(unpack.to_path_buf()));
        assert_eq!(target(Some(Path::new("/home/me/Games/Discord Times")), Some(unpack)), None, "another install is never written");
        assert_eq!(target(None, None), None);
    }

    #[test]
    fn a_picture_of_a_replaced_file_gets_a_new_key() {
        // The picture cache keys on path, size and modification time: a replaced file must differ.
        let s = Scratch::new("picture-key");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "old");
        let picture = install.join("Graphics/knight.lit");
        let old_time = SystemTime::now() - Duration::from_secs(3600);
        fs::File::options().write(true).open(&picture).unwrap().set_modified(old_time).unwrap();
        let before = crate::dt::picture_cache::source_key(&picture).unwrap();

        let archive = s.0.join("game.zip");
        make_zip(&archive, &[
            ("DiscordTimes/", ""),
            ("DiscordTimes/Maps_Rus/", ""),
            ("DiscordTimes/Maps_Rus/b.DTm", "map b"),
            ("DiscordTimes/Rus_Units.ini", "units new"),
            // The same length as the old picture, so only the modification time tells them apart.
            ("DiscordTimes/Graphics/knight.lit", "PICTURE new"),
        ]);
        import(&found_of(&archive), &install, &|_, _| {}).unwrap();
        assert_eq!(fs::read(&picture).unwrap(), b"PICTURE new");
        let after = crate::dt::picture_cache::source_key(&picture).unwrap();
        assert_ne!(before, after, "the replaced picture is not served from the cache");
    }

    /// Sets the declared uncompressed size of the zip entry `name` (in its local header and its
    /// central directory record) to `declared`, leaving the stored bytes as they are.
    fn declare_size(bytes: &mut [u8], name: &str, declared: u32) {
        let name = name.as_bytes();
        // Signature, the uncompressed size's offset in the record, and the name's offset.
        let records: [(&[u8], usize, usize); 2] = [(&b"PK\x03\x04"[..], 22, 30), (&b"PK\x01\x02"[..], 24, 46)];
        for (sig, size_at, name_at) in records {
            let starts: Vec<usize> = (0..bytes.len().saturating_sub(4)).filter(|&p| &bytes[p..p + 4] == sig).collect();
            for p in starts {
                if bytes.get(p + name_at..p + name_at + name.len()) == Some(name) {
                    bytes[p + size_at..p + size_at + 4].copy_from_slice(&declared.to_le_bytes());
                }
            }
        }
    }

    #[test]
    fn a_zip_entry_that_unpacks_to_more_than_it_declares_leaves_the_old_install_byte_identical() {
        // A game whose unit list holds 100 bytes but declares 10: the listing looks small, the
        // unpacking must not write past what the entry declares.
        let s = Scratch::new("lying");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "old");
        let before = tree(&install);
        let units = "x".repeat(100);
        let mut bytes = zip_bytes(&[
            ("DiscordTimes/Maps_Rus/", ""),
            ("DiscordTimes/Maps_Rus/b.DTm", "map b"),
            ("DiscordTimes/Rus_Units.ini", &units),
        ]);
        declare_size(&mut bytes, "DiscordTimes/Rus_Units.ini", 10);
        let archive = s.0.join("lying.zip");
        fs::write(&archive, bytes).unwrap();

        let found = found_of(&archive);
        assert_eq!(found.size, 5 + 10, "the listing trusts the declared sizes");
        assert!(matches!(import(&found, &install, &|_, _| {}), Err(Error::Unpack(_))));
        assert_eq!(tree(&install), before, "the install is untouched");
        assert!(!staging_dir(&install).exists() && !old_dir(&install).exists(), "the side folders are removed");
    }

    #[test]
    fn a_patch_that_unpacks_to_more_than_it_declares_is_not_written_over_the_install() {
        let s = Scratch::new("lying-patch");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "game");
        let before = tree(&install);
        let spells = "x".repeat(100);
        let mut bytes = zip_bytes(&[("Rus_Spells.ini", &spells), ("Maps_Rus/c.DTm", "map c")]);
        declare_size(&mut bytes, "Rus_Spells.ini", 10);
        let archive = s.0.join("lying-patch.zip");
        fs::write(&archive, bytes).unwrap();

        assert!(matches!(import(&found_of(&archive), &install, &|_, _| {}), Err(Error::Unpack(_))));
        assert_eq!(tree(&install), before, "nothing is moved over the install");
        assert!(!staging_dir(&install).exists());
    }

    /// The 7z version of [`game_zip`]: the same entries.
    fn game_7z(path: &Path, tag: &str) {
        let mut w = sevenz_rust2::ArchiveWriter::create(path).unwrap();
        w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_directory("DiscordTimes"), None::<&[u8]>).unwrap();
        w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_directory("DiscordTimes/Maps_Rus"), None::<&[u8]>).unwrap();
        w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file("DiscordTimes/Maps_Rus/b.DTm"), Some(format!("map b {tag}").as_bytes())).unwrap();
        w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file("DiscordTimes/Rus_Units.ini"), Some(format!("units {tag}").as_bytes())).unwrap();
        w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file("DiscordTimes/Rus_Spells.ini"), Some(format!("spells {tag}").as_bytes())).unwrap();
        w.finish().unwrap();
    }

    /// The archive at `path` opened as a file, as a phone's picked document is.
    fn open_found(path: &Path) -> (File, Found) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let mut file = File::open(path).unwrap();
        let found = examine_file(&mut file, &name).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        (file, found)
    }

    #[test]
    fn a_game_archive_opened_as_a_file_replaces_the_install_in_zip_and_seven_zip() {
        for ext in ["zip", "7z"] {
            let s = Scratch::new(&format!("open-game-{ext}"));
            let install = s.0.join("DiscordTimes");
            fake_install(&install, "old");
            let archive = s.0.join(format!("game.{ext}"));
            if ext == "zip" { game_zip(&archive, "new") } else { game_7z(&archive, "new") }

            let (mut file, found) = open_found(&archive);
            assert_eq!(found.kind, Kind::Game, "{ext}");
            assert_eq!(import_file(&mut file, &found, &install, &|_, _| {}), Ok(Done::Replaced), "{ext}");
            assert_eq!(fs::read_to_string(install.join("Rus_Units.ini")).unwrap(), "units new", "{ext}");
            assert!(!install.join("Maps_Rus/a.DTm").exists(), "{ext}: the old install's files are gone");
            assert!(!old_dir(&install).exists() && !staging_dir(&install).exists(), "{ext}: no side folder is left");
            assert!(archive.is_file(), "{ext}: the player's archive is not touched");
        }
    }

    #[test]
    fn a_patch_opened_as_a_file_goes_over_the_install() {
        let s = Scratch::new("open-patch");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "game");
        let archive = s.0.join("patch.zip");
        make_zip(&archive, &[("Rus_Spells.ini", "patched spells"), ("Maps_Rus/c.DTm", "map c")]);

        let (mut file, found) = open_found(&archive);
        assert_eq!(import_file(&mut file, &found, &install, &|_, _| {}), Ok(Done::Patched(2)));
        assert_eq!(fs::read_to_string(install.join("Rus_Spells.ini")).unwrap(), "patched spells");
        assert_eq!(fs::read_to_string(install.join("Rus_Units.ini")).unwrap(), "units game");
    }

    #[test]
    fn an_unknown_archive_opened_as_a_file_is_refused_and_nothing_is_written() {
        let s = Scratch::new("open-unknown");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "old");
        let before = tree(&install);
        let archive = s.0.join("notes.zip");
        make_zip(&archive, &[("readme.txt", "hello")]);
        let mut file = File::open(&archive).unwrap();
        assert_eq!(examine_file(&mut file, "notes.zip"), Err(Error::NotGame("notes.zip".into())));
        assert_eq!(tree(&install), before);
    }

    #[test]
    fn an_archive_that_does_not_fit_is_refused_and_nothing_is_written() {
        let s = Scratch::new("no-space");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "old");
        let before = tree(&install);
        let archive = s.0.join("game.zip");
        game_zip(&archive, "new");
        let (mut file, found) = open_found(&archive);
        let needed = found.size + SPACE_MARGIN;

        let mut unpack = |staging: &Path, progress: &dyn Fn(u64, u64)| unpack::unpack_file(&mut file, &found, staging, progress);
        let short = stage_and_apply(&found, &install, &|_, _| {}, &|_| Some(needed - 1), &mut unpack);
        assert_eq!(short, Err(Error::NoSpace { needed, free: needed - 1 }));
        assert_eq!(tree(&install), before, "the install is untouched");
        assert!(!staging_dir(&install).exists() && !old_dir(&install).exists(), "nothing is staged");

        // Exactly enough room: the import goes ahead. A room that cannot be read is not refused.
        assert_eq!(stage_and_apply(&found, &install, &|_, _| {}, &|_| Some(needed), &mut unpack), Ok(Done::Replaced));
        assert_eq!(fs::read_to_string(install.join("Rus_Units.ini")).unwrap(), "units new");
    }

    #[test]
    fn an_unknown_room_is_not_refused() {
        let s = Scratch::new("unknown-room");
        let install = s.0.join("DiscordTimes");
        let archive = s.0.join("game.zip");
        game_zip(&archive, "new");
        let found = found_of(&archive);
        let mut unpack = |staging: &Path, progress: &dyn Fn(u64, u64)| unpack::unpack(std::slice::from_ref(&found), staging, progress);
        assert_eq!(stage_and_apply(&found, &install, &|_, _| {}, &|_| None, &mut unpack), Ok(Done::Replaced));
    }

    #[cfg(unix)]
    #[test]
    fn a_seekable_file_is_seekable_and_a_pipe_is_not() {
        use std::os::unix::io::FromRawFd;
        let s = Scratch::new("seekable");
        let archive = s.0.join("game.zip");
        game_zip(&archive, "new");
        assert!(is_seekable(&mut File::open(&archive).unwrap()));

        let mut fds = [0i32; 2];
        // SAFETY: `fds` holds room for the two descriptors pipe(2) writes.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        // SAFETY: both descriptors are fresh from pipe(2) and owned by these two files only.
        let (mut read, _write) = unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) };
        assert!(!is_seekable(&mut read));
    }

    #[cfg(unix)]
    #[test]
    fn the_free_room_is_read_from_the_folder_disk() {
        let s = Scratch::new("statvfs");
        let free = free_space(&s.0).expect("the disk of a folder can be read");
        assert!(free > 0);
        assert_eq!(free_space(&s.0.join("missing")), None, "a folder that does not exist has no room to read");
    }

    #[test]
    fn the_staging_and_old_folders_are_beside_the_install_in_the_app_folder() {
        // The phones' unpack folder is `<app folder>/DiscordTimes`: both side folders stay in the app folder.
        let install = Path::new("/data/user/0/io.github.indicozy.razdor/files/DiscordTimes");
        let app = install.parent().unwrap();
        assert_eq!(staging_dir(install).parent(), Some(app));
        assert_eq!(old_dir(install).parent(), Some(app));
        assert_eq!(staging_dir(install).file_name().unwrap(), "DiscordTimes.new");
        assert_eq!(old_dir(install).file_name().unwrap(), "DiscordTimes.old");
    }

    #[test]
    fn a_flat_game_without_the_exe_is_a_game_and_imports() {
        // The hub's game archive: its files at the root (no top folder), no DiscordTimes.exe.
        let s = Scratch::new("flat");
        let install = s.0.join("DiscordTimes");
        let archive = s.0.join("discord-times-cu.zip");
        make_zip(&archive, &[
            ("Maps_Rus/", ""),
            ("Maps_Rus/b.DTm", "map b"),
            ("Rus_Units.ini", "units new"),
            ("Rus_Spells.ini", "spells new"),
            ("_Global.ini", "[Global]\r\n"),
        ]);
        let found = found_of(&archive);
        assert_eq!(found.kind, Kind::Game);
        assert_eq!(import(&found, &install, &|_, _| {}), Ok(Done::Replaced));
        assert!(is_install(&install), "the install is readable without an exe");
        assert_eq!(fs::read_to_string(install.join("Rus_Units.ini")).unwrap(), "units new");
    }

    /// The real game archive named by `RAZDOR_GAME_ZIP` (the hub's game, unpacked elsewhere): it is
    /// examined, imported into a scratch folder, and the result is a whole Community Update build.
    #[test]
    fn the_real_game_archive_imports_as_a_whole_install() {
        let Some(zip) = std::env::var_os("RAZDOR_GAME_ZIP") else { return };
        let zip = PathBuf::from(zip);
        let s = Scratch::new("real-zip");
        let found = found_of(&zip);
        assert_eq!(found.kind, Kind::Game);
        let install = s.0.join("DiscordTimes");
        assert_eq!(import(&found, &install, &|_, _| {}), Ok(Done::Replaced));
        assert!(is_install(&install));
        let manifest = crate::dt::integrity::builtin().unwrap();
        let report = crate::dt::integrity::check(&install, &manifest, true);
        println!("real zip: integrity {report:?}");
        assert!(report.community, "the folder is a Community Update build: {report:?}");
        assert!(report.missing.iter().all(|m| !crate::dt::integrity::needed_to_play(m)), "nothing Razdor reads is missing: {:?}", report.missing);
        assert!(crate::dt::install::check_game_files(&install, &manifest).is_ok());
        assert!(crate::dt::install::DtInstall::load(&install).is_ok(), "the install loads without an exe");
    }

    #[test]
    fn a_room_check_refuses_only_a_known_archive_that_does_not_fit() {
        let dir = Path::new("/cache");
        assert_eq!(check_room(Some(10), dir, &|_| Some(10 + SPACE_MARGIN)), Ok(()), "exactly enough");
        assert_eq!(check_room(Some(10), dir, &|_| Some(10 + SPACE_MARGIN - 1)), Err(Error::NoSpace { needed: 10 + SPACE_MARGIN, free: 10 + SPACE_MARGIN - 1 }));
        assert_eq!(check_room(Some(10), dir, &|_| None), Ok(()), "an unknown room is not refused");
        assert_eq!(check_room(None, dir, &|_| Some(0)), Ok(()), "an unknown size is not refused here");
    }

    #[test]
    fn a_full_disk_is_recognised_by_its_kind_or_its_errno() {
        assert!(is_out_of_room(&io::Error::from(io::ErrorKind::StorageFull)));
        assert!(is_out_of_room(&io::Error::from_raw_os_error(28)), "ENOSPC");
        assert!(!is_out_of_room(&io::Error::from(io::ErrorKind::PermissionDenied)));
    }

    #[test]
    fn a_refused_import_deletes_nothing_not_even_a_stale_staging_folder() {
        let s = Scratch::new("refused-no-delete");
        let install = s.0.join("DiscordTimes");
        fake_install(&install, "old");
        // A staging folder left by an earlier import: a refused import must leave it alone.
        fake_install(&staging_dir(&install), "stale");
        let before_install = tree(&install);
        let before_staging = tree(&staging_dir(&install));
        let archive = s.0.join("game.zip");
        game_zip(&archive, "new");
        let (mut file, found) = open_found(&archive);
        let mut unpack = |staging: &Path, progress: &dyn Fn(u64, u64)| unpack::unpack_file(&mut file, &found, staging, progress);
        let refused = stage_and_apply(&found, &install, &|_, _| {}, &|_| Some(1), &mut unpack);
        assert!(matches!(refused, Err(Error::NoSpace { .. })));
        assert_eq!(tree(&install), before_install);
        assert_eq!(tree(&staging_dir(&install)), before_staging, "the stale staging folder is not deleted");
    }

    #[test]
    fn a_picked_file_that_is_no_archive_is_refused_as_not_an_archive() {
        let s = Scratch::new("not-archive");
        let picked = s.0.join("picked-archive");
        fs::write(&picked, b"plain text, not an archive").unwrap();
        let mut file = File::open(&picked).unwrap();
        assert_eq!(examine_file(&mut file, "picked-archive"), Err(Error::NotArchive));
    }

    #[test]
    fn the_budget_names_an_archive_over_either_limit() {
        assert_eq!(over_budget("a.zip", MAX_ARCHIVE_BYTES, MAX_UNPACKED_BYTES), None);
        assert_eq!(over_budget("a.zip", MAX_ARCHIVE_BYTES + 1, 0), Some(Error::TooBig("a.zip".into())));
        assert_eq!(over_budget("a.zip", 0, MAX_UNPACKED_BYTES + 1), Some(Error::TooBig("a.zip".into())));
    }
}
