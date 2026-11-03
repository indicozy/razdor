//! Razdor Studio's screen: the docks of a package folder. The file tree, the script view, the
//! problems and the output (step 1); the Map tab with the game's own map renderer, the Scene and
//! the Inspector (step 2); the linked selection between the Map, the Scene, the Inspector and the
//! Script, the undo and redo, the stale banner, the outside changes, the drag to move and the
//! save (step 3). egui over the game's own window (`egui-macroquad`), docked with `egui_dock`. The
//! model is `razdor::studio`.
//!
//! Timing. A change asks for a rebuild of the package (`Workspace::rebuild`) after 300 ms without
//! another change (the debounce); a run of typing in the Script is one undo step after the same
//! pause. Outside changes settle for 250 ms before they are read.
//!
//! Desktop only: the whole module is compiled out of Android and iOS (see `ui/mod.rs`).

mod inspector;
mod map_tab;
mod scene;
mod watch;

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use egui::text::{CCursor, CCursorRange};
use egui_macroquad::egui;
use egui::{Align2, Button, Color32, CornerRadius, Key, KeyboardShortcut, Modifiers, ScrollArea, Sense, TextEdit, Ui, WidgetText};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use razdor::i18n::tr;
use razdor::pack::diag::{Diagnostic, Severity};
use razdor::pack::load::{line_of, Package};
use razdor::pack::open::lang;
use razdor::studio::map::{choose_map, map_of_file, selection_of, MapView, Selection};
use razdor::studio::{is_text, unified, Edit, Kind, Locator, Reconcile, Workspace};
use razdor::trf;

use crate::ui::dt_art::DtArt;
use map_tab::{draw as draw_map_tab, switch_map, Canvas};

/// The example package `--studio` opens when no folder is named.
pub const DEFAULT_PACKAGE: &str = "examples/packs/hello-story";

const ERROR_RED: Color32 = Color32::from_rgb(0xe0, 0x5a, 0x5a);
const WARNING_AMBER: Color32 = Color32::from_rgb(0xe8, 0xb0, 0x4f);

/// Quiet time after a change before the package is rebuilt, and after a keystroke before its
/// undo step is recorded.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// Quiet time after an outside change before the file is read.
const SETTLE: Duration = Duration::from_millis(250);

/// What the studio asks of the game after a frame.
pub enum Action {
    None,
    /// Back to the main menu.
    Exit,
}

/// One dock tab. Godot 4's docks, after the studio's design.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    FileSystem,
    Scene,
    Map,
    Script,
    Inspector,
    Problems,
    Output,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Tab::FileSystem => tr("File system"),
            Tab::Scene => tr("Scene"),
            Tab::Map => tr("Map"),
            Tab::Script => tr("Script"),
            Tab::Inspector => tr("Inspector"),
            Tab::Problems => tr("Problems"),
            Tab::Output => tr("Output"),
        }
    }
}

/// Where the Script goes next: its caret to a line (1-based), or a table's bytes selected.
#[derive(Clone, Debug)]
pub struct Jump {
    pub line: u32,
    pub range: Option<Range<usize>>,
}

/// A text file that changed on disk while it has unsaved changes: the question, and the diff
/// (its lines) when it is shown.
struct Conflict {
    path: String,
    disk: String,
    diff: Option<Vec<String>>,
}

/// The studio's state for one open package folder.
pub struct StudioScreen {
    ws: Workspace,
    dock: DockState<Tab>,
    /// Index into `ws.docs` of the document the Script tab shows and Ctrl+S saves.
    selected: Option<usize>,
    /// The output log: one line per action and its result.
    log: Vec<String>,
    /// The package's name: its folder's name.
    name: String,
    /// The map the Map tab shows and its last good compile.
    maps: MapView,
    /// The diagnostics of the last failed compile (listed in Problems after the checks').
    map_problems: Vec<Diagnostic>,
    /// Whether the game's install is there (the map is drawn only then), as of the last frame.
    installed: bool,
    /// The file last opened: it picks the map when it is a map's file.
    opened: Option<String>,
    /// What the Map, Scene and Inspector docks have selected.
    selection: Option<Selection>,
    /// The Map tab's view.
    canvas: Canvas,
    /// The Script's next jump (set by the Scene, the Inspector and a problem; taken when drawn).
    jump: Option<Jump>,
    /// When the last change asked for a rebuild.
    debounce: Option<Instant>,
    /// When the last keystroke or visual edit happened (a text burst ends after a pause).
    burst: Option<Instant>,
    /// The Script's caret as last seen: its file and character index. Moving it selects the table.
    cursor_seen: Option<(String, usize)>,
    /// The watcher of the folder (`None` when it could not start; the reason is in the log).
    watch: Option<watch::Watch>,
    /// Outside changes seen and not read yet: the path, and when it was seen.
    disk_pending: BTreeMap<String, Instant>,
    /// A dirty file changed on disk: the question.
    conflict: Option<Conflict>,
    /// Leaving with unsaved changes: the prompt is shown.
    exit_prompt: bool,
}

impl StudioScreen {
    /// Opens a package folder and runs the checks once, so the Problems tab has its list.
    pub fn open(path: &Path) -> Result<StudioScreen, String> {
        let ws = Workspace::open(path)?;
        let name = ws.root.file_name().map_or_else(|| ws.root.display().to_string(), |n| n.to_string_lossy().into_owned());
        let mut log = Vec::new();
        let watch = match watch::Watch::start(&ws.root) {
            Ok(w) => Some(w),
            Err(e) => {
                log.push(trf!("Outside changes are not seen: {reason}", reason = e));
                None
            }
        };
        let mut screen = StudioScreen {
            ws,
            dock: layout(false),
            selected: None,
            log,
            name,
            maps: MapView::default(),
            map_problems: Vec::new(),
            installed: false,
            opened: None,
            selection: None,
            canvas: Canvas::default(),
            jump: None,
            debounce: None,
            burst: None,
            cursor_seen: None,
            watch,
            disk_pending: BTreeMap::new(),
            conflict: None,
            exit_prompt: false,
        };
        screen.check();
        Ok(screen)
    }

    /// Shows map `map` and selects `army` on it, for the snapshot's `studio:<folder>:<map>:<army>`.
    /// `opts` are the snapshot's options (see `snapshot_options`); none keeps the Script as it is.
    pub fn preselect(&mut self, map: Option<&str>, army: Option<&str>, opts: &[&str]) -> Result<(), String> {
        if let Some(id) = map {
            if !self.ws.package().is_some_and(|p| p.maps.contains_key(id)) {
                return Err(format!("no map {id} in the package"));
            }
            switch_map(&mut self.maps, &mut self.canvas, &mut self.selection, id);
        }
        if let Some(id) = army {
            self.selection = Some(Selection::Army(id.to_string()));
        }
        if opts.is_empty() {
            return Ok(());
        }
        let Some(id) = self.maps.map.clone() else { return Ok(()) };
        let path = format!("maps/{id}/map.toml");
        self.open_file(&path);
        for opt in opts {
            match *opt {
                "side" => self.dock = layout(true),
                "stale" => self.snapshot_stale()?,
                "conflict" => self.snapshot_conflict(&path)?,
                other => return Err(format!("no studio option {other:?}")),
            }
        }
        if let Some(sel) = self.selection.clone() {
            self.jump = jump_for(&self.ws, self.ws.package().as_deref(), &path, &sel);
        }
        Ok(())
    }

    /// The snapshot's `stale` option: a syntax error at the end of the shown file's text, in
    /// memory only (nothing is written), so the banner shows and the screens keep the last good state.
    fn snapshot_stale(&mut self) -> Result<(), String> {
        let i = self.selected.ok_or("no file open")?;
        let text = format!("{}\n[[army\n", self.ws.docs[i].text);
        self.ws.docs[i].text = text;
        self.ws.rebuild()
    }

    /// The snapshot's `conflict` option: the shown file has a visual edit in memory (the hero's
    /// mana), and the disk got another one (its mana changed): the question, with the diff shown.
    fn snapshot_conflict(&mut self, path: &str) -> Result<(), String> {
        let disk = self.ws.read_text(path)?.replacen("mana = 10", "mana = 12", 1);
        let hero = Locator { file: path.to_string(), kind: Kind::Hero, id: None, index: 0 };
        self.ws.apply(Edit::SetValue { at: hero, key: "mana".into(), value: toml_edit::Value::from(30_i64) })?;
        match self.ws.on_disk_change(path, &disk) {
            Reconcile::Asks { path, disk } => {
                let diff = diff_lines(&self.mine(&path), &disk);
                self.conflict = Some(Conflict { path, disk, diff: Some(diff) });
                Ok(())
            }
            other => Err(format!("the outside change was not a question: {other:?}")),
        }
    }

    /// The text of the open document `path` (empty when it is not open).
    fn mine(&self, path: &str) -> String {
        self.ws.docs.iter().find(|d| d.path == path).map(|d| d.text.clone()).unwrap_or_default()
    }

    /// One frame: the outside changes and the debounced rebuild, the egui interface, then the map
    /// drawn into the Map tab's rectangle, then the egui meshes over the game's frame. `art` is the
    /// game's art and install (`None` when the install is not there).
    pub fn frame(&mut self, art: Option<&DtArt>) -> Action {
        let mut action = Action::None;
        macroquad::prelude::clear_background(macroquad::prelude::Color::new(0.125, 0.145, 0.19, 1.0));
        self.settle();
        self.prepare(art);
        egui_macroquad::ui(|ctx| action = self.show(ctx));
        // A map switched in this frame is compiled before it is drawn.
        self.prepare(art);
        let pkg = self.ws.package();
        draw_map_tab(&mut self.canvas, &self.maps, pkg.as_deref(), self.selection.as_ref(), art);
        egui_macroquad::draw();
        action
    }

    /// The timers: a text burst ends after its pause, outside changes that settled are read, and
    /// a change that settled rebuilds the package.
    fn settle(&mut self) {
        let now = Instant::now();
        if self.burst.is_some_and(|t| now.duration_since(t) >= DEBOUNCE) {
            self.ws.end_text_edit();
            self.burst = None;
        }
        self.poll_disk(now);
        if self.debounce.is_some_and(|t| now.duration_since(t) >= DEBOUNCE) {
            self.debounce = None;
            self.reload();
        }
    }

    /// A change happened: the package is rebuilt once things settle.
    fn touch(&mut self) {
        let now = Instant::now();
        self.debounce = Some(now);
        self.burst = Some(now);
    }

    /// Reads the outside changes the watcher reported, once they settle, and asks the model what
    /// each one means for an open file.
    fn poll_disk(&mut self, now: Instant) {
        if let Some(w) = &self.watch {
            for path in w.changed() {
                if path == watch::ALL {
                    for doc in &self.ws.docs {
                        self.disk_pending.insert(doc.path.clone(), now);
                    }
                } else {
                    self.disk_pending.insert(path, now);
                }
            }
        }
        let due: Vec<String> = self.disk_pending.iter().filter(|(_, t)| now.duration_since(**t) >= SETTLE).map(|(p, _)| p.clone()).collect();
        if due.is_empty() {
            return;
        }
        for path in &due {
            self.disk_pending.remove(path);
        }
        if let Err(e) = self.ws.refresh_entries() {
            self.log.push(e);
        }
        for path in due {
            if !self.ws.docs.iter().any(|d| d.path == path) {
                continue;
            }
            let Ok(disk) = self.ws.read_text(&path) else { continue };
            match self.ws.on_disk_change(&path, &disk) {
                Reconcile::Ignored => {}
                Reconcile::Reloaded => {
                    self.log.push(trf!("{path} changed on disk: reloaded (Ctrl+Z takes the old text back)", path));
                    self.touch();
                }
                Reconcile::Asks { path, disk } => self.conflict = Some(Conflict { path, disk, diff: None }),
            }
        }
    }

    /// Compiles the shown map when it is stale: on the game's install (`art`), or not at all
    /// when there is none. A compile after a visual edit ends its ghost.
    fn prepare(&mut self, art: Option<&DtArt>) {
        self.installed = art.is_some();
        if !self.maps.stale {
            return;
        }
        self.maps.stale = false;
        self.canvas.moving = None;
        let (Some(pkg), Some(art)) = (self.ws.package(), art) else {
            self.maps.good = None;
            self.map_problems.clear();
            return;
        };
        self.map_problems = self.maps.compile(&pkg, &art.install, lang());
    }

    fn show(&mut self, ctx: &egui::Context) -> Action {
        theme(ctx);
        let mut action = Action::None;
        self.shortcuts(ctx);
        let pkg = self.ws.package();
        egui::TopBottomPanel::top("studio_top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong(self.name.as_str());
                ui.weak("Razdor Studio");
                ui.separator();
                ui.menu_button(tr("Edit"), |ui| {
                    let undo = self.ws.history.undo_label().map(str::to_string);
                    let redo = self.ws.history.redo_label().map(str::to_string);
                    let undo_text = undo.as_ref().map_or_else(|| tr("Undo").to_string(), |l| trf!("Undo: {label}", label = l));
                    let redo_text = redo.as_ref().map_or_else(|| tr("Redo").to_string(), |l| trf!("Redo: {label}", label = l));
                    if ui.add_enabled(undo.is_some(), Button::new(undo_text)).clicked() {
                        self.undo();
                        ui.close_menu();
                    }
                    if ui.add_enabled(redo.is_some(), Button::new(redo_text)).clicked() {
                        self.redo();
                        ui.close_menu();
                    }
                });
                ui.separator();
                if ui.button(tr("Save")).clicked() {
                    self.save_selected();
                }
                if ui.button(tr("Save all")).clicked() {
                    self.save_all();
                }
                if ui.button(tr("Check")).clicked() {
                    self.check();
                }
                ui.separator();
                if ui.button(tr("Exit")).clicked() {
                    if self.ws.any_dirty() {
                        self.exit_prompt = true;
                    } else {
                        action = Action::Exit;
                    }
                }
            });
        });
        let style = Style::from_egui(ctx.style().as_ref());
        let mut viewer = Viewer {
            ws: &mut self.ws,
            pkg: pkg.clone(),
            selected: &mut self.selected,
            log: &mut self.log,
            map_problems: &self.map_problems,
            maps: &mut self.maps,
            canvas: &mut self.canvas,
            selection: &mut self.selection,
            has_install: self.installed,
            jump: &mut self.jump,
            cursor_seen: &mut self.cursor_seen,
            open: None,
            focus_map: false,
            changed: false,
        };
        DockArea::new(&mut self.dock)
            .style(style)
            .show_close_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show(ctx, &mut viewer);
        let (open, focus_map, changed) = (viewer.open.take(), viewer.focus_map, viewer.changed);
        if changed {
            self.touch();
        }
        if let Some(path) = open {
            self.open_file(&path);
        }
        if focus_map {
            if let Some(at) = self.dock.find_tab(&Tab::Map) {
                self.dock.set_active_tab(at);
            }
        }
        self.conflict_ui(ctx);
        if self.exit_prompt {
            let leave = self.exit_prompt_ui(ctx);
            if leave {
                action = Action::Exit;
            }
        }
        action
    }

    /// Ctrl+Shift+Z and Ctrl+Y redo, Ctrl+Z undoes, Ctrl+S saves the shown file. They are taken
    /// before the docks draw, so the Script's own undo does not run: there is one history.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let redo = ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z))
                || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y))
        });
        let undo = ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)));
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S))) {
            self.save_selected();
        }
        if redo {
            self.redo();
        } else if undo {
            self.undo();
        }
    }

    fn undo(&mut self) {
        match self.ws.undo() {
            Ok(Some(label)) => {
                self.log.push(trf!("Undone: {label}", label));
                self.touch();
            }
            Ok(None) => {}
            Err(e) => self.log.push(e),
        }
    }

    fn redo(&mut self) {
        match self.ws.redo() {
            Ok(Some(label)) => {
                self.log.push(trf!("Redone: {label}", label));
                self.touch();
            }
            Ok(None) => {}
            Err(e) => self.log.push(e),
        }
    }

    /// Opens a text file for the Script tab and shows that tab. A map's file (`maps/<id>/…`)
    /// also makes its map the one the Map tab shows.
    fn open_file(&mut self, path: &str) {
        match self.ws.open_doc(path) {
            Ok(i) => {
                self.selected = Some(i);
                self.opened = Some(path.to_string());
                if let (Some(pkg), Some(id)) = (self.ws.package(), map_of_file(path)) {
                    if pkg.maps.contains_key(id) {
                        switch_map(&mut self.maps, &mut self.canvas, &mut self.selection, id);
                    }
                }
                if let Some(at) = self.dock.find_tab(&Tab::Script) {
                    self.dock.set_active_tab(at);
                }
            }
            Err(e) => self.log.push(e),
        }
    }

    fn save_selected(&mut self) {
        let Some(i) = self.selected.filter(|&i| i < self.ws.docs.len()) else { return };
        let path = self.ws.docs[i].path.clone();
        match self.ws.save(i) {
            Ok(()) => {
                self.log.push(trf!("Saved {path}", path));
                self.reload();
            }
            Err(e) => self.log.push(e),
        }
    }

    fn save_all(&mut self) {
        match self.ws.save_all() {
            Ok(()) => self.log.push(tr("All files saved.").to_string()),
            Err(e) => self.log.push(e),
        }
        self.reload();
    }

    fn check(&mut self) {
        match self.ws.check() {
            Ok((errors, warnings)) => self.log.push(trf!("Check: {errors} errors, {warnings} warnings", errors, warnings)),
            Err(e) => self.log.push(e),
        }
        self.reload();
    }

    /// Rebuilds the package from the disk and the open text, and makes the map to compile next.
    /// The shown map stays when the package still has it; otherwise the rule of `choose_map`
    /// picks one.
    fn reload(&mut self) {
        if let Err(e) = self.ws.rebuild() {
            self.log.push(e);
        }
        if let Some(pkg) = self.ws.package() {
            let kept = self.maps.map.as_ref().is_some_and(|m| pkg.maps.contains_key(m));
            if !kept {
                if let Some(id) = choose_map(&pkg, self.opened.as_deref()) {
                    switch_map(&mut self.maps, &mut self.canvas, &mut self.selection, &id);
                }
            }
        }
        self.maps.stale = true;
    }

    /// The question about a file changed on disk: Reload, Keep mine, or the diff first.
    fn conflict_ui(&mut self, ctx: &egui::Context) {
        let Some(c) = self.conflict.as_mut() else { return };
        let mut answer: Option<bool> = None;
        let title = trf!("{path} changed on disk", path = c.path);
        let mine = self.ws.docs.iter().find(|d| d.path == c.path).map(|d| d.text.clone()).unwrap_or_default();
        egui::Window::new(title).collapsible(false).resizable(true).default_width(560.0).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(tr("The file changed on disk, and it has unsaved changes here."));
            ui.horizontal(|ui| {
                if ui.button(tr("Reload")).clicked() {
                    answer = Some(true);
                }
                if ui.button(tr("Keep mine")).clicked() {
                    answer = Some(false);
                }
                let toggle = if c.diff.is_some() { tr("Hide diff") } else { tr("Show diff") };
                if ui.button(toggle).clicked() {
                    c.diff = match c.diff {
                        Some(_) => None,
                        None => Some(diff_lines(&mine, &c.disk)),
                    };
                }
            });
            if let Some(lines) = &c.diff {
                ui.separator();
                ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
                    for line in lines {
                        ui.add(egui::Label::new(diff_text(line)).wrap_mode(egui::TextWrapMode::Extend));
                    }
                });
            }
        });
        let Some(answer) = answer else { return };
        let Some(c) = self.conflict.take() else { return };
        let result = if answer { self.ws.take_disk(&c.path, &c.disk) } else { self.ws.keep_mine(&c.path, &c.disk) };
        match result {
            Ok(()) if answer => {
                self.log.push(trf!("{path} reloaded from disk (Ctrl+Z takes the old text back)", path = c.path));
                self.touch();
            }
            Ok(()) => self.log.push(trf!("{path} kept: the disk text is the saved one now", path = c.path)),
            Err(e) => self.log.push(e),
        }
    }

    /// The prompt on leaving with unsaved files. True when the studio should close.
    fn exit_prompt_ui(&mut self, ctx: &egui::Context) -> bool {
        let mut leave = false;
        egui::Window::new(tr("Unsaved changes")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(tr("Save the changed files before leaving?"));
            ui.horizontal(|ui| {
                if ui.button(tr("Save all")).clicked() {
                    self.save_all();
                    self.exit_prompt = false;
                    leave = !self.ws.any_dirty();
                }
                if ui.button(tr("Discard")).clicked() {
                    leave = true;
                }
                if ui.button(tr("Cancel")).clicked() {
                    self.exit_prompt = false;
                }
            });
        });
        leave
    }
}

/// The lines of a unified diff, coloured: additions green, removals red, the hunk headers weak.
fn diff_text(line: &str) -> egui::RichText {
    let text = egui::RichText::new(line).monospace();
    match line.chars().next() {
        Some('+') if !line.starts_with("+++") => text.color(Color32::from_rgb(0x7f, 0xc8, 0x7f)),
        Some('-') if !line.starts_with("---") => text.color(ERROR_RED),
        _ => text.color(Color32::from_gray(0x9a)),
    }
}

/// The diff of the document's text (`mine`) and the disk text, as lines.
fn diff_lines(mine: &str, disk: &str) -> Vec<String> {
    unified(mine, disk).lines().map(str::to_string).collect()
}

/// The Script's jump to the table of `sel` in the map file `path`: the table's bytes selected and
/// its line. `None` when the selection has no table in the file.
pub fn jump_for(ws: &Workspace, pkg: Option<&Package>, path: &str, sel: &Selection) -> Option<Jump> {
    let spec = pkg?.maps.get(map_of_file(path)?)?.spec.as_ref()?;
    let loc = razdor::studio::map::locator_of(path, spec, sel)?;
    let text = ws.text_of(path).ok()?;
    let (_, range) = ws.spans(path).ok()?.into_iter().find(|(l, _)| *l == loc)?;
    Some(Jump { line: line_of(&text, range.start), range: Some(range) })
}

/// The byte offset where a 1-based line starts.
fn line_start(text: &str, line: u32) -> usize {
    text.split_inclusive('\n').take(line.saturating_sub(1) as usize).map(str::len).sum()
}

/// The character index of a byte offset (egui's cursors count characters).
fn char_index(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

/// The default layout, as Godot 4's: FileSystem over Scene on the left, Map and Script in the
/// centre (Script shown), Inspector on the right, Problems and Output under the centre. `side`
/// puts the Script beside the Map instead of in its tabs (the snapshot's `side` option).
fn layout(side: bool) -> DockState<Tab> {
    let mut dock = DockState::new(if side { vec![Tab::Map] } else { vec![Tab::Map, Tab::Script] });
    let tree = dock.main_surface_mut();
    // egui_dock's fraction is the share of the node that comes first (the new left or upper
    // node): the left column is a quarter of the 80% centre-and-files area, so 20% of the width.
    let [centre, _inspector] = tree.split_right(NodeIndex::root(), 0.8, vec![Tab::Inspector]);
    let [centre, files] = tree.split_left(centre, 0.25, vec![Tab::FileSystem]);
    let [_files, _scene] = tree.split_below(files, 0.5, vec![Tab::Scene]);
    let [centre, _bottom] = tree.split_below(centre, 0.75, vec![Tab::Problems, Tab::Output]);
    if side {
        tree.split_right(centre, 0.5, vec![Tab::Script]);
    }
    if let Some(at) = dock.find_tab(&Tab::Map) {
        dock.set_active_tab(at);
    }
    dock
}

/// The Godot 4-like dark theme: flat grey panels, one blue accent, small rounding.
fn theme(ctx: &egui::Context) {
    let c = Color32::from_rgb;
    let mut v = egui::Visuals::dark();
    v.panel_fill = c(0x20, 0x25, 0x31);
    v.window_fill = c(0x1a, 0x1d, 0x26);
    v.extreme_bg_color = c(0x1a, 0x1d, 0x26);
    v.faint_bg_color = c(0x24, 0x29, 0x36);
    v.override_text_color = Some(c(0xe0, 0xe0, 0xe0));
    v.selection.bg_fill = c(0x4f, 0x8e, 0xdc);
    v.hyperlink_color = c(0x4f, 0x8e, 0xdc);
    let states = [
        (&mut v.widgets.noninteractive, c(0x20, 0x25, 0x31)),
        (&mut v.widgets.inactive, c(0x2b, 0x30, 0x3d)),
        (&mut v.widgets.hovered, c(0x36, 0x3c, 0x4c)),
        (&mut v.widgets.active, c(0x4f, 0x8e, 0xdc)),
        (&mut v.widgets.open, c(0x2b, 0x30, 0x3d)),
    ];
    for (w, fill) in states {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.corner_radius = CornerRadius::same(3);
    }
    ctx.set_visuals(v);
}

/// The tabs' contents. Each tab is drawn from the same borrowed state; a file asked for (a click
/// in the tree or on a problem) is opened after the dock is drawn.
struct Viewer<'a> {
    ws: &'a mut Workspace,
    /// The package as last rebuilt (the screens read it; the model keeps the last good one).
    pkg: Option<Rc<Package>>,
    selected: &'a mut Option<usize>,
    log: &'a mut Vec<String>,
    map_problems: &'a [Diagnostic],
    maps: &'a mut MapView,
    canvas: &'a mut Canvas,
    selection: &'a mut Option<Selection>,
    has_install: bool,
    /// The Script's next jump (taken by the Script tab when it is drawn).
    jump: &'a mut Option<Jump>,
    cursor_seen: &'a mut Option<(String, usize)>,
    /// Requests for after the dock is drawn: a file to open, the Map tab to show.
    open: Option<String>,
    focus_map: bool,
    /// A document or the package changed in this frame (the rebuild and the burst timers start).
    changed: bool,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Tab) -> WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match tab {
            Tab::FileSystem => self.file_system(ui),
            Tab::Scene => self.scene(ui),
            Tab::Map => self.map_tab(ui),
            Tab::Script => self.script(ui),
            Tab::Inspector => self.inspector(ui),
            Tab::Problems => self.problems(ui),
            Tab::Output => self.output(ui),
        }
    }

    /// The map is drawn over the Map tab's body by the macroquad pass, so egui paints nothing there.
    fn clear_background(&self, tab: &Tab) -> bool {
        !matches!(tab, Tab::Map)
    }
}

impl Viewer<'_> {
    fn file_system(&mut self, ui: &mut Ui) {
        ui.weak(tr("Package folder"));
        let tree = build_tree(&self.ws.entries);
        let open = &mut self.open;
        ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| show_dir(ui, &tree, open));
    }

    /// Selects `sel` from the Scene or the canvas: the Script goes to its table when the Script
    /// shows the shown map's file.
    pub(super) fn select(&mut self, sel: Selection) {
        let shown = self.selected.and_then(|i| self.ws.docs.get(i)).map(|d| d.path.clone());
        if let Some(path) = shown.filter(|p| map_of_file(p) == self.maps.map.as_deref()) {
            *self.jump = jump_for(&*self.ws, self.pkg.as_deref(), &path, &sel);
        }
        *self.selection = Some(sel);
    }

    fn script(&mut self, ui: &mut Ui) {
        if self.ws.docs.is_empty() {
            ui.weak(tr("No file open: pick a text file in the file system."));
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (i, doc) in self.ws.docs.iter().enumerate() {
                let name = file_name(&doc.path);
                let label = if doc.is_dirty() { format!("{name} (*)") } else { name.to_string() };
                if ui.selectable_label(*self.selected == Some(i), label).on_hover_text(&doc.path).clicked() {
                    *self.selected = Some(i);
                }
            }
        });
        ui.separator();
        let Some(i) = self.selected.filter(|&i| i < self.ws.docs.len()) else {
            ui.weak(tr("No file open: pick a text file in the file system."));
            return;
        };
        let path = self.ws.docs[i].path.clone();
        let before = self.ws.docs[i].text.clone();
        let size = ui.available_size();
        let id = ui.id().with("studio-script");
        let row = ui.text_style_height(&egui::TextStyle::Monospace);
        let mut area = ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(j) = self.jump.take() {
            // The caret goes to the table's start, the table is selected, and the view scrolls to
            // a few lines above it. A line alone puts the caret at its start.
            let text = &self.ws.docs[i].text;
            let (start, end) = match &j.range {
                Some(r) => (r.start, r.end),
                None => (line_start(text, j.line), line_start(text, j.line)),
            };
            let line = line_of(text, start);
            let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(char_index(text, start)), CCursor::new(char_index(text, end)))));
            state.store(ui.ctx(), id);
            ui.memory_mut(|m| m.request_focus(id));
            area = area.vertical_scroll_offset(line.saturating_sub(4) as f32 * row);
        }
        let resp = area
            .show(ui, |ui| {
                ui.add(TextEdit::multiline(&mut self.ws.docs[i].text).id(id).code_editor().desired_width(f32::INFINITY).min_size(size))
            })
            .inner;
        if resp.changed() {
            self.ws.begin_text_edit(&path, &before);
            self.changed = true;
        }
        // The caret moved: the table it is in selects its object (the shown map's file only).
        let Some(state) = TextEdit::load_state(ui.ctx(), id) else { return };
        let Some(range) = state.cursor.char_range() else { return };
        let at = range.primary.index;
        if self.cursor_seen.as_ref() == Some(&(path.clone(), at)) {
            return;
        }
        *self.cursor_seen = Some((path.clone(), at));
        if map_of_file(&path).is_some() && map_of_file(&path) == self.maps.map.as_deref() {
            let text = &self.ws.docs[i].text;
            let byte = text.char_indices().nth(at).map_or(text.len(), |(b, _)| b);
            let sel = self.ws.locator_at(&path, byte).and_then(|loc| selection_of(&loc));
            if let Some(sel) = sel {
                if self.selection.as_ref() != Some(&sel) {
                    *self.selection = Some(sel);
                }
            }
        }
    }

    fn problems(&mut self, ui: &mut Ui) {
        // The checks' problems, then the failed compile's (those the checks already listed are not repeated).
        let all: Vec<&Diagnostic> = self
            .ws
            .problems
            .iter()
            .chain(self.map_problems.iter().filter(|d| !self.ws.problems.contains(d)))
            .collect();
        if all.is_empty() {
            ui.weak(tr("No problems. Press Check to look again."));
            return;
        }
        let mut clicked: Option<(String, Option<u32>)> = None;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, d) in all.iter().enumerate() {
                if problem_row(ui, i, d) && !d.file.is_empty() {
                    clicked = Some((d.file.clone(), d.line));
                }
            }
        });
        if let Some((file, line)) = clicked {
            self.open = Some(file);
            *self.jump = line.map(|line| Jump { line, range: None });
        }
    }

    fn output(&mut self, ui: &mut Ui) {
        if self.log.is_empty() {
            ui.weak(tr("Nothing yet."));
            return;
        }
        ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            for line in self.log.iter() {
                ui.monospace(line);
            }
        });
    }
}

/// One problem: its severity, `file:line`, the message and the hint. Returns whether it was clicked.
fn problem_row(ui: &mut Ui, i: usize, d: &Diagnostic) -> bool {
    let (color, word) = match d.severity {
        Severity::Error => (ERROR_RED, tr("Error")),
        Severity::Warning => (WARNING_AMBER, tr("Warning")),
    };
    let location = match (d.file.is_empty(), d.line) {
        (true, _) => String::new(),
        (false, Some(line)) => format!("{}:{line}", d.file),
        (false, None) => d.file.clone(),
    };
    // The severity and the location on the first line; the message wraps under them (the
    // Problems tab is narrow), then the hint.
    let row = ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.colored_label(color, word);
            if !location.is_empty() {
                ui.monospace(location);
            }
        });
        ui.label(d.message.as_str());
        if let Some(hint) = &d.hint {
            ui.weak(trf!("Hint: {hint}", hint));
        }
    });
    ui.add_space(2.0);
    ui.separator();
    let id = ui.id().with(("studio-problem", i));
    let response = ui.interact(row.response.rect, id, Sense::click());
    response.clicked()
}

/// The package's files as a folder tree.
#[derive(Default)]
struct Dir {
    dirs: BTreeMap<String, Dir>,
    /// Package-relative paths of the files in this folder.
    files: Vec<String>,
}

fn build_tree(entries: &[String]) -> Dir {
    let mut root = Dir::default();
    for path in entries {
        let mut dir = &mut root;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                dir = dir.dirs.entry(part.to_string()).or_default();
            } else {
                dir.files.push(path.clone());
            }
        }
    }
    root
}

/// Draws a folder: its subfolders as collapsible headers, then its files. Text files are
/// clickable (`open` gets the path); other files are shown as plain labels.
fn show_dir(ui: &mut Ui, dir: &Dir, open: &mut Option<String>) {
    for (name, sub) in &dir.dirs {
        egui::CollapsingHeader::new(name.as_str()).default_open(true).show(ui, |ui| show_dir(ui, sub, open));
    }
    for path in &dir.files {
        let name = file_name(path);
        if is_text(path) {
            if ui.selectable_label(false, name).clicked() {
                *open = Some(path.clone());
            }
        } else {
            ui.weak(name);
        }
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `--studio [<package folder>]` on the command line: the folder to open. The example package
/// when none is named. `None` when the flag is not there.
pub fn requested() -> Option<Result<PathBuf, String>> {
    let args: Vec<String> = std::env::args().collect();
    let at = args.iter().position(|a| a == "--studio")?;
    let folder = args.get(at + 1).filter(|a| !a.starts_with("--"));
    Some(match folder {
        Some(f) => Ok(PathBuf::from(f)),
        None if Path::new(DEFAULT_PACKAGE).is_dir() => Ok(PathBuf::from(DEFAULT_PACKAGE)),
        None => Err(format!("--studio: name a package folder ({DEFAULT_PACKAGE} is not here)")),
    })
}
