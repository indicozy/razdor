//! Razdor Studio's model: the package folder as the studio works on it. No window and no UI
//! toolkit here; the screens are in `ui::studio`.
//!
//! The folder on disk is the truth (the studio's design, decision 6). A `Workspace` lists the
//! package's files and holds the documents opened for editing: the text files (`docs`) and the
//! maps' terrains (`terrains`, painted in memory). Edits stay in memory until a save. The package
//! the screens read is built from the disk with every open document over it (`build`, run on a
//! worker thread by the window: `worker`), so checks and map compiles see unsaved edits. A document
//! that does not parse as TOML keeps the last good package (`stale`) and refuses visual edits.
//! Visual edits are format-preserving (`edit`), undo and redo are one stack per package (`history`),
//! a file changed on disk is reconciled with the open text (`reconcile`), and tables are added and
//! removed (`place`).

use std::fs;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use toml_edit::DocumentMut;

use crate::dt::install::DtInstall;
use crate::pack::compile::Compiled;
use crate::pack::diag::{Diagnostic, Report, Severity};
use crate::pack::files::{refused_path, Files};
use crate::pack::load::{line_of, Package};
use crate::pack::reach::Site;
use crate::pack::terrain::Palette;

pub mod brush;
pub mod campaign_graph;
pub mod characters;
pub mod cost;
pub mod build;
pub mod claude;
pub mod doc;
pub mod edit;
pub mod history;
pub mod layers;
pub mod map;
pub mod nav;
pub mod palette;
pub mod play;
pub mod sheet;
pub mod mcp;
pub mod paint;
pub mod place;
pub mod reconcile;
pub mod records;
pub mod gen;
pub mod geometry;
pub mod guarded;
pub mod health;
pub mod newpkg;
pub mod overlay;
pub mod region;
pub mod select;
pub mod richness;
pub mod scenario;
pub mod scores;
pub mod story;
pub mod story_graph;
pub mod texts;
pub mod vars;
pub mod worker;
/// The Claude dock's pseudo-terminal (desktop, with the studio's feature).
#[cfg(all(feature = "studio", not(any(target_os = "android", target_os = "ios"))))]
pub mod terminal;
pub mod testers;

#[cfg(test)]
mod tests;

pub use edit::{Edit, Kind, Locator};
pub use history::{Change, Step};
pub use paint::TerrainDoc;
pub use place::Place;
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
    /// The maps' terrains open for painting.
    pub terrains: Vec<TerrainDoc>,
    /// The last check's diagnostics: the parse problems first, then the rules' (`pack::check`).
    /// While a document is stale, its parse error is first.
    pub problems: Vec<Diagnostic>,
    /// The package as last read: the disk with every open document's text over it. Kept while a
    /// document does not parse (see [`Stale`]).
    package: Option<Arc<Package>>,
    /// The open TOML documents that do not parse, as of the last install.
    pub stale: Vec<Stale>,
    /// The undo and redo stacks of the package.
    pub history: history::History,
    /// The text burst being typed: its file and the file's text before the burst.
    burst: Option<(String, String)>,
    /// The paint stroke in progress: its terrain's path and the cells it touched, each with the
    /// code it had before the stroke.
    stroke: Option<Stroke>,
    /// The changes of the operation in progress (`begin_group`): they become one undo step.
    group: Option<Vec<history::Change>>,
}

/// A paint stroke: the cells it touched and their codes before the stroke.
#[derive(Debug)]
struct Stroke {
    path: String,
    touched: std::collections::BTreeMap<(u32, u32), u8>,
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

/// What an installed rebuild gave the screens: the map it is of, its compile and its unreached
/// story cells. Empty when the rebuild was not taken (a stale document).
#[derive(Default)]
pub struct Installed {
    pub map: Option<String>,
    pub compiled: Option<Result<Compiled, Report>>,
    pub unreached: Option<Vec<Site>>,
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
            terrains: Vec::new(),
            problems: Vec::new(),
            package: None,
            stale: Vec::new(),
            history: history::History::default(),
            burst: None,
            stroke: None,
            group: None,
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
        if !is_text(path) {
            return Err(format!("{path} is not a text file"));
        }
        let bytes = self.read_bytes(path)?;
        String::from_utf8(bytes).map_err(|e| format!("cannot read {path}: {e}"))
    }

    /// A file of the package as bytes, as it is on disk: the same checks as a text file's (a file of
    /// the package, its real place inside the folder).
    pub fn read_bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        let rel = safe_rel(path)?;
        let root = self.canon_root()?;
        let real = fs::canonicalize(self.root.join(&rel)).map_err(|e| format!("cannot read {path}: {e}"))?;
        if !real.starts_with(&root) || !real.is_file() {
            return Err(format!("{path} is not a file of the package"));
        }
        fs::read(&real).map_err(|e| format!("cannot read {path}: {e}"))
    }

    /// Writes an open document to its file; it is clean afterwards.
    pub fn save(&mut self, idx: usize) -> Result<(), String> {
        let doc = self.docs.get(idx).ok_or_else(|| format!("no open document {idx}"))?;
        let (path, text) = (doc.path.clone(), doc.text.clone());
        self.write_package_file(&path, text.as_bytes())?;
        let doc = &mut self.docs[idx];
        doc.saved = doc.text.clone();
        Ok(())
    }

    /// Writes a file of the package. A file that is a symbolic link is never written through, and
    /// nothing is written outside the package.
    fn write_package_file(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let rel = safe_rel(path)?;
        let full = self.root.join(&rel);
        let not_saved = |why: &str| format!("cannot save {path}: {why}");
        // A file that is not there yet (a new campaign, `create_text_file`) is made, in a folder of
        // the package; an existing one must be a regular file, not a link.
        match fs::symlink_metadata(&full) {
            Ok(meta) => {
                if meta.file_type().is_symlink() || !meta.is_file() {
                    return Err(not_saved("not a regular file of the package"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let parent = full.parent().ok_or_else(|| not_saved("no folder"))?;
                fs::create_dir_all(parent).map_err(|e| not_saved(&e.to_string()))?;
            }
            Err(e) => return Err(not_saved(&e.to_string())),
        }
        let root = self.canon_root()?;
        let parent = full.parent().ok_or_else(|| not_saved("no folder"))?;
        let parent = fs::canonicalize(parent).map_err(|e| not_saved(&e.to_string()))?;
        if !parent.starts_with(&root) {
            return Err(not_saved("outside the package folder"));
        }
        fs::write(&full, bytes).map_err(|e| not_saved(&e.to_string()))
    }

    /// Saves every dirty document (texts and terrains). The ones that fail are named in the error;
    /// the others are saved anyway.
    pub fn save_all(&mut self) -> Result<(), String> {
        let mut errors = Vec::new();
        for i in 0..self.docs.len() {
            if self.docs[i].is_dirty() {
                if let Err(e) = self.save(i) {
                    errors.push(e);
                }
            }
        }
        for i in 0..self.terrains.len() {
            if let Err(e) = self.save_terrain(i) {
                errors.push(e);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Writes a painted terrain to its picture; a clean one is left alone.
    pub fn save_terrain(&mut self, idx: usize) -> Result<(), String> {
        let t = self.terrains.get(idx).ok_or_else(|| format!("no open terrain {idx}"))?;
        if !t.is_dirty() {
            return Ok(());
        }
        let (path, bytes) = (t.path.clone(), t.bytes());
        self.write_package_file(&path, &bytes)?;
        self.terrains[idx].mark_saved(bytes);
        Ok(())
    }

    pub fn any_dirty(&self) -> bool {
        self.docs.iter().any(Doc::is_dirty) || self.terrains.iter().any(TerrainDoc::is_dirty)
    }

    /// The package as it is on disk now (what a save wrote): read and parsed, not checked.
    pub fn read_package(&self) -> Result<(Package, Report), String> {
        let files = Files::load(&self.root)?;
        Ok(Package::parse(files))
    }

    /// The last good package: the disk with every open document's text over it.
    pub fn package(&self) -> Option<Arc<Package>> {
        self.package.clone()
    }

    /// The rebuild's input: the open texts and the painted terrains, over the disk.
    pub fn job(&self, map: Option<String>, lang: &str, install: Option<Arc<DtInstall>>) -> build::Job {
        let mut overlays: Vec<(String, Vec<u8>)> = self.docs.iter().map(|d| (d.path.clone(), d.text.clone().into_bytes())).collect();
        overlays.extend(self.terrains.iter().filter(|t| t.is_dirty()).map(|t| (t.path.clone(), t.bytes())));
        build::Job { root: self.root.clone(), overlays, map, lang: lang.to_string(), install }
    }

    /// Takes a finished rebuild. A document that does not parse keeps the last good package (and is
    /// listed in [`Workspace::stale`]): then the rebuild's own results are not taken either. The
    /// checks' problems replace the last ones.
    pub fn install(&mut self, built: build::Built) -> Installed {
        self.stale = stale_docs(&self.docs);
        if !self.stale.is_empty() && self.package.is_some() {
            let mut problems: Vec<Diagnostic> = self.stale.iter().map(Stale::diagnostic).collect();
            problems.extend(self.problems.iter().filter(|d| !self.stale.iter().any(|s| s.path == d.file)).cloned());
            self.problems = problems;
            return Installed::default();
        }
        self.problems = built.report.diagnostics;
        self.package = Some(Arc::new(built.package));
        Installed { map: built.map, compiled: built.compiled, unreached: built.unreached }
    }

    /// Re-reads the package from the disk with the open documents' text over their files, runs the
    /// checks, and takes the result here (without a worker: the tests and the command line use it).
    pub fn rebuild(&mut self) -> Result<(), String> {
        let built = build::run(self.job(None, "en", None))?;
        self.install(built);
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
        self.record(Step::text(path, before, after, label));
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
        self.record(Step::text(file, before, after, label));
        Ok(())
    }

    /// Applies table edits to one text file (a package file, opened if it is not) as one undo step
    /// named `label`. Refused when the file does not parse (the banner's text) or an edit does not
    /// fit the file; then nothing changes.
    pub fn apply_doc(&mut self, file: &str, label: &str, edits: &[doc::DocEdit]) -> Result<(), String> {
        self.apply_docs(label, vec![(file.to_string(), edits.to_vec())])
    }

    /// Applies table edits to several text files as one undo step (a rename across files). Every
    /// file is computed before any changes, so a failure in one leaves them all as they were.
    pub fn apply_docs(&mut self, label: &str, files: Vec<(String, Vec<doc::DocEdit>)>) -> Result<(), String> {
        self.end_text_edit();
        let mut changes = Vec::new();
        for (file, edits) in files {
            let idx = self.open_doc(&file)?;
            let before = self.docs[idx].text.clone();
            if let Some((line, message)) = edit::parse_error(&before) {
                return Err(format!("stale: {file} line {line}: {message}"));
            }
            let after = doc::apply_all(&before, &edits)?;
            changes.push((file, after));
        }
        self.set_texts(label, changes)
    }

    /// Puts new texts into open text files as one undo step named `label` (the texts are already
    /// computed; each must parse). A file whose text is the same is left out.
    pub fn set_texts(&mut self, label: &str, files: Vec<(String, String)>) -> Result<(), String> {
        self.end_text_edit();
        let mut members = Vec::new();
        for (file, after) in files {
            let idx = self.open_doc(&file)?;
            let before = self.docs[idx].text.clone();
            if before == after {
                continue;
            }
            self.docs[idx].text = after.clone();
            members.push(Change::Text { path: file, before, after });
        }
        match members.len() {
            0 => {}
            1 => {
                let Some(Change::Text { path, before, after }) = members.pop() else { unreachable!("one text change") };
                self.record(Step::text(path, before, after, label));
            }
            _ => self.record(Step { label: label.to_string(), change: Change::Group(members) }),
        }
        Ok(())
    }

    /// Creates a text file of the package (its text is `text`; not on disk until saved). Refused
    /// when the file exists or the path is not a package path. One undo step: undo empties it.
    pub fn create_text_file(&mut self, file: &str, text: &str, label: &str) -> Result<(), String> {
        if self.root.join(file).exists() || self.docs.iter().any(|d| d.path == file) {
            return Err(format!("{file} already exists"));
        }
        safe_rel(file)?;
        self.end_text_edit();
        self.docs.push(Doc { path: file.to_string(), text: text.to_string(), saved: String::new() });
        self.record(Step::text(file, "", text, label));
        Ok(())
    }

    /// Writes a file under `.editor/` of the package (the studio's own data, never a package file:
    /// the reader skips the folder). The folders are made when missing. Refused through a link (see
    /// [`guarded`]). Not an undo step.
    pub fn write_editor_file(&self, rel: &str, bytes: &[u8]) -> Result<(), String> {
        guarded::write(&self.root, rel, bytes)
    }

    /// Reads a file under `.editor/` (`None` when it is not there, or a link is on the way).
    pub fn read_editor_file(&self, rel: &str) -> Option<Vec<u8>> {
        guarded::read(&self.root, rel)
    }

    /// Opens a map's terrain for painting: its picture is read and decoded with the map's palette.
    /// Returns its index in `terrains`; an open one is not read again.
    pub fn open_terrain(&mut self, map: &str) -> Result<usize, String> {
        let path = format!("maps/{map}/terrain.png");
        if let Some(i) = self.terrains.iter().position(|t| t.path == path) {
            return Ok(i);
        }
        let palette = self.palette_of(map)?;
        let bytes = self.read_bytes(&path)?;
        self.terrains.push(TerrainDoc::open(map, bytes, palette)?);
        Ok(self.terrains.len() - 1)
    }

    /// The terrain palette of a map: its `[terrain_palette]` over the default colours.
    fn palette_of(&self, map: &str) -> Result<Palette, String> {
        let text = self.text_of(&format!("maps/{map}/map.toml"))?;
        let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
        let overrides: std::collections::BTreeMap<String, String> = table
            .get("terrain_palette")
            .and_then(toml::Value::as_table)
            .map(|t| t.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
            .unwrap_or_default();
        Palette::with_overrides(&overrides)
    }

    /// Paints `cells` of map `map` with terrain `code`, as part of the stroke in progress (a stroke
    /// starts with the first call after [`Workspace::paint_end`]). Cells off the map are skipped.
    pub fn paint(&mut self, map: &str, cells: &[(i64, i64)], code: u8) -> Result<(), String> {
        self.paint_codes(map, cells.iter().map(|&(x, y)| (x, y, code)))
    }

    /// [`Workspace::paint`] with a code per cell (a paste, a burn): the same stroke.
    pub fn paint_codes(&mut self, map: &str, cells: impl IntoIterator<Item = (i64, i64, u8)>, ) -> Result<(), String> {
        let path = format!("maps/{map}/terrain.png");
        let i = self.open_terrain(map)?;
        if self.stroke.as_ref().is_some_and(|s| s.path != path) {
            self.paint_end();
        }
        let stroke = self.stroke.get_or_insert_with(|| Stroke { path, touched: Default::default() });
        let doc = &mut self.terrains[i];
        for (x, y, code) in cells {
            if x < 0 || y < 0 || x >= i64::from(doc.terrain.width) || y >= i64::from(doc.terrain.height) {
                continue;
            }
            let (x, y) = (x as u32, y as u32);
            let before = doc.set(x, y, code);
            stroke.touched.entry((x, y)).or_insert(before);
        }
        Ok(())
    }

    /// Ends the paint stroke: one undo step with the rectangle of cells it changed, before and after.
    pub fn paint_end(&mut self) {
        let Some(stroke) = self.stroke.take() else { return };
        if stroke.touched.is_empty() {
            return;
        }
        let Some(i) = self.terrains.iter().position(|t| t.path == stroke.path) else { return };
        let x0 = stroke.touched.keys().map(|c| c.0).min().unwrap_or(0);
        let x1 = stroke.touched.keys().map(|c| c.0).max().unwrap_or(0);
        let y0 = stroke.touched.keys().map(|c| c.1).min().unwrap_or(0);
        let y1 = stroke.touched.keys().map(|c| c.1).max().unwrap_or(0);
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let doc = &self.terrains[i];
        let after = doc.rect(x0, y0, w, h);
        let mut before = after.clone();
        for (&(x, y), &code) in &stroke.touched {
            before[((y - y0) * w + (x - x0)) as usize] = code;
        }
        let label = format!("paint {} terrain", doc.map);
        self.record(Step { label, change: Change::Cells { path: stroke.path, x: x0, y: y0, w, h, before, after } });
    }

    /// Whether a paint stroke is in progress.
    pub fn painting(&self) -> bool {
        self.stroke.is_some()
    }

    /// Undoes the last step; returns its label, or `None` when there is nothing to undo.
    pub fn undo(&mut self) -> Result<Option<String>, String> {
        self.end_text_edit();
        self.paint_end();
        let Some(step) = self.history.pop_undo() else { return Ok(None) };
        if let Err(e) = self.apply_step(&step, true) {
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
        self.paint_end();
        let Some(step) = self.history.pop_redo() else { return Ok(None) };
        if let Err(e) = self.apply_step(&step, false) {
            self.history.push_redo(step);
            return Err(e);
        }
        let label = step.label.clone();
        self.history.push_undo(step);
        Ok(Some(label))
    }

    /// Puts a step's before (undo) or after (redo) state back into its document.
    fn apply_step(&mut self, step: &Step, undo: bool) -> Result<(), String> {
        self.apply_change(&step.change, undo)
    }

    fn apply_change(&mut self, change: &Change, undo: bool) -> Result<(), String> {
        match change {
            Change::Text { path, before, after } => {
                let text = if undo { before } else { after };
                let i = self.open_doc(path)?;
                self.docs[i].text = text.clone();
                Ok(())
            }
            Change::Cells { path, x, y, w, h, before, after } => {
                let codes = if undo { before } else { after };
                let i = self.terrains.iter().position(|t| &t.path == path).ok_or_else(|| format!("{path} is not open for painting"))?;
                self.terrains[i].set_rect(*x, *y, *w, *h, codes);
                Ok(())
            }
            Change::Group(members) => {
                if undo {
                    members.iter().rev().try_for_each(|c| self.apply_change(c, true))
                } else {
                    members.iter().try_for_each(|c| self.apply_change(c, false))
                }
            }
            Change::File { path, before, after } => {
                let bytes = if undo { before } else { after };
                match bytes {
                    Some(bytes) => self.write_package_file(path, bytes)?,
                    None => self.remove_package_file(path)?,
                }
                self.refresh_entries()
            }
        }
    }

    /// Removes a file of the package (and its folder when that is left empty). Refused for a
    /// link, as a write is.
    fn remove_package_file(&self, path: &str) -> Result<(), String> {
        let rel = safe_rel(path)?;
        let full = self.root.join(&rel);
        match fs::symlink_metadata(&full) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => Err(format!("cannot remove {path}: not a regular file of the package")),
            Ok(_) => {
                fs::remove_file(&full).map_err(|e| format!("cannot remove {path}: {e}"))?;
                if let Some(parent) = full.parent() {
                    let _ = fs::remove_dir(parent);
                }
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("cannot remove {path}: {e}")),
        }
    }

    /// Creates package files that are not there yet (a generated map's), written at once and
    /// recorded as one undo step named `label` (or with the operation in progress, when one is).
    /// Refused, with nothing written, when a file exists or a path is not a package path.
    pub fn create_files(&mut self, files: Vec<(String, Vec<u8>)>, label: &str) -> Result<(), String> {
        for (path, _) in &files {
            safe_rel(path)?;
            if self.root.join(path).exists() || self.docs.iter().any(|d| &d.path == path) {
                return Err(format!("{path} already exists"));
            }
        }
        let mut members = Vec::new();
        for (path, bytes) in files {
            self.write_package_file(&path, &bytes)?;
            members.push(Change::File { path, before: None, after: Some(bytes) });
        }
        self.refresh_entries()?;
        self.record(Step { label: label.to_string(), change: Change::Group(members) });
        Ok(())
    }

    /// Records a change: one undo step, or a member of the group in progress.
    fn record(&mut self, step: Step) {
        match &mut self.group {
            Some(members) => {
                if !step.change.is_noop() {
                    members.push(step.change);
                }
            }
            None => self.history.record(step),
        }
    }

    /// Starts an operation whose changes become one undo step (see [`Workspace::end_group`]).
    pub fn begin_group(&mut self) {
        self.end_text_edit();
        self.paint_end();
        self.group = Some(Vec::new());
    }

    /// Ends the operation: its changes are one undo step named `label` (nothing when it changed
    /// nothing).
    pub fn end_group(&mut self, label: &str) {
        self.paint_end();
        if let Some(members) = self.group.take() {
            if !members.is_empty() {
                self.history.record(Step { label: label.to_string(), change: Change::Group(members) });
            }
        }
    }

    /// Ends the operation after a failure: the changes it made so far are put back.
    pub fn cancel_group(&mut self) {
        self.paint_end();
        if let Some(members) = self.group.take() {
            for c in members.iter().rev() {
                let _ = self.apply_change(c, true);
            }
        }
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

    /// Removes the table `at` (one undo step). The hero cannot be removed.
    pub fn remove(&mut self, at: Locator) -> Result<(), String> {
        self.apply(Edit::Remove { at })
    }

    /// Appends a new table to a map (see [`place::place`]); returns its id when it has one.
    pub fn place(&mut self, map: &str, what: Place) -> Result<Option<String>, String> {
        place::place(self, map, what)
    }

    /// A terrain's picture changed on disk while it is open. A clean one takes the disk's cells, as
    /// one undo step; a painted one keeps its own (saving writes it over the disk). Returns whether
    /// the cells changed.
    pub fn on_disk_terrain(&mut self, path: &str, disk: &[u8]) -> Result<bool, String> {
        let Some(i) = self.terrains.iter().position(|t| t.path == path) else { return Ok(false) };
        if self.terrains[i].bytes() == disk || self.terrains[i].is_dirty() {
            return Ok(false);
        }
        let map = self.terrains[i].map.clone();
        let palette = self.palette_of(&map)?;
        let fresh = TerrainDoc::open(&map, disk.to_vec(), palette)?;
        let (w, h) = (fresh.terrain.width, fresh.terrain.height);
        let before = self.terrains[i].rect(0, 0, w, h);
        let after = fresh.rect(0, 0, w, h);
        self.terrains[i] = fresh;
        self.record(Step { label: format!("outside change: {path}"), change: Change::Cells { path: path.to_string(), x: 0, y: 0, w, h, before, after } });
        Ok(true)
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
