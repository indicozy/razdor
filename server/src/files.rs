//! The zstd and save files on disk. Ids are server-made hex, so a path built from one stays inside
//! its folder. Writes go to a `.part` file first and are renamed, so a crash leaves no half file.

use std::io;
use std::path::{Path, PathBuf};

use crate::http::ApiError;

pub const BATTLES: &str = "battles";
pub const CRASHES: &str = "crashes";
pub const SAVES: &str = "saves";

/// A random id: 16 bytes as 32 lowercase hex characters.
pub fn new_id() -> Result<String, ApiError> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| ApiError::Internal(format!("random: {e}")))?;
    Ok(hex(&b))
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

/// Writes `bytes` to `path` so that a power cut leaves either the old file or the whole new one:
/// the `.part` file is fsynced before the rename, and the folder is fsynced after it (the rename
/// itself is durable only then).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent {
        std::fs::create_dir_all(parent)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let part = path.with_file_name(format!("{name}.part"));
    {
        let mut file = std::fs::File::create(&part)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&part, path)?;
    if let Some(parent) = parent {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

pub fn remove_quiet(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != io::ErrorKind::NotFound {
            tracing::warn!("remove {}: {e}", path.display());
        }
    }
}

/// Compresses on the blocking pool (a 4 MB record takes a few milliseconds).
pub async fn zstd_blocking(data: Vec<u8>) -> Result<Vec<u8>, ApiError> {
    tokio::task::spawn_blocking(move || zstd::encode_all(&data[..], 3))
        .await
        .map_err(|e| ApiError::Internal(format!("compress task: {e}")))?
        .map_err(|e| ApiError::Internal(format!("compress: {e}")))
}

pub fn unzstd(data: &[u8]) -> io::Result<Vec<u8>> {
    zstd::decode_all(data)
}

/// `root/<folder>/<name>`.
pub fn path(root: &Path, folder: &str, name: &str) -> PathBuf {
    root.join(folder).join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_the_file_and_leaves_no_part() {
        let dir = std::env::temp_dir().join(format!("razdor-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("save.bin");
        write_atomic(&file, b"first").expect("writes");
        write_atomic(&file, b"second").expect("overwrites");
        assert_eq!(std::fs::read(&file).expect("reads"), b"second");
        assert!(!dir.join("save.bin.part").exists(), "no .part file is left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
