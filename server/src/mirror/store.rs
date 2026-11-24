//! The web root and the staging folder: atomic writes and renames, hard links, disk figures, the lock.

use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use super::github::OWNER;

/// Where a release's files live in the web root: `<root>/indicozy/<repo>/releases/download/<tag>`.
pub fn tag_dir(root: &Path, repo: &str, tag: &str) -> PathBuf {
    root.join(OWNER).join(repo).join("releases").join("download").join(tag)
}

/// The `latest/download` folder of a repository: a symlink to `../download/<tag>`.
pub fn latest_link(root: &Path, repo: &str) -> PathBuf {
    root.join(OWNER).join(repo).join("releases").join("latest").join("download")
}

pub fn sha256_bytes(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Writes `path` through a temporary file in the same folder and a rename, so a reader sees the old file or
/// the new one, never half of it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = tmp_name(path);
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

fn tmp_name(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Points `link` at `target` (a relative path) with a temporary symlink and a rename over the old one.
pub fn replace_symlink(link: &Path, target: &str) -> io::Result<()> {
    if let Some(dir) = link.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = tmp_name(link);
    let _ = fs::remove_file(&tmp);
    symlink(target, &tmp)?;
    fs::rename(&tmp, link)
}

/// Moves a complete staged release directory into the web root in one rename. A directory that is already
/// there is complete by construction, so the staged copy is dropped.
pub fn publish_dir(stage: &Path, dest: &Path) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if dest.exists() {
        return fs::remove_dir_all(stage);
    }
    rename_within_mount(stage, dest)
}

/// Renames `from` to `to` and never copies: a rename that crosses a mount point fails with EXDEV, and the error
/// names both paths. (The unit's ProtectSystem bind mounts split the web root and the stage into separate
/// mounts even on one disk, which st_dev cannot show.)
pub fn rename_within_mount(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to).map_err(|e| rename_error(from, to, e))
}

/// The message for a failed rename: EXDEV names both paths and what to change; other errors pass through.
pub fn rename_error(from: &Path, to: &Path, e: io::Error) -> io::Error {
    if e.raw_os_error() == Some(libc::EXDEV) {
        io::Error::new(
            e.kind(),
            format!(
                "cannot rename {} to {}: {e}; both folders must be inside one mount (the unit's ReadWritePaths are separate bind mounts)",
                from.display(),
                to.display()
            ),
        )
    } else {
        e
    }
}

/// Makes a stage folder, private to the mirror's user (0700). Its contents are never served (nginx denies dot paths).
pub fn make_stage(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

/// Proves that a stage folder can publish into its destination folder: a probe file is renamed into `dest` and
/// back. A cross-mount setup fails here, before any download, with both paths in the message.
pub fn probe_move(stage: &Path, dest: &Path) -> Result<(), String> {
    make_stage(stage).map_err(|e| format!("stage {}: {e}", stage.display()))?;
    fs::create_dir_all(dest).map_err(|e| format!("folder {}: {e}", dest.display()))?;
    let name = format!(".probe-{}", std::process::id());
    let here = stage.join(&name);
    let there = dest.join(&name);
    fs::write(&here, b"probe").map_err(|e| format!("stage {}: {e}", stage.display()))?;
    let moved = rename_within_mount(&here, &there).and_then(|()| rename_within_mount(&there, &here));
    let _ = fs::remove_file(&here);
    let _ = fs::remove_file(&there);
    moved.map_err(|e| format!("the download mirror cannot publish from {} to {}: {e}", stage.display(), dest.display()))
}

/// A hard link, or a copy when the link is not possible (another filesystem).
pub fn link_or_copy(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::hard_link(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => fs::copy(src, dst).map(|_| ()),
    }
}

/// Free bytes available to the mirror's user on the filesystem of `path`.
pub fn free_bytes(path: &Path) -> io::Result<u64> {
    let c = CString::new(path.as_os_str().as_encoded_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    // SAFETY: `statvfs` writes into the zeroed struct we pass; `c` is a valid NUL-terminated path.
    unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(s.f_bavail as u64 * s.f_frsize as u64)
    }
}

/// The bytes of the regular files under `dir` (symlinks not followed, a hard link counted each time).
pub fn used_bytes(dir: &Path) -> io::Result<u64> {
    let mut total = 0u64;
    if !dir.exists() {
        return Ok(0);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let meta = fs::symlink_metadata(entry.path())?;
        if meta.is_dir() {
            total += used_bytes(&entry.path())?;
        } else if meta.is_file() {
            total += meta.len();
        }
    }
    Ok(total)
}

/// Takes the lock file (`flock`, exclusive, no wait). `None` when another run holds it.
pub fn lock(path: &Path) -> io::Result<Option<File>> {
    use std::os::unix::io::AsRawFd;
    let f = File::options().create(true).append(true).open(path)?;
    // SAFETY: the descriptor belongs to `f`, which lives as long as the returned lock.
    let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        Ok(Some(f))
    } else {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::WouldBlock {
            Ok(None)
        } else {
            Err(err)
        }
    }
}

/// Removes the entries of `stage` older than `max_age` (interrupted runs), keeping the lock file.
pub fn remove_stale(stage: &Path, now: SystemTime, max_age: Duration) -> io::Result<()> {
    if !stage.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(stage)? {
        let entry = entry?;
        if entry.file_name() == ".lock" {
            continue;
        }
        let modified = entry.metadata()?.modified()?;
        if now.duration_since(modified).unwrap_or_default() > max_age {
            let p = entry.path();
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(p)?;
            } else {
                fs::remove_file(p)?;
            }
        }
    }
    Ok(())
}

/// Whether `path` names a folder that is a symlink or a plain entry; used by the prune to skip links.
pub fn is_dir_not_link(path: &Path) -> bool {
    fs::symlink_metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-mirror-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_staged_release_is_published_by_one_rename_inside_its_folder() {
        let root = temp("publish");
        make_stage(&root.join(".stage")).unwrap();
        let stage = root.join(".stage/razdor/v1.6.0");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("razdor-1.6.0.apk"), b"apk").unwrap();
        let dest = root.join("indicozy/razdor/releases/download/v1.6.0");
        publish_dir(&stage, &dest).unwrap();
        assert_eq!(fs::read(dest.join("razdor-1.6.0.apk")).unwrap(), b"apk");
        assert!(!stage.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_stage_folder_is_private_and_the_probe_leaves_nothing() {
        let root = temp("probe");
        let stage = root.join(".stage");
        let dest = root.join("indicozy/razdor/releases/download");
        probe_move(&stage, &dest).expect("one tree: the probe passes");
        let mode = fs::metadata(&stage).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        assert_eq!(fs::read_dir(&stage).unwrap().count(), 0, "the probe file is removed from the stage");
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0, "and from the folder");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_cross_mount_rename_names_both_paths() {
        let err = rename_error(Path::new("/a/stage/x"), Path::new("/b/hub/x"), io::Error::from_raw_os_error(libc::EXDEV));
        let text = err.to_string();
        assert!(text.contains("/a/stage/x") && text.contains("/b/hub/x"), "{text}");
        assert!(text.contains("one mount"), "{text}");
        let other = rename_error(Path::new("/a"), Path::new("/b"), io::Error::from_raw_os_error(libc::ENOENT));
        assert_eq!(other.raw_os_error(), Some(libc::ENOENT), "other errors pass through unchanged");
    }

    #[test]
    fn stale_stage_entries_are_removed_and_fresh_ones_kept() {
        let root = temp("stale");
        let stage = root.join("stage");
        fs::create_dir_all(stage.join("old")).unwrap();
        fs::write(stage.join("old/f.part"), b"x").unwrap();
        fs::write(stage.join("fresh.part"), b"y").unwrap();
        fs::write(stage.join(".lock"), b"").unwrap();
        let long_ago = SystemTime::now() - Duration::from_secs(3 * 86_400);
        fs::File::options().write(true).open(stage.join("old/f.part")).unwrap().set_modified(long_ago).unwrap();
        fs::File::open(stage.join("old")).unwrap().set_modified(long_ago).unwrap();
        fs::File::options().write(true).open(stage.join(".lock")).unwrap().set_modified(long_ago).unwrap();
        remove_stale(&stage, SystemTime::now(), Duration::from_secs(86_400)).unwrap();
        assert!(!stage.join("old").exists());
        assert!(stage.join("fresh.part").exists());
        assert!(stage.join(".lock").exists(), "the lock file is never removed");
        remove_stale(&root.join("missing"), SystemTime::now(), Duration::from_secs(1)).unwrap();
        let _ = fs::remove_dir_all(&root);
    }
}
