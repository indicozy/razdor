//! The owner's copy of the Pages site (`https://razdor.indicozy.dev/` and `/wiki/`). The Pages workflow packs the
//! root copy (`tools/site/root_copy.py`) as `site.tar.gz` into the pre-release `site-latest` of indicozy/razdor;
//! `razdor-mirror` fetches it (digest checked), unpacks it here and swaps it in (`mod.rs`, `Sync::sync_site`).
//! This module is the part that touches the archive: it refuses anything but plain files and folders.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

/// The release and its one asset.
pub const SITE_TAG: &str = "site-latest";
pub const SITE_ASSET: &str = "site.tar.gz";
/// The most an unpacked copy may hold, and the most files: far above a real site, a guard against a bad archive.
pub const MAX_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_FILES: usize = 20_000;

/// The relative path an archive entry unpacks to: normal components only (no `/`, no `..`, no drive prefix).
/// `./` parts are dropped. None when the path must not be unpacked.
pub fn safe_rel(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            Component::RootDir | Component::ParentDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// Unpacks `archive` (a gzip-compressed tar) into `dest`, which must not exist yet. Only regular files and
/// folders are made: a symlink, a hard link, a device or any other entry type fails the whole unpack.
pub fn unpack(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    unpack_limited(file, dest, MAX_BYTES, MAX_FILES)
}

/// `unpack` with the limits given (the tests use small ones).
pub fn unpack_limited<R: Read>(reader: R, dest: &Path, max_bytes: u64, max_files: usize) -> Result<(), String> {
    if dest.exists() {
        return Err(format!("{} exists; an unpack needs a new folder", dest.display()));
    }
    fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(reader));
    let entries = tar.entries().map_err(|e| format!("archive: {e}"))?;
    let mut files = 0usize;
    let mut bytes = 0u64;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("archive: {e}"))?;
        // A pax global header carries metadata only (no file): skipped, its name is not used.
        if entry.header().entry_type().is_pax_global_extensions() {
            continue;
        }
        let raw = entry.path().map_err(|e| format!("archive: {e}"))?.into_owned();
        let rel = safe_rel(&raw).ok_or_else(|| format!("{}: a path that leaves the folder", raw.display()))?;
        let kind = entry.header().entry_type();
        if rel.as_os_str().is_empty() {
            if kind.is_dir() {
                continue;
            }
            return Err(format!("{}: an entry without a name", raw.display()));
        }
        let target = dest.join(&rel);
        match kind {
            tar::EntryType::Directory => {
                fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
            }
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                files += 1;
                if files > max_files {
                    return Err(format!("more than {max_files} files"));
                }
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                let remaining = max_bytes.saturating_sub(bytes);
                let mut out = File::create(&target).map_err(|e| format!("{}: {e}", target.display()))?;
                let n = io::copy(&mut (&mut entry).take(remaining.saturating_add(1)), &mut out)
                    .map_err(|e| format!("{}: {e}", target.display()))?;
                if n > remaining {
                    return Err(format!("more than {max_bytes} bytes unpacked"));
                }
                bytes += n;
            }
            other => {
                return Err(format!("{}: entry type {other:?} is not allowed (files and folders only)", raw.display()));
            }
        }
    }
    Ok(())
}

/// The unpacked copy must be a site: its front page and the wiki's front page are there.
pub fn check(dir: &Path) -> Result<(), String> {
    for must in ["index.html", "wiki/index.html"] {
        if !dir.join(must).is_file() {
            return Err(format!("the archive has no {must}: not a site copy"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tar::{EntryType, Header};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-site-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A tar with raw entry names (the names are written as given, `..` and `/` included).
    struct Archive {
        builder: tar::Builder<Vec<u8>>,
    }

    impl Archive {
        fn new() -> Archive {
            Archive { builder: tar::Builder::new(Vec::new()) }
        }

        fn raw(&mut self, name: &str, kind: EntryType, body: &[u8], link: &str) {
            let mut h = Header::new_gnu();
            h.set_entry_type(kind);
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            {
                let gnu = h.as_gnu_mut().unwrap();
                gnu.name = [0; 100];
                gnu.name[..name.len()].copy_from_slice(name.as_bytes());
                gnu.linkname = [0; 100];
                gnu.linkname[..link.len()].copy_from_slice(link.as_bytes());
            }
            h.set_cksum();
            self.builder.append(&h, body).unwrap();
        }

        fn file(mut self, name: &str, body: &[u8]) -> Archive {
            self.raw(name, EntryType::Regular, body, "");
            self
        }

        /// The archive as gzip bytes.
        fn gz(self) -> Vec<u8> {
            let tar_bytes = self.builder.into_inner().unwrap();
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            enc.write_all(&tar_bytes).unwrap();
            enc.finish().unwrap()
        }
    }

    fn unpack_bytes(bytes: &[u8], dest: &Path) -> Result<(), String> {
        unpack_limited(bytes, dest, MAX_BYTES, MAX_FILES)
    }

    #[test]
    fn the_relative_path_keeps_normal_parts_only() {
        assert_eq!(safe_rel(Path::new("./a/b.html")), Some(PathBuf::from("a/b.html")));
        assert_eq!(safe_rel(Path::new("wiki/x/index.html")), Some(PathBuf::from("wiki/x/index.html")));
        assert_eq!(safe_rel(Path::new("../x")), None);
        assert_eq!(safe_rel(Path::new("a/../../b")), None);
        assert_eq!(safe_rel(Path::new("/etc/passwd")), None);
    }

    #[test]
    fn a_plain_site_unpacks_and_passes_the_check() {
        let dir = temp("plain");
        let gz = Archive::new()
            .file("index.html", b"home")
            .file("wiki/index.html", b"wiki")
            .file("wiki/en/x.html", b"en")
            .gz();
        unpack_bytes(&gz, &dir.join("out")).unwrap();
        assert_eq!(fs::read(dir.join("out/wiki/en/x.html")).unwrap(), b"en");
        check(&dir.join("out")).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_symlink_fails_the_whole_unpack() {
        let dir = temp("symlink");
        let mut a = Archive::new().file("index.html", b"home");
        a.raw("link", EntryType::Symlink, b"", "/etc/passwd");
        assert!(unpack_bytes(&a.gz(), &dir.join("out")).unwrap_err().contains("not allowed"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hard_link_fails_the_whole_unpack() {
        let dir = temp("hardlink");
        let mut a = Archive::new();
        a.raw("link", EntryType::Link, b"", "index.html");
        assert!(unpack_bytes(&a.gz(), &dir.join("out")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_that_leaves_the_folder_fails_the_unpack() {
        let dir = temp("escape");
        for name in ["../escape.html", "wiki/../../escape.html", "/abs/escape.html"] {
            let a = Archive::new().file(name, b"x");
            assert!(unpack_bytes(&a.gz(), &dir.join("out")).is_err(), "{name} must be refused");
        }
        assert!(!dir.join("escape.html").exists());
        assert!(!dir.join("out/../escape.html").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_size_and_file_limits_stop_a_big_archive() {
        let dir = temp("limits");
        let gz = Archive::new().file("a", &[1u8; 100]).file("b", &[2u8; 100]).gz();
        assert!(unpack_limited(&gz[..], &dir.join("bytes"), 150, MAX_FILES).is_err());
        assert!(unpack_limited(&gz[..], &dir.join("files"), MAX_BYTES, 1).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unpack_needs_a_new_folder_and_a_site_needs_its_front_pages() {
        let dir = temp("exists");
        fs::create_dir_all(dir.join("out")).unwrap();
        let gz = Archive::new().file("index.html", b"home").gz();
        assert!(unpack_bytes(&gz, &dir.join("out")).is_err());
        let _ = fs::remove_dir_all(&dir);
        let dir = temp("nofront");
        unpack_bytes(&Archive::new().file("index.html", b"home").gz(), &dir.join("out")).unwrap();
        assert!(check(&dir.join("out")).unwrap_err().contains("wiki/index.html"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_archive_is_an_error_not_a_partial_site() {
        let dir = temp("corrupt");
        assert!(unpack_bytes(b"not gzip at all", &dir.join("out")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
