//! Reads and writes under the package's `.editor/` folder (the studio's own data: layout, stamps),
//! refusing anything that would leave the package. A downloaded package may carry `.editor` as a
//! link, or a folder or file under it: every component is checked with `symlink_metadata`, the
//! canonical parent must stay under the canonical `.editor`, and a write goes to a temporary file
//! that is renamed into place (on unix the temporary file is opened with `O_NOFOLLOW`).

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// The package's `.editor` folder name.
const EDITOR: &str = ".editor";

/// Checks that `dir` is a real folder (not a link, not a file). With `create`, a missing folder is
/// made (only as this folder, its parent being checked by the caller).
fn real_dir(dir: &Path, create: bool) -> Result<bool, String> {
    match fs::symlink_metadata(dir) {
        Ok(m) if m.file_type().is_symlink() => Err(format!("{} is a link: nothing is written or read through it", dir.display())),
        Ok(m) if m.is_dir() => Ok(true),
        Ok(_) => Err(format!("{} is not a folder", dir.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !create {
                return Ok(false);
            }
            fs::create_dir(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
            Ok(true)
        }
        Err(e) => Err(format!("cannot read {}: {e}", dir.display())),
    }
}

/// The package-relative path under `.editor/` (`stamps/x.toml`) as its parts: no empty, `.` or
/// `..` part, no backslash, no drive or device name.
fn parts_of(rel: &str) -> Result<Vec<&str>, String> {
    let parts: Vec<&str> = rel.split('/').collect();
    let ok = !rel.is_empty()
        && !rel.contains('\\')
        && !rel.contains(':')
        && parts.iter().all(|p| !p.is_empty() && *p != "." && *p != "..")
        && Path::new(rel).components().all(|c| matches!(c, Component::Normal(_)));
    if ok {
        Ok(parts)
    } else {
        Err(format!("{rel:?} is not a path under .editor/"))
    }
}

/// The canonical package folder and the canonical `.editor` folder it must hold.
fn canonical(root: &Path) -> Result<(PathBuf, PathBuf), String> {
    let root = fs::canonicalize(root).map_err(|e| format!("cannot read {}: {e}", root.display()))?;
    let editor = root.join(EDITOR);
    Ok((root, editor))
}

/// Writes `bytes` to `.editor/<rel>` of the package at `root`, making the folders it needs. Refused
/// when a folder on the way is a link or a file, when the place resolves outside `.editor`, or when
/// the target is a link or not a regular file.
pub fn write(root: &Path, rel: &str, bytes: &[u8]) -> Result<(), String> {
    let parts = parts_of(rel)?;
    let (canon_root, canon_editor) = canonical(root)?;
    let (name, dirs) = parts.split_last().ok_or("no file name")?;
    let mut dir = root.join(EDITOR);
    real_dir(&dir, true)?;
    for part in dirs {
        dir = dir.join(part);
        real_dir(&dir, true)?;
    }
    // The canonical place must be inside `.editor` of the canonical package.
    let real = fs::canonicalize(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    if !real.starts_with(&canon_editor) {
        return Err(format!("{} is outside the package's .editor folder", dir.display()));
    }
    let _ = &canon_root;
    let target = dir.join(name);
    match fs::symlink_metadata(&target) {
        Ok(m) if m.file_type().is_symlink() || !m.is_file() => {
            return Err(format!("{} is not a regular file: nothing is written over it", target.display()));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot read {}: {e}", target.display())),
    }
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&tmp).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(format!("cannot write {}: {e}", target.display()));
    }
    fs::rename(&tmp, &target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("cannot write {}: {e}", target.display())
    })
}

/// Reads `.editor/<rel>` of the package at `root`. `None` when it is not there, or when any folder
/// on the way or the file itself is a link, is not a regular file, or resolves outside `.editor`.
pub fn read(root: &Path, rel: &str) -> Option<Vec<u8>> {
    let parts = parts_of(rel).ok()?;
    let (_, canon_editor) = canonical(root).ok()?;
    let mut path = root.join(EDITOR);
    if !real_dir(&path, false).ok()? {
        return None;
    }
    let (name, dirs) = parts.split_last()?;
    for part in dirs {
        path = path.join(part);
        if !real_dir(&path, false).ok()? {
            return None;
        }
    }
    let file = path.join(name);
    let meta = fs::symlink_metadata(&file).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return None;
    }
    let real = fs::canonicalize(&file).ok()?;
    if !real.starts_with(&canon_editor) {
        return None;
    }
    fs::read(&file).ok()
}

/// Whether the package's `.editor/<dir>` folder is a real folder (no link on the way), so that a
/// listing of it reads only the package's own files.
pub fn real_folder(root: &Path, rel_dir: &str) -> bool {
    let Ok(parts) = parts_of(rel_dir) else { return false };
    let mut path = root.join(EDITOR);
    if !matches!(real_dir(&path, false), Ok(true)) {
        return false;
    }
    for part in parts {
        path = path.join(part);
        if !matches!(real_dir(&path, false), Ok(true)) {
            return false;
        }
    }
    true
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A fresh folder for a test, removed by the test's own cleanup.
    fn scratch(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-guarded-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_write_and_a_read_inside_the_package_work() {
        let root = scratch("plain");
        write(&root, "layout.json", b"{}\n").unwrap();
        write(&root, "stamps/a.toml", b"x = 1\n").unwrap();
        assert_eq!(read(&root, "layout.json").as_deref(), Some(&b"{}\n"[..]));
        assert_eq!(read(&root, "stamps/a.toml").as_deref(), Some(&b"x = 1\n"[..]));
        assert!(!root.join(".editor").join(".a.toml.tmp").exists(), "the temporary file is renamed away");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_editor_folder_that_is_a_link_outside_is_refused_for_write_and_read() {
        let root = scratch("link");
        let outside = scratch("outside");
        std::os::unix::fs::symlink(&outside, root.join(".editor")).unwrap();
        assert!(write(&root, "layout.json", b"{}").is_err(), "a write through the link is refused");
        assert!(!outside.join("layout.json").exists(), "nothing was written outside the package");
        fs::write(outside.join("layout.json"), b"secret").unwrap();
        assert!(read(&root, "layout.json").is_none(), "a read through the link is refused");
        assert!(!real_folder(&root, "stamps"));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_symlinked_file_under_stamps_is_refused() {
        let root = scratch("stamp");
        let outside = scratch("stamp-outside");
        fs::create_dir_all(root.join(".editor").join("stamps")).unwrap();
        fs::write(outside.join("victim.toml"), b"keep").unwrap();
        std::os::unix::fs::symlink(outside.join("victim.toml"), root.join(".editor/stamps/x.toml")).unwrap();
        assert!(write(&root, "stamps/x.toml", b"owned").is_err(), "a symlinked target is not written over");
        assert_eq!(fs::read(outside.join("victim.toml")).unwrap(), b"keep");
        assert!(read(&root, "stamps/x.toml").is_none(), "a symlinked file is not read");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_folder_link_under_editor_is_refused() {
        let root = scratch("subdir");
        let outside = scratch("subdir-outside");
        fs::create_dir_all(root.join(".editor")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join(".editor/stamps")).unwrap();
        assert!(write(&root, "stamps/y.toml", b"owned").is_err());
        assert!(!outside.join("y.toml").exists());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_path_that_leaves_editor_is_refused() {
        let root = scratch("path");
        assert!(write(&root, "../x", b"x").is_err());
        assert!(write(&root, "/etc/x", b"x").is_err());
        assert!(read(&root, "a/../../x").is_none());
        let _ = fs::remove_dir_all(&root);
    }
}
