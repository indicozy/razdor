//! The files of a content package, in memory: a folder or a `.rzpack` (a zip of the folder).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use super::diag::{Diagnostic, Severity};

/// Package-relative path (`/` separators) to the file's bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Files {
    entries: BTreeMap<String, Vec<u8>>,
    /// Problems met while reading (unsafe archive paths, names that are not UTF-8). The
    /// entries concerned are left out; `Package::parse` reports them.
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

    /// Reads a folder recursively.
    pub fn from_dir(dir: &Path) -> Result<Files, String> {
        let mut files = Files::default();
        collect(dir, "", &mut files)?;
        Ok(files)
    }

    /// Reads a `.rzpack`: a zip whose entries are the package's files.
    pub fn from_zip(bytes: &[u8]) -> Result<Files, String> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("not a zip archive: {e}"))?;
        let mut files = Files::default();
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
            let mut data = Vec::new();
            entry.read_to_end(&mut data).map_err(|e| format!("cannot read {name}: {e}"))?;
            files.insert(&path, data);
        }
        Ok(files)
    }

    /// A folder or a `.rzpack` file.
    pub fn load(path: &Path) -> Result<Files, String> {
        if path.is_dir() {
            Files::from_dir(path)
        } else if path.is_file() {
            let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            Files::from_zip(&bytes)
        } else {
            Err(format!("no such package folder or file: {}", path.display()))
        }
    }
}

fn collect(dir: &Path, prefix: &str, files: &mut Files) -> Result<(), String> {
    let read = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
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
        if path.is_dir() {
            collect(&path, &format!("{rel}/"), files)?;
        } else {
            let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            files.insert(&rel, bytes);
        }
    }
    Ok(())
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
}
