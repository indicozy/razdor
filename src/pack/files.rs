//! The files of a content package, in memory: a folder or a `.rzpack` (a zip of the folder).
//!
//! A package is read from a path a save names, so the reading is bounded: the package's size,
//! its file count and each file's size are capped (`MAX_*`); a zip's declared sizes are checked
//! before anything is read and its reads stop at the caps, whatever it declares; a folder's
//! symlinks stay inside it; and a path to a network share or a device is refused before the file
//! system is touched ([`refused_path`]).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use super::diag::{Diagnostic, Severity};

/// The most a package's files may hold in all, uncompressed. The Hub takes uploads of 50 MB.
pub const MAX_PACKAGE_BYTES: u64 = 64 << 20;
/// The most files a package may have.
pub const MAX_FILES: usize = 4096;
/// The most a single file may hold.
pub const MAX_FILE_BYTES: u64 = 32 << 20;
/// How deep a folder package may nest.
const MAX_DEPTH: usize = 32;

/// The top-level folders that are never read or packed: the studio's own data (`.editor/`: layout,
/// stamps), a repository (`.git/`) and the author's agent settings (`.claude/`). Only at the
/// package's top level; a folder of the same name deeper in the package is a folder like any other.
pub const SKIPPED_TOP_FOLDERS: [&str; 3] = [".editor", ".git", ".claude"];

/// Whether a package-relative path lies in one of [`SKIPPED_TOP_FOLDERS`] (at the top level).
pub fn is_skipped_top(path: &str) -> bool {
    path.split_once('/').is_some_and(|(top, _)| SKIPPED_TOP_FOLDERS.contains(&top))
}

/// Package-relative path (`/` separators) to the file's bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Files {
    entries: BTreeMap<String, Vec<u8>>,
    /// Problems met while reading (unsafe archive paths, names that are not UTF-8, symlinks out
    /// of a folder, files that are not regular). The entries concerned are left out;
    /// `Package::parse` reports them.
    pub problems: Vec<Diagnostic>,
}

impl Files {
    pub fn insert(&mut self, path: &str, bytes: impl Into<Vec<u8>>) {
        self.entries.insert(path.to_string(), bytes.into());
    }

    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.entries.get(path).map(Vec::as_slice)
    }

    /// The file as text; `None` when it is missing or not UTF-8.
    pub fn text(&self, path: &str) -> Option<&str> {
        self.get(path).and_then(|b| std::str::from_utf8(b).ok())
    }

    /// Puts `bytes` at `path` in memory, replacing what was there: the studio's unsaved text.
    /// The path must be a package path in its canonical form (no `..`, no absolute path, no
    /// backslashes), the file within [`MAX_FILE_BYTES`], the file count within [`MAX_FILES`] and
    /// the package's size within [`MAX_PACKAGE_BYTES`]. Nothing changes when it is refused.
    pub fn overlay(&mut self, path: &str, bytes: impl Into<Vec<u8>>) -> Result<(), String> {
        if safe_path(path).as_deref() != Some(path) {
            return Err(format!("not a package path: {path:?}"));
        }
        let bytes = bytes.into();
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(format!("{path} is larger than {} MiB", MAX_FILE_BYTES >> 20));
        }
        let old = self.entries.get(path).map_or(0, Vec::len) as u64;
        if old == 0 && !self.entries.contains_key(path) && self.entries.len() >= MAX_FILES {
            return Err(format!("the package has more than {MAX_FILES} files (at {path})"));
        }
        let held: u64 = self.entries.values().map(|b| b.len() as u64).sum();
        if held - old + bytes.len() as u64 > MAX_PACKAGE_BYTES {
            return Err(format!("the package is larger than {} MiB (at {path})", MAX_PACKAGE_BYTES >> 20));
        }
        self.entries.insert(path.to_string(), bytes);
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.entries.iter().map(|(p, b)| (p.as_str(), b.as_slice()))
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Every file under `dir` (a path prefix such as `units/`).
    pub fn under<'a>(&'a self, dir: &'a str) -> impl Iterator<Item = (&'a str, &'a [u8])> + 'a {
        self.iter().filter(move |(p, _)| p.starts_with(dir))
    }

    /// Reads a folder recursively, within the limits. Symlinks to files inside the folder are
    /// read; a symlink to anything outside it, or to a folder, is a problem and is left out.
    pub fn from_dir(dir: &Path) -> Result<Files, String> {
        let root = std::fs::canonicalize(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let mut files = Files::default();
        let mut budget = Budget::default();
        collect(&root, &root, "", 0, &mut files, &mut budget)?;
        Ok(files)
    }

    /// Reads a `.rzpack`: a zip whose entries are the package's files, within the limits. The
    /// declared sizes are checked before any entry is read, and each read stops at the cap.
    pub fn from_zip(bytes: &[u8]) -> Result<Files, String> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("not a zip archive: {e}"))?;
        if archive.len() > MAX_FILES {
            return Err(format!("the package has more than {MAX_FILES} entries"));
        }
        // The declared sizes first: a package that declares more than the limits is refused
        // without reading a byte of it.
        let mut declared = Budget::default();
        for i in 0..archive.len() {
            let entry = archive.by_index_raw(i).map_err(|e| format!("cannot read entry {i}: {e}"))?;
            let skipped = safe_path(&String::from_utf8_lossy(entry.name_raw())).is_some_and(|p| is_skipped_top(&p));
            if !entry.is_dir() && !skipped {
                declared.admit(&String::from_utf8_lossy(entry.name_raw()), entry.size())?;
            }
        }
        let mut files = Files::default();
        let mut budget = Budget::default();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| format!("cannot read entry {i}: {e}"))?;
            if entry.is_dir() {
                continue;
            }
            let name = match entry.name() {
                Ok(n) => n.into_owned(),
                Err(_) => String::from_utf8_lossy(entry.name_raw()).into_owned(),
            };
            let Some(path) = safe_path(&name) else {
                files.problems.push(Diagnostic {
                    severity: Severity::Error,
                    file: name.clone(),
                    line: None,
                    message: "unsafe path in the archive: absolute paths, `..` and backslashes are not allowed".into(),
                    hint: Some("pack the package's folder as it is, without a parent folder entry".into()),
                });
                continue;
            };
            if is_skipped_top(&path) {
                continue;
            }
            let size = entry.size();
            budget.admit(&name, size)?;
            let data = budget.finish(&name, size, &mut entry)?;
            files.insert(&path, data);
        }
        Ok(files)
    }

    /// A folder or a `.rzpack` file. A path to a network share or a device is refused before
    /// anything is read ([`refused_path`]); a file that is not a regular file is refused; a
    /// `.rzpack` above [`MAX_PACKAGE_BYTES`] is refused before it is read.
    pub fn load(path: &Path) -> Result<Files, String> {
        if let Some(why) = refused_path(&path.to_string_lossy()) {
            return Err(format!("{} is {why}: a package is read from this computer only", path.display()));
        }
        let meta = std::fs::metadata(path).map_err(|_| format!("no such package folder or file: {}", path.display()))?;
        if meta.is_dir() {
            Files::from_dir(path)
        } else if meta.is_file() {
            if meta.len() > MAX_PACKAGE_BYTES {
                return Err(format!("{} is larger than {} MiB", path.display(), MAX_PACKAGE_BYTES >> 20));
            }
            let file = std::fs::File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let mut bytes = Vec::new();
            file.take(MAX_PACKAGE_BYTES + 1).read_to_end(&mut bytes).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            if bytes.len() as u64 > MAX_PACKAGE_BYTES {
                return Err(format!("{} is larger than {} MiB", path.display(), MAX_PACKAGE_BYTES >> 20));
            }
            Files::from_zip(&bytes)
        } else {
            Err(format!("{} is not a package folder or a .rzpack file", path.display()))
        }
    }
}

/// Why a path must not be opened, decided from its text alone, before the file system is
/// touched. Deny by default: `/` is read as `\`, and a path that starts with two separators is
/// refused (a network share, `\\host\share`, `//host/share`, or a device, `\\.\X`), as is any
/// path that starts with `\??\` (the NT namespace, which can reach a share too). The one local
/// form with two separators is the drive path `\\?\X:\…`, the form `canonicalize` gives on
/// Windows. `None` for a local path: `C:\…`, a relative path, a unix absolute path.
pub fn refused_path(path: &str) -> Option<&'static str> {
    let p: String = path.chars().map(|c| if c == '/' { '\\' } else { c }).collect();
    if let Some(rest) = p.strip_prefix(r"\??\") {
        return Some(if is_unc(rest) { "a network share" } else { "a device path" });
    }
    if let Some(rest) = p.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' {
            return None;
        }
        return Some(if is_unc(rest) { "a network share" } else { "a device path" });
    }
    if p.starts_with(r"\\") {
        return Some(if p.starts_with(r"\\.\") { "a device path" } else { "a network share" });
    }
    None
}

/// Whether `rest` (a path after its prefix) names a UNC share: `UNC\host\share`.
fn is_unc(rest: &str) -> bool {
    rest.get(..4).is_some_and(|s| s.eq_ignore_ascii_case("UNC\\"))
}

/// What a package read has admitted so far, against the limits.
#[derive(Default)]
struct Budget {
    files: usize,
    bytes: u64,
}

impl Budget {
    /// Admits a file of `declared` bytes before it is read: its count, its size and the
    /// package's size so far plus it must stay within the limits. The size is then reserved.
    fn admit(&mut self, name: &str, declared: u64) -> Result<(), String> {
        if self.files >= MAX_FILES {
            return Err(format!("the package has more than {MAX_FILES} files (at {name})"));
        }
        if declared > MAX_FILE_BYTES {
            return Err(format!("{name} is larger than {} MiB", MAX_FILE_BYTES >> 20));
        }
        if self.bytes.saturating_add(declared) > MAX_PACKAGE_BYTES {
            return Err(format!("the package is larger than {} MiB (at {name})", MAX_PACKAGE_BYTES >> 20));
        }
        self.files += 1;
        self.bytes += declared;
        Ok(())
    }

    /// Reads an admitted file. The reader is cut at the file limit, so a file that holds more
    /// than it declared fails here; the reservation is then replaced by the real size.
    fn finish(&mut self, name: &str, declared: u64, reader: impl Read) -> Result<Vec<u8>, String> {
        let mut data = Vec::new();
        reader.take(MAX_FILE_BYTES + 1).read_to_end(&mut data).map_err(|e| format!("cannot read {name}: {e}"))?;
        let actual = data.len() as u64;
        if actual > MAX_FILE_BYTES {
            return Err(format!("{name} is larger than {} MiB (more than its declared size)", MAX_FILE_BYTES >> 20));
        }
        self.bytes = self.bytes.saturating_sub(declared) + actual;
        if self.bytes > MAX_PACKAGE_BYTES {
            return Err(format!("the package is larger than {} MiB (at {name})", MAX_PACKAGE_BYTES >> 20));
        }
        Ok(data)
    }
}

/// Reads folder `dir` (inside the canonical `root`) into `files`.
fn collect(root: &Path, dir: &Path, prefix: &str, depth: usize, files: &mut Files, budget: &mut Budget) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("the package's folders nest deeper than {MAX_DEPTH} levels (at {prefix})"));
    }
    let read = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        if prefix.is_empty() && name.to_str().is_some_and(|n| SKIPPED_TOP_FOLDERS.contains(&n)) && entry.path().is_dir() {
            continue;
        }
        let Some(name) = name.to_str() else {
            files.problems.push(Diagnostic {
                severity: Severity::Error,
                file: format!("{prefix}{}", entry.file_name().to_string_lossy()),
                line: None,
                message: "file name is not UTF-8".into(),
                hint: Some("rename it with ASCII or UTF-8 characters".into()),
            });
            continue;
        };
        let path = entry.path();
        let rel = format!("{prefix}{name}");
        let kind = entry.file_type().map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        // The file that is opened. A symlink is refused unless its canonical target is a file
        // inside the package; that target is opened, never the link path, so the file that
        // was checked is the one read (no swap between the check and the read).
        let target = if kind.is_symlink() {
            match std::fs::canonicalize(&path).ok().filter(|t| t.starts_with(root)) {
                Some(t) => t,
                None => {
                    refuse(files, &rel, "a symbolic link out of the package folder is not read");
                    continue;
                }
            }
        } else if kind.is_dir() {
            collect(root, &path, &format!("{rel}/"), depth + 1, files, budget)?;
            continue;
        } else if !kind.is_file() {
            refuse(files, &rel, "not a regular file (a device, a pipe or a socket) is not read");
            continue;
        } else {
            path.clone()
        };
        // A pre-open filter, so that a device or a pipe is never opened (opening a pipe blocks).
        // The size and the kind are then taken from the opened handle, below.
        if !std::fs::metadata(&target).is_ok_and(|m| m.is_file()) {
            refuse(files, &rel, "not a regular file is not read");
            continue;
        }
        let file = std::fs::File::open(&target).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let meta = file.metadata().map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if !meta.is_file() {
            refuse(files, &rel, "not a regular file is not read");
            continue;
        }
        budget.admit(&rel, meta.len())?;
        let data = budget.finish(&rel, meta.len(), &file)?;
        files.insert(&rel, data);
    }
    Ok(())
}

/// Records a file of a folder that is left out.
fn refuse(files: &mut Files, rel: &str, message: &str) {
    files.problems.push(Diagnostic { severity: Severity::Error, file: rel.to_string(), line: None, message: message.into(), hint: None });
}

/// The package-relative path of a zip entry name, or `None` when it would leave the package.
fn safe_path(name: &str) -> Option<String> {
    if name.is_empty() || name.contains('\\') || name.starts_with('/') {
        return None;
    }
    let mut parts = Vec::new();
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            p if p.contains(':') => return None,
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip of `entries` (deflated, the default).
    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// Rewrites the declared uncompressed size of entry `name`, in its local and its central
    /// header: the archive then lies about the entry's size.
    fn declare_size(bytes: &mut [u8], name: &[u8], size: u32) {
        // (signature, offset of the size, offset of the name length, offset of the name)
        let headers: [(&[u8; 4], usize, usize, usize); 2] = [(b"PK\x03\x04", 22, 26, 30), (b"PK\x01\x02", 24, 28, 46)];
        let mut patched = 0;
        for (sig, size_at, name_len_at, name_at) in headers {
            let mut i = 0;
            while let Some(pos) = bytes[i..].windows(4).position(|w| w == sig) {
                let h = i + pos;
                let len = u16::from_le_bytes([bytes[h + name_len_at], bytes[h + name_len_at + 1]]) as usize;
                if &bytes[h + name_at..h + name_at + len] == name {
                    bytes[h + size_at..h + size_at + 4].copy_from_slice(&size.to_le_bytes());
                    patched += 1;
                }
                i = h + 4;
            }
        }
        assert_eq!(patched, 2, "the local and the central header");
    }

    #[test]
    fn safe_paths() {
        assert_eq!(safe_path("units/a.toml").as_deref(), Some("units/a.toml"));
        assert_eq!(safe_path("./units//a.toml").as_deref(), Some("units/a.toml"));
        assert_eq!(safe_path("../evil.toml"), None);
        assert_eq!(safe_path("units/../../evil.toml"), None);
        assert_eq!(safe_path("/etc/passwd"), None);
        assert_eq!(safe_path("C:/x.toml"), None);
        assert_eq!(safe_path("units\\..\\evil.toml"), None);
        assert_eq!(safe_path(""), None);
    }

    #[test]
    fn insert_and_text() {
        let mut f = Files::default();
        f.insert("package.toml", "id = \"x\"");
        f.insert("bin.dat", vec![0xff, 0xfe]);
        assert_eq!(f.text("package.toml"), Some("id = \"x\""));
        assert_eq!(f.text("bin.dat"), None);
        assert_eq!(f.under("package").count(), 1);
    }

    #[test]
    fn overlay_replaces_a_file_and_keeps_the_limits() {
        let mut f = Files::default();
        f.insert("units/a.toml", "old");
        f.overlay("units/a.toml", "new").unwrap();
        assert_eq!(f.text("units/a.toml"), Some("new"));
        f.overlay("units/b.toml", "added").unwrap();
        assert_eq!(f.paths().count(), 2);
        // Paths in canonical form only.
        for bad in ["../x.toml", "/x.toml", "units\\x.toml", "units//x.toml", "units/./x.toml", "", "C:/x.toml"] {
            assert!(f.overlay(bad, "x").is_err(), "{bad:?} is refused");
        }
        assert_eq!(f.paths().count(), 2, "a refused overlay changes nothing");
        // A file over the limit is refused, and the total over the package limit.
        let big = vec![0u8; MAX_FILE_BYTES as usize + 1];
        assert!(f.overlay("maps/x/map.toml", big).is_err());
        let part = vec![0u8; 22 << 20];
        f.overlay("a.bin", part.clone()).unwrap();
        f.overlay("b.bin", part.clone()).unwrap();
        let err = f.overlay("c.bin", part).unwrap_err();
        assert!(err.contains("larger than 64 MiB"), "{err}");
        // Replacing a file does not count its old size twice.
        f.overlay("b.bin", vec![0u8; 1]).unwrap();
        assert_eq!(f.get("b.bin").map(<[u8]>::len), Some(1));
    }

    #[test]
    fn network_and_device_paths_are_refused_and_local_ones_are_not() {
        for p in [r"\\host\share\pkg.rzpack", "//host/share/pkg.rzpack", r"\\?\UNC\host\share\pkg.rzpack", r"\\?\unc\host\share", r"\\server", r"\\.\PhysicalDrive0", r"\\?\Volume{1234}\x", r"\\?\GLOBALROOT\Device\x"] {
            assert!(refused_path(p).is_some(), "{p} must be refused");
        }
        assert_eq!(refused_path(r"\\?\UNC\host\share\pkg.rzpack"), Some("a network share"));
        assert_eq!(refused_path(r"\\.\NUL"), Some("a device path"));
        for p in [r"\\?\C:\packs\x.rzpack", "C:\\packs\\x", "/home/u/packs/x.rzpack", "packs/x", "relative.rzpack", "/"] {
            assert_eq!(refused_path(p), None, "{p} is local");
        }
    }

    #[test]
    fn the_nt_namespace_and_separator_pairs_are_refused_by_default() {
        assert_eq!(refused_path(r"\??\UNC\host\share"), Some("a network share"));
        assert_eq!(refused_path(r"\??\C:\x"), Some("a device path"));
        assert_eq!(refused_path("//host/share"), Some("a network share"));
        assert_eq!(refused_path(r"\\host\share"), Some("a network share"));
        assert_eq!(refused_path(r"\/host/share"), Some("a network share"));
        assert_eq!(refused_path("/??/C:/x"), Some("a device path"), "/ is read as \\");
        assert_eq!(refused_path(r"\\?\C:"), Some("a device path"), "a drive path needs its \\");
        assert_eq!(refused_path(r"\\?\C:\ok"), None);
        assert_eq!(refused_path("//?/C:/ok"), None, "/ is read as \\");
        assert_eq!(refused_path("/home/user/x"), None);
        assert_eq!(refused_path(r"C:\x"), None);
    }

    #[test]
    fn load_refuses_a_network_or_device_path_before_touching_it() {
        let err = Files::load(Path::new(r"\\no-such-host.invalid\share\pkg.rzpack")).unwrap_err();
        assert!(err.contains("a network share"), "{err}");
        let err = Files::load(Path::new(r"\\.\NUL")).unwrap_err();
        assert!(err.contains("a device path"), "{err}");
    }

    #[test]
    fn a_zip_round_trips() {
        let bytes = zip_bytes(&[("package.toml", b"id = \"x\""), ("units/a.toml", b"u")]);
        let files = Files::from_zip(&bytes).unwrap();
        assert_eq!(files.text("package.toml"), Some("id = \"x\""));
        assert_eq!(files.get("units/a.toml"), Some(&b"u"[..]));
    }

    #[test]
    fn a_package_with_too_many_files_is_refused() {
        let names: Vec<String> = (0..=MAX_FILES).map(|i| format!("f{i}.txt")).collect();
        let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
        let err = Files::from_zip(&zip_bytes(&entries)).unwrap_err();
        assert!(err.contains("more than 4096"), "{err}");
        assert_eq!(Files::from_zip(&zip_bytes(&entries[..MAX_FILES])).unwrap().paths().count(), MAX_FILES);
    }

    #[test]
    fn a_file_over_the_file_limit_is_refused() {
        let big = vec![0u8; MAX_FILE_BYTES as usize + 1];
        let err = Files::from_zip(&zip_bytes(&[("maps/big/terrain.png", &big)])).unwrap_err();
        assert!(err.contains("larger than 32 MiB"), "{err}");
        let max = vec![0u8; MAX_FILE_BYTES as usize];
        assert!(Files::from_zip(&zip_bytes(&[("maps/big/terrain.png", &max)])).is_ok(), "exactly the limit is read");
    }

    #[test]
    fn a_package_over_the_size_limit_is_refused_before_it_is_read() {
        let part = vec![0u8; 22 << 20];
        let err = Files::from_zip(&zip_bytes(&[("a.bin", &part), ("b.bin", &part), ("c.bin", &part)])).unwrap_err();
        assert!(err.contains("larger than 64 MiB"), "{err}");
        assert!(Files::from_zip(&zip_bytes(&[("a.bin", &part), ("b.bin", &part)])).is_ok(), "44 MiB is read");
    }

    #[test]
    fn a_zip_bomb_is_stopped_at_the_file_limit() {
        // 40 MiB of zeros deflates to a few dozen KB; the headers then declare 100 bytes.
        let mut bytes = zip_bytes(&[("maps/bomb/terrain.png", &vec![0u8; 40 << 20])]);
        declare_size(&mut bytes, b"maps/bomb/terrain.png", 100);
        // The zip reader stops an entry at its declared size, so the read fails; were it not
        // to, the file cap would. Either way nothing of the 40 MiB is kept.
        let err = Files::from_zip(&bytes).unwrap_err();
        assert!(err.contains("maps/bomb/terrain.png"), "{err}");
    }

    #[test]
    fn a_zip_declaring_more_than_the_limits_is_refused_before_any_read() {
        let mut bytes = zip_bytes(&[("maps/x/terrain.png", b"x")]);
        declare_size(&mut bytes, b"maps/x/terrain.png", (MAX_FILE_BYTES + 1) as u32);
        let err = Files::from_zip(&bytes).unwrap_err();
        assert!(err.contains("larger than 32 MiB"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_folder_is_refused() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-files-link-{}-{nanos}", std::process::id()));
        let pkg = dir.join("pkg");
        std::fs::create_dir_all(pkg.join("units")).unwrap();
        std::fs::write(dir.join("secret.toml"), "secret").unwrap();
        std::fs::write(pkg.join("package.toml"), "id = \"x\"").unwrap();
        std::os::unix::fs::symlink(dir.join("secret.toml"), pkg.join("units/a.toml")).unwrap();
        std::os::unix::fs::symlink("/dev/zero", pkg.join("zero.toml")).unwrap();
        let files = Files::from_dir(&pkg).unwrap();
        assert_eq!(files.paths().collect::<Vec<_>>(), vec!["package.toml"]);
        assert!(files.problems.iter().any(|d| d.file == "units/a.toml"), "{:?}", files.problems);
        assert!(files.problems.iter().any(|d| d.file == "zero.toml"), "{:?}", files.problems);
        // A link to a file inside the folder is read.
        std::fs::write(pkg.join("units/real.toml"), "real").unwrap();
        std::os::unix::fs::symlink(pkg.join("units/real.toml"), pkg.join("alias.toml")).unwrap();
        let files = Files::from_dir(&pkg).unwrap();
        assert_eq!(files.get("alias.toml"), Some(&b"real"[..]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_inside_the_package_reads_its_target() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-files-inside-{}-{nanos}", std::process::id()));
        let pkg = dir.join("pkg");
        std::fs::create_dir_all(pkg.join("units")).unwrap();
        std::fs::write(pkg.join("units/real.toml"), "real").unwrap();
        std::os::unix::fs::symlink(pkg.join("units/real.toml"), pkg.join("alias.toml")).unwrap();
        std::os::unix::fs::symlink("units", pkg.join("folder-link")).unwrap();
        let files = Files::from_dir(&pkg).unwrap();
        assert_eq!(files.get("alias.toml"), Some(&b"real"[..]));
        assert_eq!(files.get("units/real.toml"), Some(&b"real"[..]));
        assert!(files.get("folder-link").is_none(), "a link to a folder is not read as a file");
        assert!(files.problems.iter().any(|d| d.file == "folder-link"), "{:?}", files.problems);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
