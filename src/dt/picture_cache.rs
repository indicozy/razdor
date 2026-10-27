//! The persistent cache of decoded pictures: the RGBA of each install file's frames, kept in the
//! app's cache folder (`platform::cache_dir`, `pictures` in it, then one subfolder per release
//! and format, `pictures/1.1.3-f1`; outside the shared storage on the phones), so a picture is
//! decoded once per install file and not on every launch. Everything in it is derived from the
//! player's own install; nothing is written to the install folder or the repo.
//!
//! An entry is named by its key, a hash of the source file's path, size and modification time,
//! the cache format and Razdor's version (so a new release or a changed file misses). Its body
//! is the frames' size and RGBA bytes, then a checksum; a truncated or corrupt entry is rejected
//! and decoded again. Writes go to a temporary file that is renamed into place. At start
//! [`prune`] removes the other releases' folders, the old flat files and stale temporary files.
//! The folder's size is kept in a byte budget ([`Budget`]), so parallel writers cannot pass the
//! cap together. Snapshot runs (`RAZDOR_SNAPSHOT`) never read or write the cache, and neither
//! does `RAZDOR_NO_PICTURE_CACHE`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use super::gfx::Image;

/// The entry layout. Bump it when the layout changes.
pub const FORMAT: u32 = 1;
/// Past this size in the current folder nothing new is written. 1 GiB on the desktop, 256 MiB on
/// the phones (a player's report: the phones' cache grew to 2.30 GB under the old cap).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub const CAP_BYTES: u64 = 1 << 30;
/// Past this size in the current folder nothing new is written (see the desktop value above).
#[cfg(any(target_os = "android", target_os = "ios"))]
pub const CAP_BYTES: u64 = 256 << 20;

const MAGIC: &[u8; 4] = b"RZPC";
const HEADER: usize = 4 + 4 + 8 + 4;
const EXTENSION: &str = "rzpc";

/// Word-wise FNV-style checksum; fast enough for the pictures of a whole install.
fn checksum(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut chunks = bytes.chunks_exact(8);
    for c in &mut chunks {
        let mut w = [0u8; 8];
        w.copy_from_slice(c);
        h = (h ^ u64::from_le_bytes(w)).wrapping_mul(0x0100_0000_01b3);
    }
    for &b in chunks.remainder() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h ^ (bytes.len() as u64)
}

fn text_hash(text: &str) -> u64 {
    checksum(text.as_bytes())
}

/// The key of an install file: its path, size and modification time (nanoseconds), the format
/// and Razdor's version. `None` if the file cannot be looked at.
pub fn source_key(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    Some(key_for(&path.display().to_string(), meta.len(), mtime))
}

/// The key for a path with the given size and modification time (nanoseconds).
pub fn key_for(path: &str, len: u64, mtime_ns: u128) -> u64 {
    text_hash(&format!("{path}|{len}|{mtime_ns}|format {FORMAT}|razdor {}", env!("CARGO_PKG_VERSION")))
}

/// The entry of a key in `dir`.
pub fn entry_path(dir: &Path, key: u64) -> PathBuf {
    dir.join(format!("{key:016x}.{EXTENSION}"))
}

/// The entry's bytes: magic, format, key, frame count, then each frame's width, height and RGBA,
/// and a checksum of all of that.
pub fn encode(key: u64, frames: &[Image]) -> Vec<u8> {
    let body: usize = frames.iter().map(|f| 8 + f.rgba.len()).sum();
    let mut out = Vec::with_capacity(HEADER + body + 8);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(&key.to_le_bytes());
    out.extend_from_slice(&(frames.len() as u32).to_le_bytes());
    for f in frames {
        out.extend_from_slice(&f.width.to_le_bytes());
        out.extend_from_slice(&f.height.to_le_bytes());
        out.extend_from_slice(&f.rgba);
    }
    let sum = checksum(&out);
    out.extend_from_slice(&sum.to_le_bytes());
    out
}

/// The frames of an entry, if it is whole, has the format and key asked for and its checksum
/// holds. Anything else (a stale key, a truncated file, a flipped byte) is `None`.
pub fn decode(key: u64, bytes: &[u8]) -> Option<Vec<Image>> {
    if bytes.len() < HEADER + 8 || &bytes[..4] != MAGIC {
        return None;
    }
    let (body, tail) = bytes.split_at(bytes.len() - 8);
    if checksum(body) != u64::from_le_bytes(tail.try_into().ok()?) {
        return None;
    }
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap_or([0; 4]));
    if u32_at(4) != FORMAT || u64::from_le_bytes(bytes[8..16].try_into().ok()?) != key {
        return None;
    }
    let count = u32_at(16) as usize;
    let mut at = HEADER;
    let mut frames = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let end = at.checked_add(8)?;
        let (w, h) = (u32_at(at), u32_at(at + 4));
        let len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        let rgba_end = end.checked_add(len)?;
        if rgba_end > body.len() {
            return None;
        }
        frames.push(Image { width: w, height: h, rgba: body[end..rgba_end].to_vec() });
        at = rgba_end;
    }
    (at == body.len()).then_some(frames)
}

/// The name of the current release's folder in the cache: `1.1.3-f1` (Razdor's version and the
/// entry format). A new release writes to a new folder; [`prune`] removes the others.
pub fn version_folder() -> String {
    format!("{}-f{FORMAT}", env!("CARGO_PKG_VERSION"))
}

/// The root of the cache, `pictures` in `platform::cache_dir` (desktop: `razdor/cache/pictures`
/// in the platform's data folder), or `None` where the cache is off (snapshot runs,
/// `RAZDOR_NO_PICTURE_CACHE`) or there is no such folder.
fn root_dir() -> Option<PathBuf> {
    if std::env::var_os("RAZDOR_SNAPSHOT").is_some() || std::env::var_os("RAZDOR_NO_PICTURE_CACHE").is_some() {
        return None;
    }
    crate::platform::cache_dir().map(|d| d.join("pictures"))
}

/// The folder entries are written to and read from: the current release's subfolder of the
/// cache root (see [`root_dir`]), or `None` where the cache is off.
pub fn cache_dir() -> Option<PathBuf> {
    root_dir().map(|root| root.join(version_folder()))
}

/// Removes what no release reads any more, at start on a background thread: every subfolder of
/// the cache root other than the current release's, every plain file directly in the root (the
/// old flat layout), and in the current folder every temporary file not made by this process.
/// Nothing else is touched. Does nothing where the cache is off. Errors are ignored.
pub fn prune() {
    if let Some(root) = root_dir() {
        prune_in(&root, &version_folder());
    }
}

/// [`prune`] for the cache root `root` and the release folder `current`.
pub fn prune_in(root: &Path, current: &str) {
    let own_pid = std::process::id().to_string();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                if entry.file_name().to_str() != Some(current) {
                    let _ = std::fs::remove_dir_all(&path);
                }
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    let Ok(entries) = std::fs::read_dir(root.join(current)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // A temporary file is `.{key:016x}.{pid}.{nanos}.tmp`; this process's own are kept.
        if name.ends_with(".tmp") && name.split('.').nth(2) != Some(own_pid.as_str()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The bytes of the cache folder in use. It is counted from the folder on first use, then kept
/// by reservations: a writer reserves its entry's bytes before writing and gives them back if
/// the write fails. The check and the count happen under one lock, so parallel writers cannot
/// all pass the cap together.
#[derive(Default)]
pub struct Budget(Mutex<Option<u64>>);

impl Budget {
    pub const fn new() -> Self {
        Budget(Mutex::new(None))
    }

    /// Reserves `len` bytes in `dir` if they keep the folder within `cap`.
    fn reserve(&self, dir: &Path, len: u64, cap: u64) -> bool {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let used = guard.get_or_insert_with(|| folder_size(dir));
        if used.saturating_add(len) > cap {
            return false;
        }
        *used = used.saturating_add(len);
        true
    }

    /// Gives back `len` reserved bytes (a write that failed).
    fn release(&self, len: u64) {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(used) = guard.as_mut() {
            *used = used.saturating_sub(len);
        }
    }
}

/// The budget of the real cache.
static BUDGET: Budget = Budget::new();

/// The frames stored under `key` in `dir`, if the entry is whole and current.
pub fn read(dir: &Path, key: u64) -> Option<Vec<Image>> {
    decode(key, &std::fs::read(entry_path(dir, key)).ok()?)
}

/// Stores the frames under `key` in `dir`: a temporary file renamed into place. Best effort: a
/// full or unwritable folder, or one past [`CAP_BYTES`], just means no entry.
pub fn write(dir: &Path, key: u64, frames: &[Image]) {
    write_with(&BUDGET, CAP_BYTES, dir, key, frames);
}

/// [`write`] with its budget and cap given. An entry whose bytes do not fit the cap is not
/// written; a failed write gives its reserved bytes back.
fn write_with(budget: &Budget, cap: u64, dir: &Path, key: u64, frames: &[Image]) {
    let bytes = encode(key, frames);
    let len = bytes.len() as u64;
    if std::fs::create_dir_all(dir).is_err() || !budget.reserve(dir, len, cap) {
        return;
    }
    let nanos = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let tmp = dir.join(format!(".{key:016x}.{}.{nanos}.tmp", std::process::id()));
    if std::fs::write(&tmp, &bytes).is_err() || std::fs::rename(&tmp, entry_path(dir, key)).is_err() {
        let _ = std::fs::remove_file(&tmp);
        budget.release(len);
    }
}

fn folder_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |rd| rd.filter_map(|e| e.ok()?.metadata().ok()).map(|m| m.len()).sum())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32, seed: u8) -> Image {
        let rgba = (0..w * h * 4).map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed)).collect();
        Image { width: w, height: h, rgba }
    }

    #[test]
    fn round_trip_is_byte_identical() {
        let frames = vec![picture(5, 3, 1), picture(0, 0, 2), picture(7, 2, 3)];
        let key = key_for("a.lit", 10, 20);
        let bytes = encode(key, &frames);
        let back = decode(key, &bytes).expect("a whole entry reads back");
        assert_eq!(back, frames);
        assert_eq!(encode(key, &back), bytes);
    }

    #[test]
    fn a_stale_key_misses() {
        let frames = vec![picture(4, 4, 9)];
        let key = key_for("Graphics/Windows/Win-paper.lit", 100, 5);
        let bytes = encode(key, &frames);
        // A changed size, a changed time, another format or another release: other keys.
        assert!(decode(key_for("Graphics/Windows/Win-paper.lit", 101, 5), &bytes).is_none());
        assert!(decode(key_for("Graphics/Windows/Win-paper.lit", 100, 6), &bytes).is_none());
        assert!(decode(key ^ 1, &bytes).is_none());
        assert_ne!(entry_path(Path::new("d"), key), entry_path(Path::new("d"), key ^ 1));
        assert!(decode(key, &bytes).is_some());
    }

    #[test]
    fn a_truncated_or_corrupt_entry_is_rejected() {
        let key = key_for("b.lit", 1, 1);
        let bytes = encode(key, &[picture(6, 6, 4), picture(2, 2, 5)]);
        for cut in 0..bytes.len() {
            assert!(decode(key, &bytes[..cut]).is_none(), "a prefix of {cut} bytes must not read");
        }
        let mut flipped = bytes.clone();
        let mid = flipped.len() / 2;
        flipped[mid] ^= 0x40;
        assert!(decode(key, &flipped).is_none());
    }

    #[test]
    fn write_then_read_from_a_folder() {
        let dir = std::env::temp_dir().join(format!("razdor-pc-test-{}", std::process::id()));
        let key = key_for("c.lit", 3, 3);
        let frames = vec![picture(9, 4, 8)];
        write(&dir, key, &frames);
        assert_eq!(read(&dir, key), Some(frames));
        assert!(read(&dir, key ^ 2).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("razdor-pc-{name}-{}", std::process::id()))
    }

    #[test]
    fn pruning_keeps_only_the_current_versions_folder() {
        let root = scratch("prune");
        let _ = std::fs::remove_dir_all(&root);
        let own_tmp = format!(".0000000000000001.{}.5.tmp", std::process::id());
        let own_path = format!("1.1.3-f1/{own_tmp}");
        for (path, body) in [
            ("1.1.2-f1/a.rzpc", &b"old"[..]),
            ("1.1.3-f1/b.rzpc", &b"new"[..]),
            ("old.rzpc", &b"flat"[..]),
            ("1.1.3-f1/.x.99999999.1.tmp", &b"foreign"[..]),
            (own_path.as_str(), &b"mine"[..]),
        ] {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, body).unwrap();
        }
        prune_in(&root, "1.1.3-f1");
        let names = |dir: &Path| {
            let mut v: Vec<String> =
                std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            v.sort();
            v
        };
        assert_eq!(names(&root), vec!["1.1.3-f1".to_string()]);
        assert_eq!(names(&root.join("1.1.3-f1")), vec![own_tmp, "b.rzpc".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parallel_writes_never_pass_the_cap() {
        let dir = scratch("cap");
        let _ = std::fs::remove_dir_all(&dir);
        let budget = Budget::new();
        let cap: u64 = 64 << 10;
        std::thread::scope(|s| {
            for t in 0..8u64 {
                let (budget, dir) = (&budget, &dir);
                s.spawn(move || {
                    for i in 0..20u64 {
                        let key = key_for(&format!("thread{t}-{i}.lit"), 1, 1);
                        write_with(budget, cap, dir, key, &[picture(32, 32, t as u8)]);
                    }
                });
            }
        });
        let total = folder_size(&dir);
        let written = std::fs::read_dir(&dir).unwrap().flatten().count();
        assert!(total <= cap, "{total} bytes past the {cap} cap");
        assert!(written >= 1, "at least one entry was written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_write_gives_its_bytes_back() {
        let dir = scratch("release");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cap: u64 = 1 << 20;
        let budget = Budget::new();
        // The budget counts the folder from its first reservation.
        assert!(budget.reserve(&dir, 0, cap));
        let base = folder_size(&dir);
        // A directory where the entry should go makes the rename fail.
        let key = key_for("release.lit", 2, 2);
        std::fs::create_dir_all(entry_path(&dir, key)).unwrap();
        write_with(&budget, cap, &dir, key, &[picture(16, 16, 3)]);
        // The failed write's bytes are back: the whole rest of the cap is still free.
        assert!(budget.reserve(&dir, cap - base, cap));
        assert!(!budget.reserve(&dir, 1, cap));
        let _ = std::fs::remove_dir_all(&dir);
        // A reserve past the cap fails and reserves nothing (a missing folder counts as empty).
        let fresh = Budget::new();
        assert!(fresh.reserve(&dir, 60, 100));
        assert!(!fresh.reserve(&dir, 50, 100));
        fresh.release(60);
        assert!(fresh.reserve(&dir, 100, 100));
    }
}
