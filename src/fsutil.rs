//! File writes that never leave half a file behind.

use std::path::Path;

/// Writes `bytes` to `path`: into a temporary file next to it, then renamed over it. Creates
/// the folder if needed.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.razdor-tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_over_an_old_file_and_leaves_no_temporary() {
        let dir = std::env::temp_dir().join(format!("razdor-fsutil-{}", std::process::id()));
        let path = dir.join("sub").join("file.txt");
        write_atomically(&path, b"one").unwrap();
        write_atomically(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(!dir.join("sub").join(".file.txt.razdor-tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
