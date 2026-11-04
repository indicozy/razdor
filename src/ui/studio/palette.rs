//! The studio's command palette (UX6, spec §10): Ctrl+Shift+P lists the commands, Ctrl+P the objects and
//! files of the package (go to), and the top bar's field both. A modal over the workspace: a search field,
//! the ranked rows (icon, name, group, shortcut), arrows and Enter run the row, Esc closes, a click runs it.
//! While it is open the shortcuts wait (the search field has the keyboard, and Ctrl and the typed keys
//! reach it, not the commands). The ranking is `studio::palette`; the entries are the registry's commands
//! (`commands.rs`) and the Ctrl+P list (`studio::nav::goto_index`).

use egui_macroquad::egui::{self, Align2, Color32, Key, Modifiers, ScrollArea, Sense, Stroke, Ui};
use egui_phosphor::regular as ic;
use razdor::i18n::{n_, tr};
use razdor::studio::nav::{GoKind, GoTo};
use razdor::studio::palette::rank;

use super::commands::{self, Prefs, Scope, COMMANDS};
use super::theme;

/// The window's size (points), as the spec's palette has it.
const WIDTH: f32 = 580.0;
const HEIGHT: f32 = 330.0;
/// The rows a result list shows (the rest are reached by the query).
const MAX_ROWS: usize = 300;
/// A row's height in points.
const ROW: f32 = 26.0;

/// The palette's modes: the commands (Ctrl+Shift+P), the objects and files (Ctrl+P), or both (the top bar's field).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Commands,
    Go,
    All,
}

/// What a row runs: a registry command, or a go-to entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    Command(&'static str),
    Go(GoTo),
}

/// One row: its icon, its name, its group, its shortcut (or the file or map it is in), the names it is
/// matched by, what it runs, and whether it runs now (a command the format lacks does not).
struct Row {
    icon: &'static str,
    title: String,
    group: String,
    hint: String,
    names: Vec<String>,
    pick: Pick,
    runnable: bool,
}

/// The palette's state: whether it is open and in which mode, its query, the rows it was opened with and
/// the ranked hits of the query.
#[derive(Default)]
pub struct Palette {
    open: Option<Mode>,
    /// Asked for by a command, a snapshot or the top bar: opened at the next frame with this query.
    asked: Option<(Mode, String)>,
    query: String,
    rows: Vec<Row>,
    hits: Vec<usize>,
    /// The query the hits were ranked for.
    hits_for: Option<String>,
    selected: usize,
    focus: bool,
}

/// The icon of a go-to kind.
fn kind_icon(kind: GoKind) -> &'static str {
    match kind {
        GoKind::File => ic::FILE,
        GoKind::Map => ic::MAP_TRIFOLD,
        GoKind::Army => ic::SWORD,
        GoKind::Building => ic::BUILDINGS,
        GoKind::Trigger => ic::LIGHTNING,
        GoKind::Dialog => ic::CHAT_CIRCLE,
        GoKind::Unit => ic::USER,
        GoKind::Item => ic::SPARKLE,
        GoKind::Character => ic::USER_CIRCLE,
        GoKind::Var => ic::HASH,
    }
}

/// The group's name of a go-to kind (the catalogue's words).
fn kind_name(kind: GoKind) -> &'static str {
    match kind {
        GoKind::File => n_("File"),
        GoKind::Map => n_("Map"),
        GoKind::Army => n_("Army"),
        GoKind::Building => n_("Building"),
        GoKind::Trigger => n_("Trigger"),
        GoKind::Dialog => n_("Dialog"),
        GoKind::Unit => n_("Unit"),
        GoKind::Item => n_("Item"),
        GoKind::Character => n_("Character"),
        GoKind::Var => n_("Variable"),
    }
}

/// The group of a command: the workspace it applies in.
fn scope_name(scope: Scope) -> &'static str {
    match scope {
        Scope::Global => n_("Everywhere"),
        Scope::Map => n_("Map workspace"),
        Scope::Inspector => n_("Inspector"),
        Scope::Story => n_("Story workspace"),
        Scope::Data => n_("Data workspace"),
    }
}

/// The rows of the commands: each command once, with its icon, its name in the language shown, its group and
/// its shortcut. The names it matches by are its id and both its English and its shown names.
fn command_rows(prefs: &Prefs) -> Vec<Row> {
    COMMANDS
        .iter()
        .map(|c| Row {
            icon: c.icon,
            title: tr(c.name).to_string(),
            group: tr(scope_name(c.scope)).to_string(),
            hint: commands::binding_of(c, prefs).map(|b| b.text()).unwrap_or_default(),
            names: vec![c.id.to_string(), c.name.to_string(), tr(c.name).to_string()],
            pick: Pick::Command(c.id),
            runnable: c.later.is_empty(),
        })
        .collect()
}

/// The rows of the go-to entries: the id, the group (the kind), the context (the map or the file's folder).
fn go_rows(entries: Vec<GoTo>) -> Vec<Row> {
    entries
        .into_iter()
        .map(|g| Row {
            icon: kind_icon(g.kind),
            title: g.id.clone(),
            group: tr(kind_name(g.kind)).to_string(),
            hint: g.sub.clone(),
            names: vec![g.id.clone()],
            pick: Pick::Go(g),
            runnable: true,
        })
        .collect()
}

impl Palette {
    /// Asks for the palette to open in `mode` with `query` (a command, the snapshot or the top bar).
    pub fn request(&mut self, mode: Mode, query: &str) {
        self.asked = Some((mode, query.to_string()));
    }

    /// Whether the palette is open (the shortcuts wait while it is).
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Opens the asked palette: its rows are built once (the Ctrl+P list is the package's), and the query is set.
    fn open_now(&mut self, mode: Mode, query: String, prefs: &Prefs, goto: impl FnOnce() -> Vec<GoTo>) {
        let mut rows = Vec::new();
        if mode != Mode::Go {
            rows.extend(command_rows(prefs));
        }
        if mode != Mode::Commands {
            rows.extend(go_rows(goto()));
        }
        self.rows = rows;
        self.open = Some(mode);
        self.query = query;
        self.hits_for = None;
        self.selected = 0;
        self.focus = true;
    }

    /// Ranks the rows for the query (again only when it changed).
    fn rank_now(&mut self) {
        if self.hits_for.as_deref() == Some(self.query.as_str()) {
            return;
        }
        let names: Vec<Vec<String>> = self.rows.iter().map(|r| r.names.clone()).collect();
        self.hits = rank(&self.query, &names).into_iter().map(|(i, _)| i).collect();
        self.hits_for = Some(self.query.clone());
        self.selected = self.selected.min(self.hits.len().saturating_sub(1));
    }

    /// The palette for this frame: opens an asked palette, takes the keys, draws the window. Returns the row
    /// the author chose (Enter or a click), and closes the palette then. `goto` gives the Ctrl+P list; it is
    /// read only when the palette opens in a mode that has it.
    pub fn show(&mut self, ctx: &egui::Context, prefs: &Prefs, goto: impl FnOnce() -> Vec<GoTo>) -> Option<Pick> {
        if let Some((mode, query)) = self.asked.take() {
            self.open_now(mode, query, prefs, goto);
        }
        self.open?;
        let (down, up, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if esc {
            self.open = None;
            return None;
        }
        self.rank_now();
        let shown = self.hits.len().min(MAX_ROWS);
        if down && shown > 0 {
            self.selected = (self.selected + 1).min(shown - 1);
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        let mut pick: Option<Pick> = None;
        let mut query_changed = false;
        // An area over the workspace, the spec's size (580 × 330 px): the frame is the window's tokens.
        egui::Area::new(egui::Id::new("studio-palette"))
            .anchor(Align2::CENTER_TOP, [0.0, 60.0])
            .order(egui::Order::Foreground)
            .movable(false)
            .show(ctx, |ui| {
                egui::Frame::NONE.fill(theme::PANEL).stroke(Stroke::new(1.0, theme::LINE)).corner_radius(theme::WINDOW_RADIUS).inner_margin(8).show(ui, |ui| {
                    ui.set_width(WIDTH - 16.0);
                    ui.set_height(HEIGHT - 16.0);
                    let field = ui.add(egui::TextEdit::singleline(&mut self.query).hint_text(tr("Type a command or an object's name")).desired_width(f32::INFINITY));
                    if std::mem::take(&mut self.focus) {
                        field.request_focus();
                    }
                    query_changed = field.changed();
                    ui.add_space(4.0);
                    if self.hits.is_empty() {
                        ui.weak(tr("Nothing found"));
                        return;
                    }
                    if enter {
                        if let Some(row) = self.hits.get(self.selected).and_then(|&i| self.rows.get(i)) {
                            if row.runnable {
                                pick = Some(row.pick.clone());
                            }
                        }
                    }
                    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        for k in 0..shown {
                            let row = &self.rows[self.hits[k]];
                            let (hit, p) = draw_row(ui, row, k == self.selected);
                            if hit {
                                self.selected = k;
                            }
                            if let Some(p) = p {
                                pick.get_or_insert(p);
                            }
                        }
                    });
                });
            });
        if query_changed {
            self.selected = 0;
        }
        if pick.is_some() {
            self.open = None;
        }
        pick
    }
}

/// One row: its icon, name, group and shortcut. Returns whether the pointer is over it (it becomes the selected
/// row) and the row's pick when it is clicked.
fn draw_row(ui: &mut Ui, row: &Row, selected: bool) -> (bool, Option<Pick>) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 3.0, theme::HOVER);
        painter.rect_stroke(rect, 3.0, Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        painter.rect_filled(rect, 3.0, theme::RAISED);
    }
    let text = if row.runnable { theme::TEXT } else { theme::FAINT };
    let font = egui::FontId::proportional(theme::TEXT_SIZE);
    painter.text(egui::pos2(rect.left() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, row.icon, egui::FontId::proportional(theme::ICON), text);
    painter.text(egui::pos2(rect.left() + 32.0, rect.center().y), egui::Align2::LEFT_CENTER, &row.title, font.clone(), text);
    let title_w = painter.layout_no_wrap(row.title.clone(), font.clone(), text).size().x;
    let group = if row.hint.is_empty() || row.runnable { row.group.clone() } else { format!("{} · {}", row.group, row.hint) };
    painter.text(egui::pos2(rect.left() + 40.0 + title_w, rect.center().y), egui::Align2::LEFT_CENTER, group, egui::FontId::proportional(theme::SMALL), theme::DIM);
    if !row.hint.is_empty() && row.runnable {
        painter.text(egui::pos2(rect.right() - 8.0, rect.center().y), egui::Align2::RIGHT_CENTER, &row.hint, egui::FontId::monospace(theme::MONO), Color32::from_gray(0xc8));
    }
    let pick = (resp.clicked() && row.runnable).then(|| row.pick.clone());
    (resp.hovered(), pick)
}
