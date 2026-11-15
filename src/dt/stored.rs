//! Game files as they sit in an install: the plain file, or `<name>.lz4` (an LZ4 frame whose header
//! holds the content size). The one resolver every reader of a game file goes through (app data
//! compression, backlog 1609 part 2: `docs/specs/appdata-compression.md`). Plain wins: when both
//! forms exist the plain file is the truth. Only `.ugs` and `.lit` are ever stored compressed
//! ([`is_compressible`]); everything else is read as before.
//!
//! No macroquad. Razdor writes `.lz4` files only through [`compress_file`], and only in a folder it
//! owns ([`owned`]): [`convert_owned`] converts the plain pictures there, after an unpack, an import,
//! a patch or a repair, and at the start of a load.

use super::install;
use super::DtError;
use lz4_flex::frame::{FrameDecoder, FrameEncoder, FrameInfo};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// The extension of a stored (compressed) game file, added to the logical name.
pub const LZ4_EXT: &str = "lz4";
/// Suffix of a conversion in progress; never read, deleted when found at start.
pub const PART_SUFFIX: &str = "part";

/// LZ4 frame magic number, little endian (`lz4_Frame_format.md`).
const FRAME_MAGIC: u32 = 0x184D_2204;
/// FLG byte: version bits (must be `01`), block checksums (bit 4), content size (bit 3), content
/// checksum (bit 2), dictionary id (bit 0).
const FLG_VERSION_MASK: u8 = 0b1100_0000;
const FLG_VERSION_1: u8 = 0b0100_0000;
const FLG_BLOCK_CHECKSUM: u8 = 0b0001_0000;
const FLG_CONTENT_SIZE: u8 = 0b0000_1000;
const FLG_CONTENT_CHECKSUM: u8 = 0b0000_0100;
const FLG_DICT_ID: u8 = 0b0000_0001;
/// The end mark of the blocks: a block size of 0.
const END_MARK: u32 = 0;
/// Bytes a frame header with a content size takes before the body: magic, FLG, BD, size.
const HEADER_LEN: usize = 4 + 1 + 1 + 8;
/// Largest preallocation taken on a header's word alone (the body still decides the length).
const MAX_PREALLOC: u64 = 64 << 20;

/// Whether the logical file name `name` may be stored compressed: `.ugs` and `.lit` only.
pub fn is_compressible(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.len() > 4 && (lower.ends_with(".ugs") || lower.ends_with(".lit"))
}

/// The logical name of the stored file `name` (`Knight.ugs.lz4` → `Knight.ugs`), or None when `name` is
/// not a stored form (no `.lz4`, or a `.lz4` of a name that may not be stored compressed).
pub fn logical_name_of(name: &str) -> Option<&str> {
    strip_lz4(name).filter(|logical| is_compressible(logical))
}

/// `name` without a trailing `.lz4` (any case), or None.
fn strip_lz4(name: &str) -> Option<&str> {
    let at = name.len().checked_sub(LZ4_EXT.len() + 1)?;
    let dot = name.is_char_boundary(at) && name[at..].starts_with('.') && name[at + 1..].eq_ignore_ascii_case(LZ4_EXT);
    dot.then(|| &name[..at])
}

fn invalid(what: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.into())
}

/// A game file as stored: its path (plain, or `<name>.lz4`) and whether it is compressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    pub path: PathBuf,
    pub compressed: bool,
}

impl Stored {
    /// The stored form of a path a reader was given: a `<name>.lz4` path is compressed, any other
    /// path is plain. Does not look at the disk.
    pub fn at(path: &Path) -> Stored {
        let compressed = path.file_name().is_some_and(|n| strip_lz4(&n.to_string_lossy()).is_some());
        Stored { path: path.to_path_buf(), compressed }
    }

    /// The logical file name: the stored name without `.lz4`.
    pub fn logical_name(&self) -> String {
        let name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match strip_lz4(&name) {
            Some(stem) if self.compressed => stem.to_string(),
            _ => name,
        }
    }

    /// The length of the decompressed bytes: the frame header's content size, or the file's length
    /// when plain. Reads only the header of a compressed file.
    pub fn logical_len(&self) -> io::Result<u64> {
        if !self.compressed {
            return match fs::metadata(&self.path) {
                Ok(meta) => Ok(meta.len()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => self.converted().ok_or(e)?.logical_len(),
                Err(e) => Err(e),
            };
        }
        let mut head = Vec::with_capacity(HEADER_LEN);
        File::open(&self.path)?.take(HEADER_LEN as u64).read_to_end(&mut head)?;
        header_content_size(&head).ok_or_else(|| invalid(format!("{}: no content size in the frame header", self.path.display())))
    }

    /// The decompressed bytes. A compressed file must have a content size, and the decoded length
    /// must match it.
    pub fn read(&self) -> io::Result<Vec<u8>> {
        let bytes = match fs::read(&self.path) {
            // The background conversion ([`compress_owned_in_background`]) may have turned the plain
            // file into `<name>.lz4` since it was found: read that one.
            Err(e) if e.kind() == io::ErrorKind::NotFound => match self.converted() {
                Some(now) => return now.read(),
                None => return Err(e),
            },
            other => other?,
        };
        if !self.compressed {
            return Ok(bytes);
        }
        decode(&bytes).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", self.path.display())))
    }

    /// The `<name>.lz4` a plain picture became (it is gone, the stored form is there), or None.
    fn converted(&self) -> Option<Stored> {
        if self.compressed || !is_compressible(&self.logical_name()) {
            return None;
        }
        let name = self.path.file_name()?.to_string_lossy().into_owned();
        let lz4 = self.path.with_file_name(format!("{name}.{LZ4_EXT}"));
        lz4.is_file().then_some(Stored { path: lz4, compressed: true })
    }

    /// The form of this file that is there now: this one, or the `.lz4` a plain picture became in a
    /// conversion since it was found. None when neither is. A reader that checks a found file with
    /// `is_file` must use this: the converter may have moved the picture in between.
    pub fn present(&self) -> Option<Stored> {
        if self.path.is_file() {
            Some(self.clone())
        } else {
            self.converted()
        }
    }
}

/// The content size in an LZ4 frame header, or None when the bytes are not a frame or the header
/// has no content size. Only the fixed part and the size are read; the body is not checked here.
fn header_content_size(bytes: &[u8]) -> Option<u64> {
    let magic = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?);
    if magic != FRAME_MAGIC {
        return None;
    }
    let flg = *bytes.get(4)?;
    if flg & FLG_VERSION_MASK != FLG_VERSION_1 || flg & FLG_CONTENT_SIZE == 0 || bytes.len() < HEADER_LEN {
        return None;
    }
    let size: [u8; 8] = bytes.get(6..14)?.try_into().ok()?;
    Some(u64::from_le_bytes(size))
}

/// Whether `bytes` is a whole frame: the blocks run up to the end mark, and the optional checksums
/// after it end exactly at the last byte. The decoder alone takes a cut at a block boundary for a
/// clean end, so this walk is what makes a truncated frame an error.
fn is_whole_frame(bytes: &[u8]) -> bool {
    let Some(&flg) = bytes.get(4) else { return false };
    let word = |at: usize| -> Option<u32> { Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?)) };
    let flagged = |bit: u8, len: usize| if flg & bit != 0 { len } else { 0 };
    // magic, FLG, BD, the optional content size and dictionary id, then the header checksum byte.
    let mut at = 6 + flagged(FLG_CONTENT_SIZE, 8) + flagged(FLG_DICT_ID, 4) + 1;
    loop {
        let Some(size) = word(at) else { return false };
        at += 4;
        if size == END_MARK {
            break;
        }
        // The top bit marks an uncompressed block; its length is the other 31 bits.
        at += (size & 0x7FFF_FFFF) as usize + flagged(FLG_BLOCK_CHECKSUM, 4);
        if at > bytes.len() {
            return false;
        }
    }
    at + flagged(FLG_CONTENT_CHECKSUM, 4) == bytes.len()
}

/// Decodes a whole LZ4 frame whose header gives the content size; the decoded length must match.
fn decode(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let size = header_content_size(bytes).ok_or_else(|| invalid("no content size in the frame header"))?;
    if !is_whole_frame(bytes) {
        return Err(invalid("the frame is truncated or has bytes after its end"));
    }
    let mut out = Vec::with_capacity(size.min(MAX_PREALLOC) as usize);
    FrameDecoder::new(bytes).read_to_end(&mut out)?;
    if out.len() as u64 != size {
        return Err(invalid(format!("decoded {} bytes, the frame says {size}", out.len())));
    }
    Ok(out)
}

/// The entry of `dir` named `name` (exact first, then ignoring case): any entry when `files_only`
/// is false, else a file only.
fn named(dir: &Path, name: &str, files_only: bool) -> Option<PathBuf> {
    let ok = |p: &Path| if files_only { p.is_file() } else { p.exists() };
    let exact = dir.join(name);
    if ok(&exact) {
        return Some(exact);
    }
    fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)) && ok(p))
}

/// The stored form of the logical file `logical` in `dir` (matched ignoring case, as the game's
/// files come from Windows): the plain file if there is one, else `<logical>.lz4`. Only
/// [`is_compressible`] names have a `.lz4` form. A leftover `.part` is never a match.
pub fn find(dir: &Path, logical: &str) -> Option<Stored> {
    find_here(dir, logical).or_else(|| install::overlay_fallback(dir).and_then(|base| find(&base, logical)))
}

/// [`find`] in `dir` alone (a mod's merged folder without the base under it).
fn find_here(dir: &Path, logical: &str) -> Option<Stored> {
    if let Some(path) = named(dir, logical, false) {
        return Some(Stored { path, compressed: false });
    }
    if !is_compressible(logical) {
        return None;
    }
    named(dir, &format!("{logical}.{LZ4_EXT}"), true).map(|path| Stored { path, compressed: true })
}

/// The stored form of `rel` (`/`-separated, each component matched ignoring case as
/// [`install::find_path`] matches them). Only the last component may be `.lz4`.
pub fn find_path(base: &Path, rel: &str) -> Result<Stored, DtError> {
    let parts: Vec<&str> = rel.split('/').filter(|c| !c.is_empty()).collect();
    let Some((last, dirs)) = parts.split_last() else {
        return Err(DtError::Io { path: base.to_path_buf(), source: io::ErrorKind::NotFound.into() });
    };
    let dir = install::find_path(base, &dirs.join("/"))?;
    find(&dir, last).ok_or_else(|| DtError::Io { path: dir.join(last), source: io::ErrorKind::NotFound.into() })
}

/// The path of the stored form of `rel` (see [`find_path`]): what the picture readers open.
pub fn find_file(base: &Path, rel: &str) -> Result<PathBuf, DtError> {
    find_path(base, rel).map(|s| s.path)
}

/// The game files of `dir` (files only) by logical name, sorted by it: `.lz4` stripped, a name
/// listed once, the plain file winning. Other `.lz4` files (not compressible) are not listed.
pub fn list(dir: &Path) -> Vec<(String, Stored)> {
    let mut out = list_here(dir);
    // A mod's merged folder: the base's files it lacks are listed too (the merged folder's win).
    if let Some(base) = install::overlay_fallback(dir) {
        let seen: std::collections::HashSet<String> = out.iter().map(|(n, _)| n.to_ascii_lowercase()).collect();
        out.extend(list(&base).into_iter().filter(|(n, _)| !seen.contains(&n.to_ascii_lowercase())));
        out.sort_by(|a, b| a.0.cmp(&b.0));
    }
    out
}

/// [`list`] of `dir` alone.
fn list_here(dir: &Path) -> Vec<(String, Stored)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    // Keyed by the name ignoring case, so one file is listed once.
    let mut by_name: BTreeMap<String, (String, Stored)> = BTreeMap::new();
    for entry in entries.flatten() {
        // A plain picture converted after the listing was read is listed as its `.lz4` (the `.lz4` is
        // in place before the plain file goes, so the name is never lost).
        let listed = entry.path();
        let path = if listed.is_file() {
            listed
        } else {
            match Stored::at(&listed).present() {
                Some(now) if now.compressed => now.path,
                _ => continue,
            }
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.to_ascii_lowercase().ends_with(&format!(".{LZ4_EXT}.{PART_SUFFIX}")) {
            continue;
        }
        let (logical, compressed) = match strip_lz4(&name) {
            Some(stem) => (stem.to_string(), true),
            None => (name, false),
        };
        if compressed && !is_compressible(&logical) {
            continue;
        }
        // The plain file wins; between two stored forms the first one kept.
        let key = logical.to_ascii_lowercase();
        let keep_existing = by_name.get(&key).is_some_and(|(_, s)| !s.compressed || compressed);
        if !keep_existing {
            by_name.insert(key, (logical, Stored { path, compressed }));
        }
    }
    let mut out: Vec<(String, Stored)> = by_name.into_values().collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Converts the plain `.ugs`/`.lit` file `plain` to `<name>.lz4` and removes the plain file.
/// Crash-safe: the frame (with its content size) is written to `<name>.lz4.part`, fsynced,
/// decoded again and compared with the plain bytes, then renamed. The plain file goes last, so
/// a crash at any point leaves a readable install (plain wins). Returns the `.lz4` path.
/// Test and helper use for now; the converter of the next step calls it too.
pub fn compress_file(plain: &Path) -> io::Result<PathBuf> {
    let name = plain.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| invalid("no file name"))?;
    if !is_compressible(&name) {
        return Err(invalid(format!("{name}: only .ugs and .lit files are stored compressed")));
    }
    let data = fs::read(plain)?;
    let target = plain.with_file_name(format!("{name}.{LZ4_EXT}"));
    let part = plain.with_file_name(format!("{name}.{LZ4_EXT}.{PART_SUFFIX}"));
    let written = write_part(&part, &data).and_then(|()| {
        let back = decode(&fs::read(&part)?)?;
        if back != data {
            return Err(invalid(format!("{name}: the converted copy does not decode to the original")));
        }
        fs::rename(&part, &target)?;
        if let Some(dir) = target.parent() {
            // Best effort: the rename is durable on most file systems only once the folder is synced.
            let _ = File::open(dir).and_then(|d| d.sync_all());
        }
        fs::remove_file(plain)
    });
    if let Err(e) = written {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    Ok(target)
}

/// Writes `data` as an LZ4 frame with its content size to `part`, and syncs it.
fn write_part(part: &Path, data: &[u8]) -> io::Result<()> {
    let info = FrameInfo::new().content_size(Some(data.len() as u64)).content_checksum(true);
    let mut enc = FrameEncoder::with_frame_info(info, File::create(part)?);
    enc.write_all(data)?;
    let file = enc.finish()?;
    file.sync_all()
}

/// What a conversion of a folder did (see [`convert_owned`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Plain pictures compressed into `<name>.lz4` (the plain file removed).
    pub converted: usize,
    /// Of those, the plain file was the truth over an older `.lz4` of the same name, which was replaced.
    pub replaced: usize,
    /// Leftover `.lz4.part` files of an interrupted conversion, removed.
    pub parts_removed: usize,
    /// The bytes of the plain pictures that were converted.
    pub plain_bytes: u64,
    /// The bytes of the `.lz4` files they became.
    pub stored_bytes: u64,
}

/// Converts the plain pictures of the game folder `dir` to `<name>.lz4`, when Razdor owns the folder
/// ([`owned`]); any other folder is left exactly as it is (`Summary::default()`). Crash-safe at every
/// step: a leftover `.lz4.part` is deleted first; a plain file with an older `.lz4` of its name (any
/// case) drops that one before it is compressed from the plain file (plain wins); the plain file goes
/// last ([`compress_file`]). A second run finds nothing to do.
pub fn convert_owned(dir: &Path) -> io::Result<Summary> {
    if !owned(dir) {
        return Ok(Summary::default());
    }
    convert_tree(dir)
}

/// [`convert_owned`] for the callers that write game files (an unpack, an import, a patch, a repair,
/// the start of a load): the summary is logged when anything changed, and a failure is logged too.
/// The plain files stay readable, so nothing else is needed on a failure.
pub fn compress_owned(dir: &Path) {
    let _converting = Converting::enter(dir);
    log_conversion(dir, convert_owned(dir));
}

fn log_conversion(dir: &Path, result: io::Result<Summary>) {
    match result {
        Ok(s) if s != Summary::default() => crate::diag!(
            "stored: {}: {} pictures compressed ({} plain bytes to {} stored), {} replaced, {} leftover parts removed",
            dir.display(),
            s.converted,
            s.plain_bytes,
            s.stored_bytes,
            s.replaced,
            s.parts_removed
        ),
        Ok(_) => {}
        Err(e) => crate::diag!("stored: {}: the pictures are not compressed: {e}", dir.display()),
    }
}

/// [`compress_owned`] on a thread of its own, for the start of a load: the first start after an
/// update converts the whole folder (about 2 s on a desktop, longer on a phone), which must not hold
/// the game. Safe while the game reads: plain wins, a picture is first written as `<name>.lz4`, then
/// its plain file removed, and a read of a plain file that is gone reads the `.lz4` instead
/// ([`Stored::read`]). Whole-folder readers (a mod build) wait for it ([`wait_converted`]).
///
/// Ownership is checked and the folder marked converting on the calling thread, before the thread
/// starts: a reader that runs right after this call waits for the conversion, and (in tests) the
/// per-thread test ownership is seen.
pub fn compress_owned_in_background(dir: &Path) {
    if !owned(dir) {
        return;
    }
    let converting = Converting::enter(dir);
    let dir = dir.to_path_buf();
    let spawned = std::thread::Builder::new().name("compress-owned".into()).spawn(move || {
        // The guard is held until the conversion ends, also on a panic (it is dropped while unwinding).
        let _converting = converting;
        log_conversion(&dir, convert_tree(&dir));
    });
    if let Err(e) = spawned {
        // The closure (and with it the guard) was dropped by the failed spawn: the folder is free again.
        crate::diag!("stored: the pictures are not compressed (no thread: {e})");
    }
}

/// Folders a conversion is compressing now, with the number of conversions holding each (a
/// synchronous and a background one may overlap). Keys are canonical folders ([`folder_key`]).
static CONVERTING: std::sync::Mutex<BTreeMap<PathBuf, usize>> = std::sync::Mutex::new(BTreeMap::new());
/// Signalled when a conversion ends (a folder leaves [`CONVERTING`]).
static CONVERTED: std::sync::Condvar = std::sync::Condvar::new();

/// Holds a folder as converting until it is dropped (see [`compress_owned`], [`compress_owned_in_background`]).
struct Converting(PathBuf);

impl Converting {
    fn enter(dir: &Path) -> Converting {
        let key = folder_key(dir);
        let mut map = lock_converting();
        *map.entry(key.clone()).or_insert(0) += 1;
        Converting(key)
    }
}

impl Drop for Converting {
    fn drop(&mut self) {
        let mut map = lock_converting();
        if let Some(n) = map.get_mut(&self.0) {
            *n -= 1;
            if *n == 0 {
                map.remove(&self.0);
            }
        }
        CONVERTED.notify_all();
    }
}

fn lock_converting() -> std::sync::MutexGuard<'static, BTreeMap<PathBuf, usize>> {
    CONVERTING.lock().unwrap_or_else(|e| e.into_inner())
}

fn folder_key(dir: &Path) -> PathBuf {
    dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())
}

/// Waits while a conversion of the game folder `dir` is running (see [`compress_owned_in_background`]),
/// then returns. Returns whether it waited; a folder that is not converting returns at once.
/// A reader that walks the folder's files as a whole (a mod build, its fingerprint) calls this first,
/// so it never sees a folder that is half converted. Never call it from a conversion's own thread.
pub fn wait_converted(dir: &Path) -> bool {
    let key = folder_key(dir);
    let mut map = lock_converting();
    let mut waited = false;
    while map.contains_key(&key) {
        waited = true;
        map = CONVERTED.wait(map).unwrap_or_else(|e| e.into_inner());
    }
    waited
}

/// Whether a conversion of the game folder `dir` runs now (see [`wait_converted`]), without waiting: the
/// start's line is shown while it does (`ui::prep_line`). Never call it from a conversion's own thread.
pub fn is_converting(dir: &Path) -> bool {
    lock_converting().contains_key(&folder_key(dir))
}

/// Holds `dir` as converting for a test, as a conversion does (the guard ends it when dropped).
#[cfg(test)]
pub(crate) fn hold_converting_for_test(dir: &Path) -> impl Drop {
    Converting::enter(dir)
}

#[cfg(test)]
mod logical_name_tests {
    use super::*;

    /// Only a `.lz4` of a name that may be stored compressed is a stored form; any case.
    #[test]
    fn a_stored_name_is_the_compressible_name_with_lz4() {
        assert_eq!(logical_name_of("Knight.ugs.lz4"), Some("Knight.ugs"));
        assert_eq!(logical_name_of("a.LIT.LZ4"), Some("a.LIT"));
        assert_eq!(logical_name_of("Knight.ugs"), None, "a plain name is not a stored form");
        assert_eq!(logical_name_of("notes.txt.lz4"), None, "a name that may not be compressed");
        assert_eq!(logical_name_of("x.lz4"), None);
    }
}

#[cfg(test)]
mod background_tests {
    use super::*;

    /// A picture found plain and converted before it is read (the background conversion at start)
    /// is read from its `.lz4`: the same bytes and length, no error.
    #[test]
    fn a_plain_picture_converted_after_it_was_found_reads_its_lz4() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-stored-race-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.ugs"), b"unit picture bytes").unwrap();
        let found = find(&dir, "a.ugs").unwrap();
        assert!(!found.compressed);
        compress_file(&dir.join("a.ugs")).unwrap();
        assert_eq!(found.read().unwrap(), b"unit picture bytes");
        assert_eq!(found.logical_len().unwrap(), 18);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A found plain picture that is converted before the check ([`Stored::present`]) is present as its `.lz4`;
    /// a file that is in neither form is not.
    #[test]
    fn present_follows_a_picture_converted_after_it_was_found() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-stored-present-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.ugs"), b"unit picture bytes").unwrap();
        let found = find(&dir, "a.ugs").unwrap();
        assert!(found.present().is_some_and(|s| !s.compressed));
        compress_file(&dir.join("a.ugs")).unwrap();
        let now = found.present().expect("the .lz4 is there");
        assert!(now.compressed && now.path.ends_with("a.ugs.lz4"));
        fs::remove_file(dir.join("a.ugs.lz4")).unwrap();
        assert!(found.present().is_none(), "neither form is present");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The conversion gate ([`wait_converted`]): a folder that is not converting returns at once; while a
    /// conversion holds it, nothing returns; when the conversion ends, the wait returns.
    #[test]
    fn wait_converted_holds_while_a_conversion_runs_and_returns_when_it_ends() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-stored-gate-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(!wait_converted(&dir), "a folder that is not converting does not wait");
        let converting = Converting::enter(&dir);
        let (tx, rx) = std::sync::mpsc::channel();
        let waiter = dir.clone();
        std::thread::spawn(move || tx.send(wait_converted(&waiter)).unwrap());
        // While the conversion holds the folder, the waiter cannot have returned.
        assert!(rx.try_recv().is_err(), "the wait returned during a conversion");
        drop(converting);
        // Ends once the conversion is over (the waiter may also start after it: then it does not wait).
        rx.recv_timeout(std::time::Duration::from_secs(30)).expect("the wait returns after the conversion");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The non-blocking check ([`is_converting`]) follows the same guard as the wait does.
    #[test]
    fn is_converting_follows_the_conversion_guard() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-stored-converting-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(!is_converting(&dir), "a folder that is not converting");
        let converting = Converting::enter(&dir);
        assert!(is_converting(&dir), "a folder a conversion holds");
        drop(converting);
        assert!(!is_converting(&dir), "the conversion ended");
        let _ = fs::remove_dir_all(&dir);
    }
}

/// [`convert_tree`] for a folder Razdor wrote itself and owns by construction: a hub mod's files
/// (`hub/mods.rs`), whose plain pictures are compressed after an unpack, an import or a start. No
/// ownership check; the same rules as [`convert_owned`] (leftover parts removed, plain wins).
pub fn compress_folder(dir: &Path) -> io::Result<Summary> {
    convert_tree(dir)
}

/// The conversions running now, by folder: `(converted, plain pictures found)`. Made by `convert_tree` for
/// its run and removed when it ends; read by the start's progress bar ([`conversion_progress`]).
static PROGRESS: std::sync::Mutex<BTreeMap<PathBuf, (usize, usize)>> = std::sync::Mutex::new(BTreeMap::new());

/// How far a conversion of the game folder `dir` is, while it runs: `(converted, plain pictures found)`.
/// Cheap and non-blocking (the start's «Подготовка файлов игры…» bar, `ui::prep_line`); `None` when no
/// conversion of the folder runs.
pub fn conversion_progress(dir: &Path) -> Option<(usize, usize)> {
    PROGRESS.lock().unwrap_or_else(|e| e.into_inner()).get(&folder_key(dir)).copied()
}

/// One conversion's entry in [`PROGRESS`]: made when the conversion starts, removed when it is dropped.
struct Progress(PathBuf);

impl Progress {
    fn start(dir: &Path, total: usize) -> Progress {
        let key = folder_key(dir);
        PROGRESS.lock().unwrap_or_else(|e| e.into_inner()).insert(key.clone(), (0, total));
        Progress(key)
    }

    fn set(dir: &Path, done: usize) {
        if let Some(entry) = PROGRESS.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&folder_key(dir)) {
            entry.0 = done;
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        PROGRESS.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}

/// The plain pictures under `dir` (the ones [`convert_tree`] compresses), counted before it runs.
fn count_plain(dir: &Path) -> io::Result<usize> {
    let mut total = 0;
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                folders.push(entry.path());
            } else if kind.is_file() && is_compressible(&entry.file_name().to_string_lossy()) {
                total += 1;
            }
        }
    }
    Ok(total)
}

/// [`convert_owned`] for a folder already known to be owned (the walk, the rules of the converter).
fn convert_tree(dir: &Path) -> io::Result<Summary> {
    let total = count_plain(dir)?;
    let _shown = Progress::start(dir, total);
    let mut sum = Summary::default();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        // This folder's plain pictures, and its stored pictures by their logical name.
        let mut plain: Vec<PathBuf> = Vec::new();
        let mut stored: Vec<(String, PathBuf)> = Vec::new();
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            // Never follow a link out of the folder: only real folders and files are walked.
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                folders.push(path);
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.to_ascii_lowercase().ends_with(&format!(".{LZ4_EXT}.{PART_SUFFIX}")) {
                fs::remove_file(&path)?;
                sum.parts_removed += 1;
            } else if let Some(logical) = strip_lz4(&name).filter(|l| is_compressible(l)) {
                stored.push((logical.to_string(), path));
            } else if is_compressible(&name) {
                plain.push(path);
            }
        }
        for path in plain {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let mut replaced_any = false;
            for (_, old) in stored.iter().filter(|(logical, _)| logical.eq_ignore_ascii_case(&name)) {
                fs::remove_file(old)?;
                replaced_any = true;
            }
            let plain_len = fs::metadata(&path)?.len();
            let target = compress_file(&path)?;
            sum.converted += 1;
            Progress::set(dir, sum.converted);
            sum.replaced += usize::from(replaced_any);
            sum.plain_bytes += plain_len;
            sum.stored_bytes += fs::metadata(&target)?.len();
        }
    }
    Ok(sum)
}

/// The file Razdor writes into a game folder it unpacks or downloads into `platform::unpack_dir`.
/// Only a folder with it (on the desktop) is Razdor's own copy: see [`owned`].
pub const OWNED_MARKER: &str = ".razdor-owned";

/// Whether Razdor owns the game folder `dir`: the phones' unpack folder, or on the desktop that
/// folder with [`OWNED_MARKER`]. A player's own copy (a `DiscordTimes` folder next to the exe
/// without the marker), `RAZDOR_DT_DIR` and any other install are never owned. Only an owned
/// folder is converted and written as stored pictures (`docs/specs/appdata-compression.md`).
pub fn owned(dir: &Path) -> bool {
    owned_in(dir, crate::platform::unpack_dir().as_deref())
}

/// [`owned`] with the unpack folder given (`unpack`, None when the platform has none).
pub fn owned_in(dir: &Path, unpack: Option<&Path>) -> bool {
    if test_owned(dir) {
        return true;
    }
    let mobile = cfg!(any(target_os = "android", target_os = "ios"));
    unpack.is_some_and(|u| same_dir(dir, u)) && (mobile || dir.join(OWNED_MARKER).is_file())
}

/// Whether `dir` is the unpack folder on this platform (`platform::unpack_dir`): where an import
/// writes the game Razdor owns.
pub fn in_unpack_dir(dir: &Path) -> bool {
    crate::platform::unpack_dir().is_some_and(|u| same_dir(dir, &u))
}

/// Writes [`OWNED_MARKER`] into `dir`. The caller decides that `dir` is Razdor's to mark.
pub fn mark_owned(dir: &Path) -> io::Result<()> {
    fs::write(dir.join(OWNED_MARKER), "Razdor's game folder: written by Razdor when it unpacks or downloads the game.\n")
}

/// Two paths name one folder (canonical when both exist, else as written).
fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
crate::local! {
    /// A test's folder that counts as owned (the gate's copy of the install; see [`set_test_owned`]).
    static TEST_OWNED: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Marks `dir` as owned on this thread, for a test (None: back to the real rule).
#[cfg(test)]
pub fn set_test_owned(dir: Option<PathBuf>) {
    TEST_OWNED.with(|t| *t.borrow_mut() = dir);
}

#[cfg(test)]
fn test_owned(dir: &Path) -> bool {
    TEST_OWNED.with(|t| t.borrow().as_deref().is_some_and(|p| same_dir(dir, p)))
}

#[cfg(not(test))]
fn test_owned(_dir: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for one test (throwaway, removed at the end of the test).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-stored-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes `data` as the frame of `name` in `dir` (with or without its content size).
    fn write_frame(dir: &Path, name: &str, data: &[u8], content_size: bool) {
        let info = FrameInfo::new().content_size(content_size.then_some(data.len() as u64));
        let mut enc = FrameEncoder::with_frame_info(info, File::create(dir.join(name)).unwrap());
        enc.write_all(data).unwrap();
        enc.finish().unwrap();
    }

    fn sample() -> Vec<u8> {
        (0..5000u32).flat_map(|i| (i % 251).to_le_bytes()).collect()
    }

    #[test]
    fn only_ugs_and_lit_are_compressible() {
        assert!(is_compressible("Knight.UGS"));
        assert!(is_compressible("Win-paper.lit"));
        assert!(!is_compressible("Maps.DTm"));
        assert!(!is_compressible("sound.raw"));
        assert!(!is_compressible(".ugs.lz4"), "a stored name is not a logical one");
        assert!(!is_compressible(".lit"));
    }

    #[test]
    fn plain_file_wins_over_lz4() {
        let dir = scratch("plain-wins");
        fs::write(dir.join("a.lit"), b"plain").unwrap();
        write_frame(&dir, "a.lit.lz4", b"packed", true);
        let found = find(&dir, "a.lit").unwrap();
        assert!(!found.compressed);
        assert_eq!(found.read().unwrap(), b"plain");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lz4_is_found_ignoring_case_and_reads_back() {
        let dir = scratch("case");
        let data = sample();
        write_frame(&dir, "KNIGHT.UGS.LZ4", &data, true);
        let found = find(&dir, "knight.ugs").unwrap();
        assert!(found.compressed);
        assert_eq!(found.logical_name(), "KNIGHT.UGS");
        assert_eq!(found.logical_len().unwrap(), data.len() as u64);
        assert_eq!(found.read().unwrap(), data);
        assert!(find(&dir, "knight.dtm").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_strips_lz4_and_lists_each_name_once() {
        let dir = scratch("list");
        fs::write(dir.join("a.lit"), b"plain a").unwrap();
        write_frame(&dir, "a.lit.lz4", b"packed a", true);
        write_frame(&dir, "b.ugs.lz4", b"packed b", true);
        write_frame(&dir, "c.dtm.lz4", b"not a game file", true);
        fs::write(dir.join("x.lit.lz4.part"), b"half").unwrap();
        let names: Vec<(String, bool)> = list(&dir).into_iter().map(|(n, s)| (n, s.compressed)).collect();
        assert_eq!(names, vec![("a.lit".to_string(), false), ("b.ugs".to_string(), true)]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_frame_without_content_size_is_an_error() {
        let dir = scratch("no-size");
        write_frame(&dir, "n.lit.lz4", &sample(), false);
        let found = find(&dir, "n.lit").unwrap();
        assert!(found.logical_len().is_err());
        assert!(found.read().is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_truncated_frame_is_an_error_not_a_panic() {
        let dir = scratch("truncated");
        write_frame(&dir, "t.lit.lz4", &sample(), true);
        let full = fs::read(dir.join("t.lit.lz4")).unwrap();
        for cut in [3, HEADER_LEN - 1, HEADER_LEN + 2, full.len() / 2, full.len() - 1] {
            fs::write(dir.join("t.lit.lz4"), &full[..cut]).unwrap();
            let found = find(&dir, "t.lit").unwrap();
            assert!(found.read().is_err(), "cut at {cut}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compress_file_leaves_only_the_lz4_and_reads_back_identical() {
        let dir = scratch("compress");
        let data = sample();
        fs::write(dir.join("Pic.lit"), &data).unwrap();
        let target = compress_file(&dir.join("Pic.lit")).unwrap();
        assert_eq!(target, dir.join("Pic.lit.lz4"));
        let names: Vec<String> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["Pic.lit.lz4".to_string()]);
        let found = find(&dir, "Pic.lit").unwrap();
        assert!(found.compressed);
        assert_eq!(found.logical_len().unwrap(), data.len() as u64);
        assert_eq!(found.read().unwrap(), data);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compress_file_refuses_other_kinds() {
        let dir = scratch("refuse");
        fs::write(dir.join("Maps.DTm"), b"map").unwrap();
        assert!(compress_file(&dir.join("Maps.DTm")).is_err());
        assert!(dir.join("Maps.DTm").is_file(), "the plain file is untouched");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_leftover_part_file_is_ignored() {
        let dir = scratch("part");
        write_frame(&dir, "p.ugs.lz4.part", &sample(), true);
        assert!(find(&dir, "p.ugs").is_none());
        assert!(list(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn owned_is_the_unpack_folder_with_the_marker_on_the_desktop() {
        let root = scratch("owned");
        let unpack = root.join("DiscordTimes");
        let other = root.join("Games").join("DiscordTimes");
        fs::create_dir_all(&unpack).unwrap();
        fs::create_dir_all(&other).unwrap();
        // The unpack folder without the marker is a player's own copy until Razdor marks it.
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        assert_eq!(owned_in(&unpack, Some(&unpack)), mobile);
        mark_owned(&unpack).unwrap();
        assert!(owned_in(&unpack, Some(&unpack)));
        // Any other folder is never owned, marker or not (a RAZDOR_DT_DIR install, a Steam folder).
        mark_owned(&other).unwrap();
        assert!(!owned_in(&other, Some(&unpack)));
        // No unpack folder on this platform: nothing is owned.
        assert!(!owned_in(&unpack, None));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn owned_on_the_phones_needs_no_marker() {
        // The phones' rule, as the platform switch of `owned_in` decides it (checked as a pure rule).
        let root = scratch("owned-mobile");
        let unpack = root.join("DiscordTimes");
        fs::create_dir_all(&unpack).unwrap();
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        assert_eq!(owned_in(&unpack, Some(&unpack)), mobile || unpack.join(OWNED_MARKER).is_file());
        let _ = fs::remove_dir_all(&root);
    }

    /// The sorted file names of `dir`.
    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn convert_owned_leaves_an_unowned_folder_alone() {
        let dir = scratch("convert-unowned");
        fs::write(dir.join("Pic.lit"), sample()).unwrap();
        fs::write(dir.join("Pic.lit.lz4.part"), b"half").unwrap();
        set_test_owned(None);
        assert_eq!(convert_owned(&dir).unwrap(), Summary::default());
        assert_eq!(names_in(&dir), vec!["Pic.lit", "Pic.lit.lz4.part"], "a player's own copy is never touched");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The progress entry of a conversion: the plain pictures counted (the map is not one), and no
    /// entry once the conversion has ended.
    #[test]
    fn conversion_progress_counts_the_pictures_and_ends_with_the_conversion() {
        let dir = scratch("convert-progress");
        fs::create_dir_all(dir.join("Graphics")).unwrap();
        let data = sample();
        fs::write(dir.join("Graphics").join("Knight.ugs"), &data).unwrap();
        fs::write(dir.join("Pic.lit"), &data).unwrap();
        fs::write(dir.join("Maps.DTm"), b"map").unwrap();
        assert_eq!(count_plain(&dir).unwrap(), 2);
        assert_eq!(conversion_progress(&dir), None, "no conversion runs");
        set_test_owned(Some(dir.clone()));
        let sum = convert_owned(&dir).unwrap();
        set_test_owned(None);
        assert_eq!(sum.converted, 2);
        assert_eq!(conversion_progress(&dir), None, "the entry is removed when the conversion ends");
    }

    #[test]
    fn convert_owned_compresses_pictures_and_keeps_other_files() {
        let dir = scratch("convert-owned");
        fs::create_dir_all(dir.join("Graphics")).unwrap();
        let data = sample();
        fs::write(dir.join("Graphics").join("Knight.ugs"), &data).unwrap();
        fs::write(dir.join("Pic.lit"), &data).unwrap();
        fs::write(dir.join("Maps.DTm"), b"map").unwrap();
        fs::write(dir.join("sound.raw"), b"sound").unwrap();
        set_test_owned(Some(dir.clone()));
        let sum = convert_owned(&dir).unwrap();
        set_test_owned(None);
        assert_eq!(sum.converted, 2);
        assert_eq!(sum.replaced, 0);
        assert_eq!(sum.parts_removed, 0);
        assert_eq!(sum.plain_bytes, 2 * data.len() as u64);
        assert!(sum.stored_bytes > 0 && sum.stored_bytes < sum.plain_bytes, "the pictures shrink");
        assert_eq!(names_in(&dir), vec!["Graphics", "Maps.DTm", "Pic.lit.lz4", "sound.raw"]);
        assert_eq!(find(&dir, "Pic.lit").unwrap().read().unwrap(), data);
        assert_eq!(find(&dir.join("Graphics"), "Knight.ugs").unwrap().read().unwrap(), data);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn convert_owned_is_crash_safe_and_plain_wins_over_a_stale_lz4() {
        let dir = scratch("convert-crash");
        let fresh = sample();
        // A crash after the plain file was written and before a rename: a stale `.lz4` of the same
        // name (another case) and a `.part` of an interrupted conversion.
        fs::write(dir.join("pic.lit"), &fresh).unwrap();
        write_frame(&dir, "PIC.LIT.LZ4", b"stale picture", true);
        fs::write(dir.join("Knight.ugs.lz4.part"), b"half a frame").unwrap();
        fs::write(dir.join("Knight.ugs"), &fresh).unwrap();
        set_test_owned(Some(dir.clone()));
        let sum = convert_owned(&dir).unwrap();
        assert_eq!(sum.parts_removed, 1);
        assert_eq!(sum.converted, 2);
        assert_eq!(sum.replaced, 1, "the stale form is replaced by the plain file's");
        assert_eq!(names_in(&dir), vec!["Knight.ugs.lz4", "pic.lit.lz4"]);
        assert_eq!(find(&dir, "pic.lit").unwrap().read().unwrap(), fresh, "the plain file was the truth");
        assert_eq!(find(&dir, "Knight.ugs").unwrap().read().unwrap(), fresh);
        // The second run, after any crash point, changes nothing.
        let before = names_in(&dir);
        assert_eq!(convert_owned(&dir).unwrap(), Summary::default());
        assert_eq!(names_in(&dir), before);
        set_test_owned(None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_path_resolves_each_component_and_only_the_last_may_be_lz4() {
        let dir = scratch("path");
        fs::create_dir_all(dir.join("Graphics").join("Units")).unwrap();
        write_frame(&dir.join("Graphics").join("Units"), "Knight.ugs.lz4", &sample(), true);
        let found = find_path(&dir, "graphics/UNITS/knight.UGS").unwrap();
        assert!(found.compressed);
        assert!(find_path(&dir, "Graphics/Knight.ugs.lz4/x").is_err());
        assert!(find_path(&dir, "Graphics/Units/none.ugs").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Gate of step 1 (`docs/specs/appdata-compression.md`): a scratch copy of the real install,
    /// every `.ugs`/`.lit` converted by [`compress_file`], then the resolver reads every manifest
    /// picture byte-identical (SHA-256), the integrity check passes, `DtInstall::load` succeeds,
    /// and every picture decodes to the same RGBA as the plain install. Needs `RAZDOR_DT_DIR` and
    /// `RAZDOR_COMPRESS_SCRATCH` (a folder outside the install, for the copy); skips without them,
    /// and skips a source that already holds `.lz4` files (a run against the converted copy). The
    /// converted copy is left in `RAZDOR_COMPRESS_SCRATCH/install` for the tests that follow.
    #[test]
    fn gate_converted_copy_reads_identical_and_decodes_the_same() {
        let (Some(src), Some(scratch_root)) = (std::env::var_os(install::ENV_VAR), std::env::var_os("RAZDOR_COMPRESS_SCRATCH")) else { return };
        let src = PathBuf::from(src);
        let mut plain_files = Vec::new();
        collect_files(&src, &mut plain_files);
        if plain_files.iter().any(|p| Stored::at(p).compressed) {
            eprintln!("gate: {} already holds .lz4 files, skipped (run it on the plain install)", src.display());
            return;
        }
        fs::create_dir_all(&scratch_root).unwrap();
        let scratch_root = fs::canonicalize(&scratch_root).unwrap();
        let src = fs::canonicalize(&src).unwrap();
        let copy = scratch_root.join("install");
        assert!(!copy.starts_with(&src) && !src.starts_with(&scratch_root), "the scratch copy must not be the install or inside it");
        let _ = fs::remove_dir_all(&copy);
        copy_tree(&src, &copy).unwrap();

        // Convert every picture of the copy.
        let mut copied = Vec::new();
        collect_files(&copy, &mut copied);
        let mut converted = 0usize;
        let mut plain_bytes = 0u64;
        for p in copied.iter().filter(|p| is_compressible(&p.file_name().unwrap().to_string_lossy())) {
            plain_bytes += fs::metadata(p).unwrap().len();
            compress_file(p).unwrap();
            converted += 1;
        }
        let mut after = Vec::new();
        collect_files(&copy, &mut after);
        assert!(after.iter().all(|p| !is_compressible(&p.file_name().unwrap().to_string_lossy())), "no plain picture is left");
        let stored_bytes: u64 = after.iter().filter(|p| Stored::at(p).compressed).map(|p| fs::metadata(p).unwrap().len()).sum();
        let src_total: u64 = plain_files.iter().map(|p| fs::metadata(p).unwrap().len()).sum();
        let copy_total: u64 = after.iter().map(|p| fs::metadata(p).unwrap().len()).sum();
        eprintln!("gate: converted {converted} pictures: {plain_bytes} plain bytes -> {stored_bytes} stored bytes; install {src_total} -> {copy_total} bytes");

        // Every manifest picture reads back byte-identical through the resolver.
        let manifest = crate::dt::integrity::builtin().expect("the built-in manifest parses");
        let mut verified = 0usize;
        for f in manifest.files.iter().filter(|f| is_compressible(f.path.rsplit('/').next().unwrap_or_default())) {
            let s = find_path(&copy, &f.path).unwrap_or_else(|e| panic!("{}: {e}", f.path));
            assert!(s.compressed, "{} is stored compressed", f.path);
            let bytes = s.read().unwrap_or_else(|e| panic!("{}: {e}", f.path));
            assert_eq!(bytes.len() as u64, f.size, "{}", f.path);
            assert_eq!(crate::update::sha256_hex(&bytes), f.sha256, "{}", f.path);
            verified += 1;
        }
        eprintln!("gate: {verified} manifest pictures read byte-identical (SHA-256)");

        // The integrity check reports on the copy what it reports on the plain install (a player's
        // own edits, such as a patched exe, are changed in both), and the loader loads the copy.
        let report = crate::dt::integrity::check(&copy, &manifest, true);
        let plain = crate::dt::integrity::check(&src, &manifest, true);
        assert!(report.community && plain.community);
        assert_eq!((&report.missing, &report.changed), (&plain.missing, &plain.changed), "the copy checks like the plain install");
        assert!(report.missing.is_empty(), "nothing is missing: {:?}", report.missing);
        eprintln!("gate: integrity on the copy = plain install (changed in both: {:?})", report.changed);
        install::DtInstall::load(&copy).expect("the converted copy loads");

        // Every picture decodes to the same RGBA as in the plain install (no cache in between).
        let mut graphics = Vec::new();
        collect_graphics(&src.join("Graphics"), &mut graphics);
        let mut decoded = 0usize;
        for p in &graphics {
            let rel = p.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
            let plain = crate::dt::gfx::decode_uncached(p).unwrap_or_else(|e| panic!("{rel}: {e}"));
            let stored = find_path(&copy, &rel).unwrap_or_else(|e| panic!("{rel}: {e}"));
            let packed = crate::dt::gfx::decode_uncached(&stored.path).unwrap_or_else(|e| panic!("{rel} (stored): {e}"));
            assert_eq!(pixels_digest(&plain), pixels_digest(&packed), "{rel}");
            decoded += 1;
        }
        eprintln!("gate: {decoded} pictures decode the same in the copy");
        assert!(decoded > 0 && verified > 0 && converted > 0, "the gate ran on something");
    }

    /// Gate of step 2 (`docs/specs/appdata-compression.md`): a scratch copy of the real install, marked
    /// as owned for the test, converted by [`convert_owned`]: the timing and sizes are printed, the
    /// copy loads (`DtInstall::load`), its integrity report equals the plain install's (nothing
    /// missing), and a second run changes nothing. Needs `RAZDOR_DT_DIR` and `RAZDOR_COMPRESS_SCRATCH`
    /// (a folder outside the install; the copy is `<scratch>/copy` and is left there for a look).
    #[test]
    fn gate_convert_owned_copy_loads_checks_and_is_idempotent() {
        let (Some(src), Some(scratch_root)) = (std::env::var_os(install::ENV_VAR), std::env::var_os("RAZDOR_COMPRESS_SCRATCH")) else { return };
        let src = fs::canonicalize(PathBuf::from(src)).unwrap();
        fs::create_dir_all(&scratch_root).unwrap();
        let scratch_root = fs::canonicalize(&scratch_root).unwrap();
        let copy = scratch_root.join("copy");
        assert!(!copy.starts_with(&src) && !src.starts_with(&scratch_root), "the scratch copy must not be the install or inside it");
        let _ = fs::remove_dir_all(&copy);
        copy_tree(&src, &copy).unwrap();
        set_test_owned(Some(copy.clone()));

        let mut plain = Vec::new();
        collect_files(&copy, &mut plain);
        let plain_total: u64 = plain.iter().map(|p| fs::metadata(p).unwrap().len()).sum();
        let started = std::time::Instant::now();
        let first = convert_owned(&copy).unwrap();
        let seconds = started.elapsed().as_secs_f64();
        let mut all = Vec::new();
        collect_files(&copy, &mut all);
        let copy_total: u64 = all.iter().map(|p| fs::metadata(p).unwrap().len()).sum();
        eprintln!(
            "gate: convert_owned: {} pictures ({} parts removed, {} replaced) in {seconds:.2} s: {} plain bytes -> {} stored bytes; the folder {plain_total} -> {copy_total} bytes",
            first.converted, first.parts_removed, first.replaced, first.plain_bytes, first.stored_bytes
        );
        assert!(first.converted > 0, "the install has pictures");
        assert!(all.iter().all(|p| !is_compressible(&p.file_name().unwrap().to_string_lossy())), "no plain picture is left");

        // The copy loads, and checks like the plain install: nothing missing, the same changed files.
        let manifest = crate::dt::integrity::builtin().expect("the built-in manifest parses");
        let report = crate::dt::integrity::check(&copy, &manifest, true);
        let plain_report = crate::dt::integrity::check(&src, &manifest, true);
        assert!(report.community && plain_report.community);
        assert!(report.missing.is_empty(), "nothing is missing: {:?}", report.missing);
        assert_eq!((&report.missing, &report.changed), (&plain_report.missing, &plain_report.changed), "the copy checks like the plain install");
        install::DtInstall::load(&copy).expect("the converted copy loads");

        // The second run finds nothing to convert and changes nothing.
        let listing = |dir: &Path| {
            let mut files = Vec::new();
            collect_files(dir, &mut files);
            let mut rows: Vec<(PathBuf, u64)> = files.into_iter().map(|p| {
                let len = fs::metadata(&p).unwrap().len();
                (p, len)
            }).collect();
            rows.sort();
            rows
        };
        let before = listing(&copy);
        let again = convert_owned(&copy).unwrap();
        assert_eq!(again, Summary::default(), "a second run converts nothing");
        assert_eq!(listing(&copy), before, "a second run changes nothing");
        set_test_owned(None);
        eprintln!("gate: the converted copy loads, checks like the plain install, and a second run changes nothing");
    }

    /// Every file under `dir`, recursively.
    fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_files(&p, out);
            } else if p.is_file() {
                out.push(p);
            }
        }
    }

    /// Every `.ugs`, `.lit`, `.spi` and `.bmp` under `dir`, recursively.
    fn collect_graphics(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_graphics(&p, out);
            } else if p.extension().is_some_and(|x| matches!(x.to_string_lossy().to_ascii_lowercase().as_str(), "ugs" | "lit" | "spi" | "bmp")) {
                out.push(p);
            }
        }
    }

    fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
        fs::create_dir_all(dst)?;
        for e in fs::read_dir(src)?.flatten() {
            let (from, to) = (e.path(), dst.join(e.file_name()));
            if from.is_dir() {
                copy_tree(&from, &to)?;
            } else {
                fs::copy(&from, &to)?;
            }
        }
        Ok(())
    }

    /// The size and RGBA of every frame, as one digest.
    fn pixels_digest(frames: &[crate::dt::gfx::Image]) -> String {
        let mut all = Vec::new();
        for f in frames {
            all.extend_from_slice(&f.width.to_le_bytes());
            all.extend_from_slice(&f.height.to_le_bytes());
            all.extend_from_slice(&f.rgba);
        }
        crate::update::sha256_hex(&all)
    }
}
