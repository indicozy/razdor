//! Packing a package folder into a `.rzpack` (`razdor-pack pack`): a zip of the package's files,
//! written the same way every time (entries in path order, one fixed timestamp, deflate, fixed
//! permissions), so the same folder gives the same bytes and the same SHA-256 whatever its files'
//! modification times are. The files are the ones `Files::load` reads, so the archive holds
//! exactly what `check` checked.

use std::io::{Cursor, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use super::{check, Files, Package};

/// A packed package: its bytes and their SHA-256 (lower-case hex).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packed {
    pub bytes: Vec<u8>,
    pub sha256: String,
}

/// The `.rzpack` bytes of `files`, in path order, with one timestamp (the zip epoch) for all.
pub fn zip(files: &Files) -> Result<Vec<u8>, String> {
    let when = DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).map_err(|e| format!("timestamp: {e}"))?;
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).last_modified_time(when).unix_permissions(0o644);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in files.iter() {
        writer.start_file(path, options).map_err(|e| format!("{path}: {e}"))?;
        writer.write_all(bytes).map_err(|e| format!("{path}: {e}"))?;
    }
    let out = writer.finish().map_err(|e| format!("the archive: {e}"))?;
    Ok(out.into_inner())
}

/// Packs the package folder (or `.rzpack`) at `path`. The package is checked first: an error
/// refuses it, and the error is the report's text.
pub fn pack(path: &Path) -> Result<Packed, String> {
    let files = Files::load(path)?;
    let (package, _) = Package::parse(files.clone());
    let report = check(&package);
    if report.has_errors() {
        return Err(report.to_text());
    }
    let bytes = zip(&files)?;
    let sha256 = crate::update::sha256_hex(&bytes);
    Ok(Packed { bytes, sha256 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/hello-story")
    }

    /// Copies the example into a fresh folder: its files get new modification times.
    fn copy_of_example(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dst = std::env::temp_dir().join(format!("razdor-pack-{tag}-{}-{nanos}", std::process::id()));
        let files = Files::from_dir(&example()).unwrap();
        for (rel, bytes) in files.iter() {
            let path = dst.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        dst
    }

    #[test]
    fn the_same_folder_packs_to_the_same_bytes_and_sha256() {
        let first = pack(&example()).expect("the example packs");
        let copy = copy_of_example("same");
        let second = pack(&copy).expect("the copy packs");
        let _ = std::fs::remove_dir_all(&copy);
        assert_eq!(first, second, "file times and the folder's place do not change the archive");
        assert_eq!(first.sha256.len(), 64);
        assert_eq!(first.sha256, crate::update::sha256_hex(&first.bytes));
    }

    #[test]
    fn the_archive_reads_back_to_the_same_files() {
        let packed = pack(&example()).unwrap();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let out = std::env::temp_dir().join(format!("razdor-pack-readback-{}-{nanos}.rzpack", std::process::id()));
        std::fs::write(&out, &packed.bytes).unwrap();
        let back = Files::load(&out).unwrap();
        let _ = std::fs::remove_file(&out);
        let original = Files::from_dir(&example()).unwrap();
        assert_eq!(back.paths().collect::<Vec<_>>(), original.paths().collect::<Vec<_>>());
        for (path, bytes) in original.iter() {
            assert_eq!(back.get(path), Some(bytes), "{path}");
        }
    }

    #[test]
    fn a_package_that_fails_the_check_is_not_packed() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-pack-bad-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.toml"), "id = \"bad\"\n").unwrap();
        let refused = pack(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let text = refused.expect_err("a broken manifest is refused");
        assert!(text.contains("error"), "{text}");
    }

    #[test]
    fn the_studio_folders_are_neither_read_nor_packed() {
        // `.editor/` (the studio's layout and stamps), `.git/` and `.claude/` are not package files:
        // a copy of the example with them in it reads and packs as the example alone.
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-pack-skip-{}-{nanos}", std::process::id()));
        let plain = Files::from_dir(&example()).unwrap();
        for (rel, bytes) in plain.iter() {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        for (rel, text) in [(".editor/stamps/x.toml", "x = 1\n"), (".git/HEAD", "ref: refs/heads/main\n"), (".claude/settings.json", "{}\n")] {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let read = Files::from_dir(&dir);
        let packed = pack(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let read = read.unwrap();
        for skipped in [".editor/stamps/x.toml", ".git/HEAD", ".claude/settings.json"] {
            assert!(read.get(skipped).is_none(), "{skipped} is not read");
        }
        assert!(read.problems.is_empty(), "a skipped folder gives no problem: {:?}", read.problems);
        assert_eq!(read.paths().collect::<Vec<_>>(), plain.paths().collect::<Vec<_>>());
        let packed = packed.expect("the package with the studio folders packs");
        assert_eq!(packed, pack(&example()).unwrap(), "the same bytes as the example alone");
    }

    #[test]
    fn an_archive_with_the_studio_folders_reads_without_them() {
        // A `.rzpack` that carries them (packed by hand) is read without them too.
        let mut files = Files::from_dir(&example()).unwrap();
        files.insert(".git/HEAD", "ref: refs/heads/main\n");
        files.insert(".editor/layout.json", "{}");
        files.insert(".claude/settings.json", "{}");
        let back = Files::from_zip(&zip(&files).unwrap()).unwrap();
        assert!(back.paths().all(|p| !super::super::files::is_skipped_top(p)), "{:?}", back.paths().collect::<Vec<_>>());
        assert_eq!(back.paths().count(), Files::from_dir(&example()).unwrap().paths().count());
    }
}
