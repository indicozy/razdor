//! Razdor Studio's model: the package folder as the studio works on it. No window and no UI
//! toolkit here; the screens are in `ui::studio`.
//!
//! The folder on disk is the truth (the studio's design, decision 6). A `Workspace` lists the
//! package's files and holds the text documents opened for editing. Edits stay in memory until a
//! save. The package the screens read is parsed from the folder with every open document's text
//! put over its file (`rebuild`), so checks and map compiles see unsaved edits. A document that
//! does not parse as TOML keeps the last good package (`stale`) and refuses visual edits. Visual
//! edits are format-preserving (`edit`), undo and redo are one stack per package (`history`), and
//! a file changed on disk is reconciled with the open text (`reconcile`).

use std::fs;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use toml_edit::DocumentMut;

use crate::pack::check;
use crate::pack::diag::{Diagnostic, Report, Severity};
use crate::pack::files::{refused_path, Files};
use crate::pack::load::{line_of, Package};

pub mod edit;
pub mod history;
pub mod map;
pub mod reconcile;

#[cfg(test)]
mod tests;

pub use edit::{Edit, Kind, Locator};
pub use history::Step;
pub use reconcile::{unified, Reconcile};

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
    /// While a document is stale, its parse error is first.
    pub problems: Vec<Diagnostic>,
    /// The package as last read: the disk with every open document's text over it. Kept while a
    /// document does not parse (see [`Stale`]).
    package: Option<Rc<Package>>,
    /// The open TOML documents that do not parse, as of the last [`Workspace::rebuild`].
    pub stale: Vec<Stale>,
    /// The undo and redo stacks of the package.
    pub history: history::History,
    /// The text burst being typed: its file and the file's text before the burst.
    burst: Option<(String, String)>,
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

/// An open TOML document that does not parse: the banner's line and message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stale {
    pub path: String,
    pub line: u32,
    pub message: String,
}

impl Stale {
    /// The banner text: `stale: maps/brod/map.toml line 12: expected …`.
    pub fn banner(&self) -> String {
        format!("stale: {} line {}: {}", self.path, self.line, self.message)
    }

    /// The same as a diagnostic, for the Problems list.
    pub fn diagnostic(&self) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            file: self.path.clone(),
            line: Some(self.line),
            message: self.message.clone(),
            hint: Some("the file does not parse: the screens show its last good state until it does".into()),
        }
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
        let mut ws = Workspace {
            root,
            entries: Vec::new(),
            docs: Vec::new(),
            problems: Vec::new(),
            package: None,
            stale: Vec::new(),
            history: history::History::default(),
            burst: None,
        };
        ws.refresh_entries()?;
        Ok(ws)
    }

    /// Lists the package's files again (after a file was added, removed or renamed outside).
    pub fn refresh_entries(&mut self) -> Result<(), String> {
        let mut entries = Vec::new();
        let canon = fs::canonicalize(&self.root).map_err(|e| format!("cannot read {}: {e}", self.root.display()))?;
        list(&canon, &canon, "", 0, &mut entries)?;
        entries.sort();
        self.entries = entries;
        Ok(())
    }

    /// Opens a text file of the package for editing and returns its index in `docs`. A file
    /// already open is not read again: its index comes back. A file whose real place (symlinks
    /// followed) is outside the package is refused.
    pub fn open_doc(&mut self, path: &str) -> Result<usize, String> {
        if let Some(i) = self.docs.iter().position(|d| d.path == path) {
            return Ok(i);
        }
        let text = self.read_text(path)?;
        self.docs.push(Doc { path: path.to_string(), saved: text.clone(), text });
        Ok(self.docs.len() - 1)
    }

    /// A text file of the package as it is on disk, with the same checks as [`Workspace::open_doc`].
    pub fn read_text(&self, path: &str) -> Result<String, String> {
        let rel = safe_rel(path)?;
        if !is_text(path) {
            return Err(format!("{path} is not a text file"));
        }
        let root = self.canon_root()?;
        let real = fs::canonicalize(self.root.join(&rel)).map_err(|e| format!("cannot read {path}: {e}"))?;
        if !real.starts_with(&root) || !real.is_file() {
            return Err(format!("{path} is not a file of the package"));
        }
        fs::read_to_string(&real).map_err(|e| format!("cannot read {path}: {e}"))
    }

    /// Writes an open document to its file; it is clean afterwards. A file that is a symbolic
    /// link is never written through, and nothing is written outside the package.
    pub fn save(&mut self, idx: usize) -> Result<(), String> {
        let doc = self.docs.get(idx).ok_or_else(|| format!("no open document {idx}"))?;
        let rel = safe_rel(&doc.path)?;
        let path = self.root.join(&rel);
        let not_saved = |why: &str| format!("cannot save {}: {why}", doc.path);
        let meta = fs::symlink_metadata(&path).map_err(|e| not_saved(&e.to_string()))?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(not_saved("not a regular file of the package"));
        }
        let root = self.canon_root()?;
        let parent = path.parent().ok_or_else(|| not_saved("no folder"))?;
        let parent = fs::canonicalize(parent).map_err(|e| not_saved(&e.to_string()))?;
        if !parent.starts_with(&root) {
            return Err(not_saved("outside the package folder"));
        }
        fs::write(&path, &doc.text).map_err(|e| not_saved(&e.to_string()))?;
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

    /// The package as it is on disk now (what a save wrote): read and parsed, not checked.
    pub fn read_package(&self) -> Result<(Package, Report), String> {
        let files = Files::load(&self.root)?;
        Ok(Package::parse(files))
    }

    /// The last good package: the disk with every open document's text over it.
    pub fn package(&self) -> Option<Rc<Package>> {
        self.package.clone()
    }

    /// Re-reads the package from the disk with the open documents' text over their files, and
    /// runs the checks. A document that does not parse as TOML keeps the last good package (and
    /// is listed in [`Workspace::stale`]); the other files are read as usual. An error when the
    /// folder cannot be read at all.
    pub fn rebuild(&mut self) -> Result<(), String> {
        let mut files = Files::load(&self.root)?;
        for doc in &self.docs {
            files.overlay(&doc.path, doc.text.as_bytes())?;
        }
        self.stale = stale_docs(&self.docs);
        if !self.stale.is_empty() && self.package.is_some() {
            let mut problems: Vec<Diagnostic> = self.stale.iter().map(Stale::diagnostic).collect();
            problems.extend(self.problems.iter().filter(|d| !self.stale.iter().any(|s| s.path == d.file)).cloned());
            self.problems = problems;
            return Ok(());
        }
        let (package, _) = Package::parse(files);
        let report = check::check(&package);
        self.problems = report.diagnostics;
        self.package = Some(Rc::new(package));
        Ok(())
    }

    /// [`Workspace::rebuild`], and the counts of the problems (errors, warnings).
    pub fn check(&mut self) -> Result<(usize, usize), String> {
        self.rebuild()?;
        let errors = self.problems.iter().filter(|d| d.severity == Severity::Error).count();
        Ok((errors, self.problems.len() - errors))
    }

    /// Starts a text burst of `path` (typing in the Script tab): `before` is the text before the
    /// first change of the burst. A burst already open for the same file goes on; one for another
    /// file is ended first.
    pub fn begin_text_edit(&mut self, path: &str, before: &str) {
        match &self.burst {
            Some((open, _)) if open == path => return,
            Some(_) => self.end_text_edit(),
            None => {}
        }
        self.burst = Some((path.to_string(), before.to_string()));
    }

    /// Ends the text burst: one undo step from its `before` to the file's text now.
    pub fn end_text_edit(&mut self) {
        let Some((path, before)) = self.burst.take() else { return };
        let Some(doc) = self.docs.iter().find(|d| d.path == path) else { return };
        let after = doc.text.clone();
        let label = format!("edit {path}");
        self.history.record(Step { path, before, after, label });
    }

    /// The file of the open text burst, if any.
    fn burst_path(&self) -> Option<&str> {
        self.burst.as_ref().map(|(p, _)| p.as_str())
    }

    /// Makes one visual edit: the table's text changes in the document of its file (opened if it
    /// is not), as one undo step. Refused with the reason when the file does not parse (the
    /// banner's text), when the table is not there, or when the edit names no key of the format.
    pub fn apply(&mut self, edit: Edit) -> Result<(), String> {
        self.end_text_edit();
        let file = edit.locator().file.clone();
        let idx = self.open_doc(&file)?;
        let before = self.docs[idx].text.clone();
        if let Some((line, message)) = edit::parse_error(&before) {
            return Err(format!("stale: {file} line {line}: {message}"));
        }
        let after = edit::apply_to_text(&before, &edit)?;
        let label = edit.label();
        self.docs[idx].text = after.clone();
        self.history.record(Step { path: file, before, after, label });
        Ok(())
    }

    /// Undoes the last step; returns its label, or `None` when there is nothing to undo.
    pub fn undo(&mut self) -> Result<Option<String>, String> {
        self.end_text_edit();
        let Some(step) = self.history.pop_undo() else { return Ok(None) };
        if let Err(e) = self.set_text(&step.path, &step.before) {
            self.history.push_undo(step);
            return Err(e);
        }
        let label = step.label.clone();
        self.history.push_redo(step);
        Ok(Some(label))
    }

    /// Redoes the last undone step; returns its label, or `None` when there is nothing to redo.
    pub fn redo(&mut self) -> Result<Option<String>, String> {
        self.end_text_edit();
        let Some(step) = self.history.pop_redo() else { return Ok(None) };
        if let Err(e) = self.set_text(&step.path, &step.after) {
            self.history.push_redo(step);
            return Err(e);
        }
        let label = step.label.clone();
        self.history.push_undo(step);
        Ok(Some(label))
    }

    fn set_text(&mut self, path: &str, text: &str) -> Result<(), String> {
        let i = self.open_doc(path)?;
        self.docs[i].text = text.to_string();
        Ok(())
    }

    /// The text of a package file: the open document's, else the file's on disk.
    pub fn text_of(&self, path: &str) -> Result<String, String> {
        match self.docs.iter().find(|d| d.path == path) {
            Some(doc) => Ok(doc.text.clone()),
            None => self.read_text(path),
        }
    }

    /// The byte ranges of the map tables of a TOML file (see `studio::edit::spans`).
    pub fn spans(&self, path: &str) -> Result<Vec<(Locator, Range<usize>)>, String> {
        edit::spans(path, &self.text_of(path)?)
    }

    /// The map table that holds byte `offset` of the file, if any.
    pub fn locator_at(&self, path: &str, offset: usize) -> Option<Locator> {
        self.spans(path).ok()?.into_iter().find(|(_, r)| r.contains(&offset)).map(|(l, _)| l)
    }

    /// The package folder with its symbolic links resolved: what every path is checked against.
    fn canon_root(&self) -> Result<PathBuf, String> {
        fs::canonicalize(&self.root).map_err(|e| format!("cannot read {}: {e}", self.root.display()))
    }
}

/// The open TOML documents whose text does not parse, with the parser's line and message.
fn stale_docs(docs: &[Doc]) -> Vec<Stale> {
    docs.iter()
        .filter(|d| d.path.ends_with(".toml"))
        .filter_map(|d| {
            let err = d.text.parse::<DocumentMut>().err()?;
            let line = err.span().map_or(1, |s| line_of(&d.text, s.start));
            Some(Stale { path: d.path.clone(), line, message: err.message().to_string() })
        })
        .collect()
}

/// Whether `path` is a text file the studio opens (by its extension, any case).
pub fn is_text(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && TEXT_EXTENSIONS.iter().any(|t| t.eq_ignore_ascii_case(ext)))
}

/// A package-relative path as a filesystem path: no absolute paths, no `..`, no backslashes, no
/// network share or device path.
fn safe_rel(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() || path.contains('\\') || refused_path(path).is_some() {
        return Err(format!("not a package path: {path:?}"));
    }
    let p = Path::new(path);
    if p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!("not a package path: {path:?}"));
    }
    Ok(p.to_path_buf())
}

/// Lists the files under `dir` (package-relative `rel`), skipping dotfiles and `.editor/`. Folder
/// symbolic links are skipped; a file symbolic link is listed only when its real place is a
/// regular file inside `root` (the canonical package folder).
fn list(root: &Path, dir: &Path, rel: &str, depth: usize, out: &mut Vec<String>) -> Result<(), String> {
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
        let at = entry.path();
        let Ok(meta) = fs::symlink_metadata(&at) else { continue };
        if meta.file_type().is_symlink() {
            let inside = fs::canonicalize(&at).is_ok_and(|real| real.starts_with(root) && real.is_file());
            if inside {
                out.push(path);
            }
        } else if meta.is_dir() {
            list(root, &at, &format!("{path}/"), depth + 1, out)?;
        } else if meta.is_file() {
            out.push(path);
        }
    }
    Ok(())
}
