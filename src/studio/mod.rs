//! Razdor Studio's model: the package folder as the studio works on it. No window and no UI
//! toolkit here; the screens are in `ui::studio`.
//!
//! The folder on disk is the truth (the studio's design, decision 6). A `Workspace` lists the
//! package's files, holds the text documents opened for editing and the last check's
//! diagnostics. Edits stay in memory until a save; `check` re-reads the folder from disk, so it
//! sees what was saved and what other tools changed.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::pack::check;
use crate::pack::diag::Diagnostic;
use crate::pack::files::Files;
use crate::pack::load::Package;

#[cfg(test)]
mod tests;

/// The extensions the studio opens as text.
const TEXT_EXTENSIONS: [&str; 4] = ["toml", "lua", "md", "txt"];

/// A package folder opened in the studio.
#[derive(Debug)]
pub struct Workspace {
    pub root: PathBuf,
    /// Package-relative paths of the files, with `/` separators, sorted. Directories are not
    /// listed; `.editor/` and dotfiles are skipped.
    pub entries: Vec<String>,
    /// The text documents open for editing, in the order they were opened.
    pub docs: Vec<Doc>,
    /// The last check's diagnostics: the parse problems first, then the rules' (`pack::check`).
    pub problems: Vec<Diagnostic>,
}

/// One text file open for editing: its text in memory and the text last saved (or read).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doc {
    pub path: String,
    pub text: String,
    saved: String,
}

impl Doc {
    /// Whether the text differs from what is on disk (as far as the studio knows it).
    pub fn is_dirty(&self) -> bool {
        self.text != self.saved
    }
}

impl Workspace {
    /// Opens a package folder: lists its files. An error when the folder is missing or has no
    /// `package.toml`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Workspace, String> {
        let root: PathBuf = root.into();
        if !root.is_dir() {
            return Err(format!("{} is not a folder", root.display()));
        }
        if !root.join("package.toml").is_file() {
            return Err(format!("{} has no package.toml: not a package folder", root.display()));
        }
        let mut entries = Vec::new();
        list(&root, "", 0, &mut entries)?;
        entries.sort();
        Ok(Workspace { root, entries, docs: Vec::new(), problems: Vec::new() })
    }

    /// Opens a text file of the package for editing and returns its index in `docs`. A file
    /// already open is not read again: its index comes back.
    pub fn open_doc(&mut self, path: &str) -> Result<usize, String> {
        if let Some(i) = self.docs.iter().position(|d| d.path == path) {
            return Ok(i);
        }
        let rel = safe_rel(path)?;
        if !is_text(path) {
            return Err(format!("{path} is not a text file"));
        }
        let text = fs::read_to_string(self.root.join(&rel)).map_err(|e| format!("cannot read {path}: {e}"))?;
        self.docs.push(Doc { path: path.to_string(), saved: text.clone(), text });
        Ok(self.docs.len() - 1)
    }

    /// Writes an open document to its file; it is clean afterwards.
    pub fn save(&mut self, idx: usize) -> Result<(), String> {
        let doc = self.docs.get(idx).ok_or_else(|| format!("no open document {idx}"))?;
        let rel = safe_rel(&doc.path)?;
        fs::write(self.root.join(rel), &doc.text).map_err(|e| format!("cannot save {}: {e}", doc.path))?;
        let doc = &mut self.docs[idx];
        doc.saved = doc.text.clone();
        Ok(())
    }

    /// Saves every dirty document. The ones that fail are named in the error; the others are
    /// saved anyway.
    pub fn save_all(&mut self) -> Result<(), String> {
        let mut errors = Vec::new();
        for i in 0..self.docs.len() {
            if self.docs[i].is_dirty() {
                if let Err(e) = self.save(i) {
                    errors.push(e);
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    pub fn any_dirty(&self) -> bool {
        self.docs.iter().any(Doc::is_dirty)
    }

    /// Re-reads the package from disk and runs the checks, as `razdor-pack check` does. Stores
    /// the diagnostics and returns the counts (errors, warnings). An error when the folder
    /// cannot be read at all.
    pub fn check(&mut self) -> Result<(usize, usize), String> {
        let files = Files::load(&self.root)?;
        let (package, _) = Package::parse(files);
        let report = check::check(&package);
        let counts = (report.errors(), report.warnings());
        self.problems = report.diagnostics;
        Ok(counts)
    }
}

/// Whether `path` is a text file the studio opens (by its extension, any case).
pub fn is_text(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && TEXT_EXTENSIONS.iter().any(|t| t.eq_ignore_ascii_case(ext)))
}

/// A package-relative path as a filesystem path: no absolute paths, no `..`, no backslashes.
fn safe_rel(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() || path.contains('\\') {
        return Err(format!("not a package path: {path:?}"));
    }
    let p = Path::new(path);
    if p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!("not a package path: {path:?}"));
    }
    Ok(p.to_path_buf())
}

/// Lists the files under `dir` (package-relative `rel`), skipping dotfiles and `.editor/`.
fn list(dir: &Path, rel: &str, depth: usize, out: &mut Vec<String>) -> Result<(), String> {
    if depth > 32 {
        return Ok(());
    }
    let rd = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = format!("{rel}{name}");
        let Ok(meta) = fs::metadata(entry.path()) else { continue };
        if meta.is_dir() {
            list(&entry.path(), &format!("{path}/"), depth + 1, out)?;
        } else if meta.is_file() {
            out.push(path);
        }
    }
    Ok(())
}
