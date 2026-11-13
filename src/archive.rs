//! One file taken out of a zip archive: the updater's program (the member of a release's zip) and
//! the hub's file wrapped in a zip (`hub::FileRef::wrapped`, a map on GitVerse). Nothing here needs
//! macroquad or a platform; the caller checks the bytes it gets (size and SHA-256).

use std::io::Read;
use std::path::Path;

/// The most a member may be.
pub const MAX_MEMBER: u64 = 256 << 20;

/// The bytes of `member`, a file of the zip archive at `zip`. Only that entry is read: the rest
/// of the archive is not unpacked. A name that is not a plain relative path (an absolute one, or
/// one with `..` or a drive) is refused before the archive is opened, as is a member over
/// [`MAX_MEMBER`].
pub fn member(zip: &Path, member: &str) -> Result<Vec<u8>, String> {
    if !plain_member(member) {
        return Err(format!("the archive's file {member:?} is not a plain name"));
    }
    let file = std::fs::File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("the archive cannot be read: {e}"))?;
    let entry = archive.by_name(member).map_err(|e| format!("the archive has no {member}: {e}"))?;
    let mut bytes = Vec::new();
    entry.take(MAX_MEMBER + 1).read_to_end(&mut bytes).map_err(|e| format!("{member}: {e}"))?;
    if bytes.len() as u64 > MAX_MEMBER {
        return Err(format!("{member} is bigger than {MAX_MEMBER} bytes"));
    }
    Ok(bytes)
}

/// A file name inside an archive: relative, `/`-separated, none empty, `.` or `..`, no drive letter.
pub fn plain_member(member: &str) -> bool {
    !member.is_empty() && !member.contains(['\\', ':']) && !member.starts_with('/') && member.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_name_that_leaves_the_folder_is_refused_before_the_archive_is_opened() {
        for bad in ["../razdor", "/etc/razdor", "bin/../../razdor", "C:razdor", "..\\razdor", "", "./", "a//b"] {
            assert!(!plain_member(bad), "{bad:?}");
            let err = member(Path::new("/nonexistent/pack.zip"), bad).unwrap_err();
            assert!(err.contains("not a plain name"), "{err}");
        }
        assert!(plain_member("razdor") && plain_member("bin/razdor") && plain_member("Razdor-1.5.2.exe"));
    }

    #[test]
    fn a_missing_member_or_a_broken_archive_is_an_error() {
        let dir = std::env::temp_dir().join(format!("razdor-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("one.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut w = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            w.start_file("a.DTm", opts).unwrap();
            std::io::Write::write_all(&mut w, b"map").unwrap();
            w.finish().unwrap();
        }
        assert_eq!(member(&zip_path, "a.DTm").unwrap(), b"map");
        assert!(member(&zip_path, "b.DTm").unwrap_err().contains("has no b.DTm"));
        let junk = dir.join("junk.zip");
        std::fs::write(&junk, b"not a zip at all").unwrap();
        assert!(member(&junk, "a.DTm").unwrap_err().contains("cannot be read"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
