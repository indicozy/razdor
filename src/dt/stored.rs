//! Game files as they sit in an install: the plain file, or `<name>.lz4` (an LZ4 frame whose header
//! holds the content size). The one resolver every reader of a game file goes through (app data
//! compression, backlog 1609 part 2: `docs/specs/appdata-compression.md`). Plain wins: when both
//! forms exist the plain file is the truth. Only `.ugs` and `.lit` are ever stored compressed
//! ([`is_compressible`]); everything else is read as before.
//!
//! No macroquad. Step 1 only reads: Razdor writes `.lz4` files only through [`compress_file`],
//! which the tests and the next step use.

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
            return Ok(fs::metadata(&self.path)?.len());
        }
        let mut head = Vec::with_capacity(HEADER_LEN);
        File::open(&self.path)?.take(HEADER_LEN as u64).read_to_end(&mut head)?;
        header_content_size(&head).ok_or_else(|| invalid(format!("{}: no content size in the frame header", self.path.display())))
    }

    /// The decompressed bytes. A compressed file must have a content size, and the decoded length
    /// must match it.
    pub fn read(&self) -> io::Result<Vec<u8>> {
        let bytes = fs::read(&self.path)?;
        if !self.compressed {
            return Ok(bytes);
        }
        decode(&bytes).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", self.path.display())))
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
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    // Keyed by the name ignoring case, so one file is listed once.
    let mut by_name: BTreeMap<String, (String, Stored)> = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
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
