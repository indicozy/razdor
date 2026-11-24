//! The web root and the staging folder: atomic writes and renames, hard links, disk figures, the lock.

use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::{symlink, MetadataExt};
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
    fs::rename(stage, dest)
}

/// A hard link, or a copy when the link is not possible (another filesystem).
pub fn link_or_copy(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::hard_link(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => fs::copy(src, dst).map(|_| ()),
    }
}

/// The filesystem's device number of `path` (the folder must exist).
pub fn device(path: &Path) -> io::Result<u64> {
    Ok(fs::metadata(path)?.dev())
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
