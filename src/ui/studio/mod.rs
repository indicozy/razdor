//! Razdor Studio's screen (step 1 of the studio): the docks of a package folder, the file tree,
//! the script view, the problems and the output. egui over the game's own window
//! (`egui-macroquad`), docked with `egui_dock`. The model is `razdor::studio`; the tabs Scene,
//! Map and Inspector are placeholders until their steps.
//!
//! Desktop only: the whole module is compiled out of Android and iOS (see `ui/mod.rs`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use egui_macroquad::egui;
use egui::{Color32, CornerRadius, Key, KeyboardShortcut, Modifiers, ScrollArea, Sense, TextEdit, Ui, WidgetText};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use razdor::i18n::tr;
use razdor::pack::diag::{Diagnostic, Severity};
use razdor::studio::{is_text, Workspace};
use razdor::trf;

/// The example package `--studio` opens when no folder is named.
pub const DEFAULT_PACKAGE: &str = "examples/packs/hello-story";

const ERROR_RED: Color32 = Color32::from_rgb(0xe0, 0x5a, 0x5a);
const WARNING_AMBER: Color32 = Color32::from_rgb(0xe8, 0xb0, 0x4f);

/// What the studio asks of the game after a frame.
pub enum Action {
    None,
    /// Back to the main menu.
    Exit,
}

/// One dock tab. Godot 4's docks, after the studio's design (step 1 fills FileSystem, Script,
/// Problems and Output).
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
}

impl StudioScreen {
    /// Opens a package folder and runs the checks once, so the Problems tab has its list.
    pub fn open(path: &Path) -> Result<StudioScreen, String> {
        let ws = Workspace::open(path)?;
        let name = ws.root.file_name().map_or_else(|| ws.root.display().to_string(), |n| n.to_string_lossy().into_owned());
        let mut screen = StudioScreen { ws, dock: layout(), selected: None, log: Vec::new(), name };
        screen.check();
        Ok(screen)
    }

    /// One frame: the egui interface, then the draw of its meshes over the game's frame.
    pub fn frame(&mut self) -> Action {
        let mut action = Action::None;
        macroquad::prelude::clear_background(macroquad::prelude::Color::new(0.125, 0.145, 0.19, 1.0));
        egui_macroquad::ui(|ctx| action = self.show(ctx));
        egui_macroquad::draw();
        action
    }

    fn show(&mut self, ctx: &egui::Context) -> Action {
        theme(ctx);
        let mut action = Action::None;
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S))) {
            self.save_selected();
        }
        egui::TopBottomPanel::top("studio_top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong(self.name.as_str());
                ui.weak("Razdor Studio");
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
                    action = Action::Exit;
                }
            });
        });
        let style = Style::from_egui(ctx.style().as_ref());
        let mut viewer = Viewer { ws: &mut self.ws, selected: &mut self.selected, log: &self.log, open: None };
        DockArea::new(&mut self.dock)
            .style(style)
            .show_close_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show(ctx, &mut viewer);
        if let Some(path) = viewer.open.take() {
            self.open_file(&path);
        }
        action
    }

    /// Opens a text file for the Script tab and shows that tab.
    fn open_file(&mut self, path: &str) {
        match self.ws.open_doc(path) {
            Ok(i) => {
                self.selected = Some(i);
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
            Ok(()) => self.log.push(trf!("Saved {path}", path)),
            Err(e) => self.log.push(e),
        }
    }

    fn save_all(&mut self) {
        match self.ws.save_all() {
            Ok(()) => self.log.push(tr("All files saved.").to_string()),
            Err(e) => self.log.push(e),
        }
    }

    fn check(&mut self) {
        match self.ws.check() {
            Ok((errors, warnings)) => self.log.push(trf!("Check: {errors} errors, {warnings} warnings", errors, warnings)),
            Err(e) => self.log.push(e),
        }
    }
}

/// The default layout, as Godot 4's: FileSystem over Scene on the left, Map and Script in the
/// centre (Script shown), Inspector on the right, Problems and Output under the centre.
fn layout() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Map, Tab::Script]);
    let tree = dock.main_surface_mut();
    let [centre, _inspector] = tree.split_right(NodeIndex::root(), 0.8, vec![Tab::Inspector]);
    let [centre, files] = tree.split_left(centre, 0.75, vec![Tab::FileSystem]);
    let [_files, _scene] = tree.split_below(files, 0.5, vec![Tab::Scene]);
    let [_centre, _bottom] = tree.split_below(centre, 0.75, vec![Tab::Problems, Tab::Output]);
    if let Some(at) = dock.find_tab(&Tab::Script) {
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
    selected: &'a mut Option<usize>,
    log: &'a [String],
    open: Option<String>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Tab) -> WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match tab {
            Tab::FileSystem => self.file_system(ui),
            Tab::Script => self.script(ui),
            Tab::Problems => self.problems(ui),
            Tab::Output => self.output(ui),
            Tab::Scene | Tab::Map | Tab::Inspector => {
                ui.centered_and_justified(|ui| {
                    ui.weak(tr("Coming next"));
                });
            }
        }
    }
}

impl Viewer<'_> {
    fn file_system(&mut self, ui: &mut Ui) {
        ui.weak(tr("Package folder"));
        let tree = build_tree(&self.ws.entries);
        let open = &mut self.open;
        ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| show_dir(ui, &tree, open));
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
        let doc = &mut self.ws.docs[i];
        let size = ui.available_size();
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add(TextEdit::multiline(&mut doc.text).code_editor().desired_width(f32::INFINITY).min_size(size));
        });
    }

    fn problems(&mut self, ui: &mut Ui) {
        if self.ws.problems.is_empty() {
            ui.weak(tr("No problems. Press Check to look again."));
            return;
        }
        let open = &mut self.open;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, d) in self.ws.problems.iter().enumerate() {
                if problem_row(ui, i, d) && !d.file.is_empty() {
                    *open = Some(d.file.clone());
                }
            }
        });
    }

    fn output(&mut self, ui: &mut Ui) {
        if self.log.is_empty() {
            ui.weak(tr("Nothing yet."));
            return;
        }
        ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            for line in self.log {
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
