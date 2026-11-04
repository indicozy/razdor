//! The Story screen (P4): the flow graph of the shown map (dialogs, triggers and variables, with
//! labelled edges), and the Inspector's forms for a selected trigger, dialog or variable. Every
//! change is one undo step: a single edit through `Workspace::apply_doc`, or an operation that
//! touches several files, as a group. The edits are `studio::story`'s; the rows' widgets are
//! `story_forms`.

use std::collections::BTreeMap;

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use egui_macroquad::egui;
use razdor::i18n::tr;
use razdor::pack::load::Package;
use razdor::pack::triggers as pt;
use razdor::studio::campaign_graph as cg;
use razdor::studio::doc::DocEdit;
use razdor::studio::story::{self as st, Options, Section};
use razdor::studio::story_graph::{self as sg, Kind, Story};
use razdor::studio::texts as tx;
use razdor::studio::vars::{self as vars, Holder, Role};
use razdor::trf;
use egui_phosphor::regular as ic;
use toml_edit::Value;

use super::forms;
use super::inspector_kit as kit;
use super::scenarios::Focus;
use super::story_forms::{self as sf, Cx, RowAct};
use super::{Tab, Viewer, ERROR_RED};

const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x8e, 0xdc);
const GREY: Color32 = Color32::from_rgb(0x8a, 0x8f, 0x9c);
const DIALOG_FILL: Color32 = Color32::from_rgb(0x2b, 0x30, 0x3d);
const TRIGGER_FILL: Color32 = Color32::from_rgb(0x2c, 0x3a, 0x2e);
const VAR_FILL: Color32 = Color32::from_rgb(0x3a, 0x2d, 0x47);
const LABEL_FILL: Color32 = Color32::from_rgb(0x1c, 0x1f, 0x26);
/// The first choice row's centre, below a dialog's title, and the step between rows.
const ROW_Y0: f32 = 40.0;
const ROW_STEP: f32 = 20.0;
/// The graph's zoom: Ctrl+scroll (or a pinch) stays within these; the first view fits the tab.
const ZOOM_MIN: f32 = 0.3;
const ZOOM_MAX: f32 = 2.0;

/// What the Story screen has selected (the Inspector shows its form).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum StorySel {
    #[default]
    None,
    Trigger(String),
    Dialog(String),
    Var(String),
}

/// A port a link is dragged from: a dialog's choice, or a trigger's output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Port {
    Choice(String, String),
    Trigger(String),
}

/// The Story screen's state, kept across frames.
#[derive(Default)]
pub struct StoryUi {
    pub sel: StorySel,
    /// A snapshot's option (`trigger` or `dialog`) waiting for the package to be built (see
    /// `story_resolve_pending`).
    pub pending: Option<String>,
    /// The saved positions of the shown map's nodes (read once per map).
    saved: Option<(String, BTreeMap<String, [f32; 2]>)>,
    /// The port being dragged to a dialog.
    link: Option<Port>,
    /// The last operation's refusal, shown under the screen and the forms.
    pub error: Option<String>,
    /// The graph's zoom for a map. Unset (or for another map) the view fits the tab.
    pub zoom: Option<(String, f32)>,
}

/// The texts of the language shown (any language when that one is missing).
fn texts_shown(pkg: &Package) -> BTreeMap<String, String> {
    pkg.texts.get(razdor::pack::open::lang()).or_else(|| pkg.texts.values().next()).cloned().unwrap_or_default()
}

impl Viewer<'_> {
    /// Applies edits to one package file as one undo step; a refusal is shown under the screen.
    fn story_apply(&mut self, label: &str, file: &str, edits: &[DocEdit]) {
        match self.ws.apply_doc(file, label, edits) {
            Ok(()) => {
                self.scen.story.error = None;
                self.changed = true;
            }
            Err(e) => {
                self.log.push(e.clone());
                self.scen.story.error = Some(e);
            }
        }
    }

    /// Makes an operation that touches several files one undo step: the steps run in order (a new
    /// file is created in its place); a refusal in one puts the others back.
    fn story_group(&mut self, label: &str, steps: Vec<st::Step>) {
        self.ws.begin_group();
        for step in steps {
            let result = match step {
                st::Step::Create(file, text) => self.ws.create_text_file(&file, &text, label),
                st::Step::Edit(_, edits) if edits.is_empty() => Ok(()),
                st::Step::Edit(file, edits) => self.ws.apply_doc(&file, label, &edits),
            };
            if let Err(e) = result {
                self.ws.cancel_group();
                self.log.push(e.clone());
                self.scen.story.error = Some(e);
                return;
            }
        }
        self.ws.end_group(label);
        self.scen.story.error = None;
        self.changed = true;
    }

    /// Writes the shown map's node positions (`.editor/layout.json`, under `story`); not an undo step.
    fn story_save_layout(&mut self, map: &str) {
        let saved = self.scen.story.saved.as_ref().filter(|(m, _)| m == map).map(|(_, p)| p.clone()).unwrap_or_default();
        let bytes = sg::layout_json(self.ws.read_editor_file(cg::LAYOUT).as_deref(), map, &saved);
        if let Err(e) = self.ws.write_editor_file(cg::LAYOUT, &bytes) {
            self.log.push(e);
        }
    }

    /// A snapshot's pending selection (`trigger` or `dialog`), once the package is there.
    fn story_resolve_pending(&mut self) {
        let Some(what) = self.scen.story.pending.clone() else { return };
        let Some(pkg) = self.pkg.clone() else { return };
        let map = self.maps.map.clone().unwrap_or_default();
        self.scen.story.pending = None;
        snapshot_select(&mut *self.scen, Some(&pkg), &map, &what);
    }

    /// Selects a node and shows its form in the Inspector.
    pub(super) fn story_goto(&mut self, sel: StorySel) {
        self.scen.story.sel = sel;
        self.scen.focus = Focus::Story;
    }

    /// The Story screen: the toolbar, then the map's flow graph.
    pub(super) fn story_screen(&mut self, ui: &mut Ui) {
        self.story_resolve_pending();
        let Some(pkg) = self.pkg.clone() else {
            ui.colored_label(ERROR_RED, tr("The package could not be read: see Problems."));
            return;
        };
        let Some(map) = self.maps.map.clone() else {
            ui.weak(tr("No map to show: open one on the Map tab."));
            return;
        };
        ui.horizontal(|ui| {
            if ui.button(tr("New trigger")).clicked() {
                self.story_new_trigger(&pkg, &map);
            }
            if ui.button(tr("New dialog")).clicked() {
                self.story_new_dialog(&pkg, &map);
            }
            if ui.button(tr("Auto layout")).clicked() {
                self.scen.story.saved = Some((map.clone(), BTreeMap::new()));
                self.story_save_layout(&map);
            }
            if ui.button(tr("Fit")).on_hover_text(tr("Fit the whole graph in the tab")).clicked() {
                self.scen.story.zoom = None;
            }
            if ui.button(tr("Variables")).clicked() {
                self.scen.want_tab = Some(Tab::Vars);
            }
        });
        ui.weak(tr("Drag from a choice's or a trigger's dot onto a dialog to wire it. Drag a node to move it."));
        if let Some(e) = self.scen.story.error.clone() {
            ui.colored_label(ERROR_RED, e);
        }
        let story = sg::build(&pkg, &map);
        if self.scen.story.saved.as_ref().is_none_or(|(m, _)| m != &map) {
            let saved = sg::read_layout(self.ws.read_editor_file(cg::LAYOUT).as_deref(), &map);
            self.scen.story.saved = Some((map.clone(), saved));
        }
        let saved = self.scen.story.saved.as_ref().map(|(_, p)| p.clone()).unwrap_or_default();
        let positions = sg::layout(&story, &saved);
        let (mut w, mut h) = (800.0_f32, 500.0_f32);
        for n in &story.nodes {
            if let Some(p) = positions.get(&n.id) {
                let s = sg::size(n);
                w = w.max(p[0] + s[0] + 60.0);
                h = h.max(p[1] + s[1] + 60.0);
            }
        }
        let z = match &self.scen.story.zoom {
            Some((m, z)) if *m == map => *z,
            _ => {
                let z = fit_zoom(w, h, ui.available_size());
                self.scen.story.zoom = Some((map.clone(), z));
                z
            }
        };
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            let (resp, painter) = ui.allocate_painter(Vec2::new(w * z, h * z), Sense::click());
            self.story_draw(ui, &resp, &painter, &story, &positions, &pkg, &map, z);
        });
    }

    /// Draws the graph at zoom `z` and takes its clicks, drags, zooms and links. Edges come first,
    /// so the opaque nodes are drawn over them.
    #[allow(clippy::too_many_arguments)]
    fn story_draw(&mut self, ui: &mut Ui, resp: &egui::Response, painter: &egui::Painter, story: &Story, positions: &BTreeMap<String, [f32; 2]>, pkg: &Package, map: &str, z: f32) {
        if resp.hovered() {
            let d = ui.input(|i| i.zoom_delta());
            if (d - 1.0).abs() > f32::EPSILON {
                self.scen.story.zoom = Some((map.to_string(), (z * d).clamp(ZOOM_MIN, ZOOM_MAX)));
            }
        }
        let origin = resp.rect.min;
        let at = |p: [f32; 2]| Pos2::new(origin.x + p[0] * z, origin.y + p[1] * z);
        let node_rect = |id: &str| -> Option<Rect> {
            let n = story.node(id)?;
            let p = positions.get(id)?;
            Some(Rect::from_min_size(at(*p), Vec2::from(sg::size(n)) * z))
        };
        let row_y = |r: Rect, i: usize| r.top() + (ROW_Y0 + ROW_STEP * i as f32) * z;
        let port_at = |id: &str, port: Option<&str>| -> Option<Pos2> {
            let r = node_rect(id)?;
            match port {
                Some(c) => {
                    let n = story.node(id)?;
                    let i = n.choices.iter().position(|x| x == c)?;
                    Some(Pos2::new(r.right(), row_y(r, i)))
                }
                None => Some(Pos2::new(r.right(), r.center().y)),
            }
        };
        let font = |size: f32| FontId::proportional(size * z);
        let off = |x: f32, y: f32| Vec2::new(x, y) * z;
        // Edges, under the nodes. A label sits on the edge at 40–60%, staggered so that labels of
        // neighbouring edges do not overlap.
        for (i, e) in story.edges.iter().enumerate() {
            let (Some(from), Some(r)) = (port_at(&e.from, e.port.as_deref()), node_rect(&e.to)) else { continue };
            let to = Pos2::new(r.left(), r.center().y);
            let stroke = Stroke::new(1.4, GREY);
            painter.line_segment([from, to], stroke);
            arrow(painter, from, to, stroke, z);
            let mid = super::scenarios::edge_label_at(from, to, i);
            let text = e.label.clone();
            let w = (text.chars().count() as f32 * 6.4 + 10.0) * z;
            painter.rect_filled(Rect::from_center_size(mid, Vec2::new(w, 15.0 * z)), 3.0, LABEL_FILL);
            painter.text(mid, Align2::CENTER_CENTER, text, font(11.0), Color32::from_gray(0xd0));
        }
        let selected_id = match &self.scen.story.sel {
            StorySel::Trigger(t) => Some(sg::trigger_id(t)),
            StorySel::Dialog(d) => Some(sg::dialog_id(d)),
            StorySel::Var(v) => Some(sg::var_id(v)),
            StorySel::None => None,
        };
        let mut ports: Vec<(Port, Pos2)> = Vec::new();
        for n in &story.nodes {
            let Some(r) = node_rect(&n.id) else { continue };
            let selected = selected_id.as_deref() == Some(n.id.as_str());
            let border = if selected { ACCENT } else { GREY };
            let stroke_w = if selected { 2.0 } else { 1.0 };
            match n.kind {
                Kind::Dialog => {
                    painter.rect_filled(r, egui::CornerRadius::same(4), DIALOG_FILL);
                    painter.rect_stroke(r, egui::CornerRadius::same(4), Stroke::new(stroke_w, border), egui::StrokeKind::Inside);
                    painter.text(r.left_top() + off(8.0, 7.0), Align2::LEFT_TOP, format!("{} {}", tr("Dialog"), n.name), font(13.0), Color32::WHITE);
                    for (i, c) in n.choices.iter().enumerate() {
                        let y = row_y(r, i);
                        painter.text(Pos2::new(r.left() + 10.0 * z, y), Align2::LEFT_CENTER, c.as_str(), font(11.0), Color32::from_gray(0xc8));
                        // Each choice's own output port, at the right edge of its row.
                        let port = Pos2::new(r.right(), y);
                        painter.circle_filled(port, 4.5 * z, ACCENT);
                        ports.push((Port::Choice(n.name.clone(), c.clone()), port));
                    }
                }
                Kind::Trigger => {
                    painter.rect_filled(r, egui::CornerRadius::same(4), TRIGGER_FILL);
                    painter.rect_stroke(r, egui::CornerRadius::same(4), Stroke::new(stroke_w, border), egui::StrokeKind::Inside);
                    painter.text(r.left_top() + off(8.0, 7.0), Align2::LEFT_TOP, format!("{} {}", tr("Trigger"), n.name), font(13.0), Color32::WHITE);
                    painter.text(r.left_top() + off(8.0, 30.0), Align2::LEFT_TOP, n.detail.as_str(), font(11.0), GREY);
                    let port = Pos2::new(r.right(), r.center().y);
                    painter.circle_filled(port, 4.5 * z, ACCENT);
                    ports.push((Port::Trigger(n.name.clone()), port));
                }
                Kind::Var => {
                    painter.rect_filled(r, egui::CornerRadius::same(12), VAR_FILL);
                    painter.rect_stroke(r, egui::CornerRadius::same(12), Stroke::new(stroke_w, border), egui::StrokeKind::Inside);
                    painter.text(r.center(), Align2::CENTER_CENTER, format!("{} · {}", n.name, n.detail), font(11.5), Color32::WHITE);
                }
            }
            let body = ui.interact(r.shrink(4.0 * z), ui.id().with(("story-node", n.id.as_str())), Sense::click_and_drag());
            if body.clicked() {
                let sel = match n.kind {
                    Kind::Dialog => StorySel::Dialog(n.name.clone()),
                    Kind::Trigger => StorySel::Trigger(n.name.clone()),
                    Kind::Var => StorySel::Var(n.name.clone()),
                };
                self.story_goto(sel);
            }
            if body.dragged() {
                let d = body.drag_delta() / z;
                let p = positions.get(&n.id).copied().unwrap_or([0.0, 0.0]);
                let saved = self.scen.story.saved.get_or_insert_with(|| (map.to_string(), BTreeMap::new()));
                saved.1.insert(n.id.clone(), [p[0] + d.x, p[1] + d.y]);
            }
            if body.drag_stopped() {
                self.story_save_layout(map);
            }
        }
        // The ports: a dot on a dialog's choice or a trigger's output starts a link when dragged.
        for (port, at_pos) in &ports {
            let hit = ui.interact(Rect::from_center_size(*at_pos, Vec2::splat(14.0)), ui.id().with(("story-port", format!("{port:?}"))), Sense::drag());
            if hit.drag_started() {
                self.scen.story.link = Some(port.clone());
            }
        }
        // A link being dragged: the rubber band, then the wiring on release over a dialog.
        if let (Some(port), Some(pos)) = (self.scen.story.link.clone(), ui.input(|i| i.pointer.hover_pos())) {
            let from = ports.iter().find(|(p, _)| *p == port).map(|(_, a)| *a);
            if let Some(from) = from {
                painter.line_segment([from, pos], Stroke::new(1.5, ACCENT));
            }
            if ui.input(|i| i.pointer.any_released()) {
                self.scen.story.link = None;
                let target = story.nodes.iter().find(|n| n.kind == Kind::Dialog && node_rect(&n.id).is_some_and(|r| r.contains(pos))).map(|n| n.name.clone());
                if let Some(target) = target {
                    self.story_wire(pkg, map, port, &target);
                }
            }
        }
    }

    /// Wires a port to a dialog (`studio::story`'s steps), as one undo step.
    fn story_wire(&mut self, pkg: &Package, map: &str, port: Port, target: &str) {
        let text_of = |f: &str| self.ws.text_of(f).ok();
        let steps = match port {
            Port::Choice(d, c) => st::wire_choice(pkg, &text_of, map, &d, &c, target),
            Port::Trigger(t) => st::wire_trigger(pkg, &text_of, &t, target),
        };
        match steps {
            Ok(steps) => self.story_group("wire to a dialog", steps),
            Err(e) => self.story_refuse(e),
        }
    }

    fn story_refuse(&mut self, e: String) {
        self.log.push(e.clone());
        self.scen.story.error = Some(e);
    }

    /// A new trigger on the shown map: a `game_start` with no actions; selected at once.
    fn story_new_trigger(&mut self, pkg: &Package, map: &str) {
        let (id, steps) = st::new_trigger_steps(pkg, map);
        self.story_group("add a trigger", steps);
        if self.scen.story.error.is_none() {
            self.story_goto(StorySel::Trigger(id));
        }
    }

    /// A new dialog on the shown map, its keys in every language; selected at once.
    fn story_new_dialog(&mut self, pkg: &Package, map: &str) {
        let (id, steps) = st::new_dialog_steps(pkg, map);
        self.story_group("add a dialog", steps);
        if self.scen.story.error.is_none() {
            self.story_goto(StorySel::Dialog(id));
        }
    }

    /// The Inspector's story form: the selected trigger, dialog or variable.
    pub(super) fn story_inspector(&mut self, ui: &mut Ui) {
        self.story_resolve_pending();
        match self.scen.story.sel.clone() {
            StorySel::None => {
                ui.weak(tr("Pick a trigger or a dialog on the Story screen."));
            }
            StorySel::Trigger(id) => self.trigger_form(ui, &id),
            StorySel::Dialog(id) => self.dialog_form(ui, &id),
            StorySel::Var(name) => self.var_summary(ui, &name),
        }
        if let Some(e) = self.scen.story.error.clone() {
            ui.colored_label(ERROR_RED, e);
        }
    }

    fn trigger_form(&mut self, ui: &mut Ui, id: &str) {
        let Some(pkg) = self.pkg.clone() else { return };
        let Some(t) = pkg.triggers.iter().find(|t| t.value.id == id) else {
            self.scen.story.sel = StorySel::None;
            return;
        };
        let (file, line) = (t.file.clone(), t.line);
        let text = match self.ws.text_of(&file) {
            Ok(x) => x,
            Err(e) => {
                ui.colored_label(ERROR_RED, e);
                return;
            }
        };
        let Some(draft) = st::trigger_draft(&text, id) else {
            super::inspector::head(ui, ic::LIGHTNING, &format!("{}: {id}", tr("Trigger")), &format!("{file}:{line}"));
            ui.colored_label(ERROR_RED, tr("This trigger does not parse: see Problems."));
            return;
        };
        let scope = draft.map.clone().or_else(|| self.maps.map.clone()).unwrap_or_default();
        let opts = Options::of(&pkg, &scope);
        let texts = texts_shown(&pkg);
        let cx = Cx { opts: &opts, texts: &texts };
        let q = super::inspector::head(ui, ic::LIGHTNING, &format!("{}: {id}", tr("Trigger")), &format!("{file}:{line}"));
        let mut pending: Vec<(&str, Vec<DocEdit>)> = Vec::new();
        kit::section(ui, "trigger", "main", tr("Main"), true, &q, |ui| {
            if let Some(new) = kit::row(ui, &q, tr("Name"), "trigger.id", None, |ui| forms::text_field(ui, "st-tid", id, 170.0)).value.flatten() {
                let taken = pkg.triggers.iter().any(|t| t.value.id == new);
                if !st::is_name(&new) {
                    self.scen.story.error = Some(trf!("{name} is not a name: lowercase letters, digits and _", name = new.as_str()));
                } else if taken {
                    self.scen.story.error = Some(trf!("a trigger {name} already exists", name = new.as_str()));
                } else {
                    pending.push(("rename trigger", vec![st::set_trigger_key(id, "id", Some(Value::from(new.as_str())))]));
                    self.scen.story.sel = StorySel::Trigger(new);
                }
            }
            let mut maps: Vec<String> = vec![tr("(every map)").to_string()];
            let mut ids: Vec<String> = pkg.maps.keys().cloned().collect();
            ids.sort();
            maps.extend(ids);
            let refs: Vec<&str> = maps.iter().map(String::as_str).collect();
            let current = draft.map.clone().unwrap_or_else(|| tr("(every map)").to_string());
            if let Some(v) = kit::row(ui, &q, tr("Map"), "trigger.map", None, |ui| forms::choice(ui, "st-tmap", &current, &refs)).value.flatten() {
                let value = if v == tr("(every map)") { None } else { Some(Value::from(v.as_str())) };
                pending.push(("set map", vec![st::set_trigger_key(id, "map", value)]));
            }
            let once = kit::row(ui, &q, "once", "trigger.once", None, |ui| {
                let mut once = draft.once.unwrap_or(true);
                if ui.checkbox(&mut once, "").on_hover_text(tr("once: fires one time, not on every chance")).changed() {
                    Some(once)
                } else {
                    None
                }
            })
            .value
            .flatten();
            if let Some(once) = once {
                pending.push(("set once", vec![st::set_trigger_key(id, "once", Some(Value::from(once)))]));
            }
            let chance = kit::row(ui, &q, "chance", "trigger.chance", None, |ui| {
                let mut on = draft.chance.is_some();
                let mut value = None;
                if ui.checkbox(&mut on, "").on_hover_text(tr("chance: a percent, rolled after the conditions hold")).changed() {
                    value = Some(if on { Some(Value::from(50_i64)) } else { None });
                }
                if let Some(c) = draft.chance {
                    if let Some(n) = forms::int_field(ui, "st-tchance", c, 0, 100) {
                        value = Some(Some(Value::from(n)));
                    }
                }
                value
            })
            .value
            .flatten();
            if let Some(value) = chance {
                pending.push(("set chance", vec![st::set_trigger_key(id, "chance", value)]));
            }
        });
        let mut when_row = None;
        kit::section(ui, "trigger", "when", tr("When"), true, &q, |ui| {
            kit::block(ui, &q, tr("When"), "trigger.when", |ui| {
                ui.group(|ui| match sf::row_editor(ui, "st-when", Section::Event, &draft.when, &cx, false, false) {
                    Ok(RowAct::Changed(r)) => when_row = Some(r),
                    Ok(_) => {}
                    Err(e) => self.scen.story.error = Some(e),
                });
            });
        });
        if let Some(r) = when_row {
            pending.push(("set when", vec![st::set_when(id, &r)]));
        }
        let mut conds = None;
        kit::section(ui, "trigger", "if", tr("If (all must hold)"), true, &q, |ui| {
            kit::block(ui, &q, tr("If (all must hold)"), "trigger.if", |ui| {
                ui.group(|ui| match sf::list_editor(ui, "st-if", Section::Cond, &draft.cond, &cx) {
                    Ok(rows) => conds = rows,
                    Err(e) => self.scen.story.error = Some(e),
                });
            });
        });
        if let Some(rows) = conds {
            pending.push(("set conditions", st::set_trigger_list(id, "if", &rows, !draft.cond.is_empty())));
        }
        let mut actions = None;
        kit::section(ui, "trigger", "then", tr("Then (in order)"), true, &q, |ui| {
            kit::block(ui, &q, tr("Then (in order)"), "trigger.then", |ui| {
                ui.group(|ui| match sf::list_editor(ui, "st-then", Section::Action, &draft.then, &cx) {
                    Ok(rows) => actions = rows,
                    Err(e) => self.scen.story.error = Some(e),
                });
            });
        });
        if let Some(rows) = actions {
            pending.push(("set actions", st::set_trigger_list(id, "then", &rows, !draft.then.is_empty())));
        }
        ui.add_space(6.0);
        if ui.button(tr("Delete trigger")).on_hover_text(tr("Removes the trigger's table; its comments above stay")).clicked() {
            pending.push(("delete trigger", vec![st::drop_trigger(id)]));
            self.scen.story.sel = StorySel::None;
        }
        for (label, edits) in pending {
            self.story_apply(label, &file, &edits);
        }
    }

    fn dialog_form(&mut self, ui: &mut Ui, id: &str) {
        let Some(pkg) = self.pkg.clone() else { return };
        let Some(d) = pkg.dialogs.iter().find(|x| x.value.id == id) else {
            self.scen.story.sel = StorySel::None;
            return;
        };
        let (file, line) = (d.file.clone(), d.line);
        let text = match self.ws.text_of(&file) {
            Ok(x) => x,
            Err(e) => {
                ui.colored_label(ERROR_RED, e);
                return;
            }
        };
        let Some(draft) = st::dialog_draft(&text, id) else {
            super::inspector::head(ui, ic::CHAT_CIRCLE_TEXT, &format!("{}: {id}", tr("Dialog")), &format!("{file}:{line}"));
            ui.colored_label(ERROR_RED, tr("This dialog does not parse: see Problems."));
            return;
        };
        let scope = self.maps.map.clone().unwrap_or_default();
        let opts = Options::of(&pkg, &scope);
        let shown = texts_shown(&pkg);
        let by_lang = pkg.texts.clone();
        let suggestions: Vec<String> = shown.keys().cloned().collect();
        let cx = Cx { opts: &opts, texts: &shown };
        let q = super::inspector::head(ui, ic::CHAT_CIRCLE_TEXT, &format!("{}: {id}", tr("Dialog")), &format!("{file}:{line}"));
        let mut pending: Vec<(&str, String, Vec<DocEdit>)> = Vec::new();
        kit::section(ui, "dialog", "main", tr("Main"), true, &q, |ui| {
            // Text keys: the key (picked or typed) and each language's text.
            let title = kit::row(ui, &q, tr("Title"), "dialog.title", None, |ui| {
                let key = forms::text_key(ui, "st-dtitle", &draft.title, &suggestions, &shown);
                let cells = sf::text_cells(ui, "st-dtitle", &draft.title, &by_lang);
                (key, cells)
            })
            .value;
            if let Some((key, cells)) = title {
                if let Some(key) = key {
                    pending.push(("set title key", file.clone(), vec![st::set_dialog_key(id, "title", Some(Value::from(key.as_str())))]));
                }
                if let Some((lang, v)) = cells {
                    let (f, e) = tx::set_cell(&lang, &draft.title, &v);
                    pending.push(("set title", f, vec![e]));
                }
            }
            let text_row = kit::row(ui, &q, tr("Text"), "dialog.text", None, |ui| {
                let key = forms::text_key(ui, "st-dtext", &draft.text, &suggestions, &shown);
                let cells = sf::text_cells(ui, "st-dtext", &draft.text, &by_lang);
                (key, cells)
            })
            .value;
            if let Some((key, cells)) = text_row {
                if let Some(key) = key {
                    pending.push(("set text key", file.clone(), vec![st::set_dialog_key(id, "text", Some(Value::from(key.as_str())))]));
                }
                if let Some((lang, v)) = cells {
                    let (f, e) = tx::set_cell(&lang, &draft.text, &v);
                    pending.push(("set text", f, vec![e]));
                }
            }
            let speaker = kit::row(ui, &q, tr("Speaker"), "dialog.speaker", None, |ui| {
                let mut units: Vec<String> = vec![tr("(none)").to_string()];
                units.extend(opts.units.iter().cloned());
                let refs: Vec<&str> = units.iter().map(String::as_str).collect();
                let current = draft.speaker.clone().unwrap_or_else(|| tr("(none)").to_string());
                forms::choice(ui, "st-dspeaker", &current, &refs)
            })
            .value
            .flatten();
            if let Some(v) = speaker {
                let value = if v == tr("(none)") { None } else { Some(Value::from(v.as_str())) };
                pending.push(("set speaker", file.clone(), vec![st::set_dialog_key(id, "speaker", value)]));
            }
        });
        let count = draft.choices.len();
        let mut moved: Option<(usize, usize)> = None;
        let mut removed: Option<String> = None;
        kit::section(ui, "dialog", "choices", tr("Choices"), true, &q, |ui| {
            kit::note(ui, tr("A choice's own actions run when it is picked; a trigger on dialog_choice can do the rest."));
            if let Some(tool) = kit::list_tools(ui, count < pt::MAX_CHOICES, false, false) {
                let _ = tool;
                match st::add_choice_steps(&pkg, id) {
                    Ok((_, steps)) => self.story_group("add a choice", steps),
                    Err(e) => self.story_refuse(e),
                }
            }
            moved = kit::reorder(ui, "dialog-choices", count, None, |ui, i| {
                let c = &draft.choices[i];
                let salt = format!("st-c{i}");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        kit::grip(ui, "dialog-choices", i);
                        ui.strong(format!("{}. {}", i + 1, c.id));
                        if ui.add(kit::icon_button(ic::TRASH, 24.0)).on_hover_text(tr("Delete this choice")).clicked() {
                            removed = Some(c.id.clone());
                        }
                    });
                    let text = kit::row(ui, &q, tr("Text"), &format!("dialog.choice.{}.text", c.id), None, |ui| {
                        let key = forms::text_key(ui, &format!("{salt}-t"), &c.text, &suggestions, &shown);
                        let cells = sf::text_cells(ui, &format!("{salt}-t"), &c.text, &by_lang);
                        (key, cells)
                    })
                    .value;
                    if let Some((key, cells)) = text {
                        if let Some(key) = key {
                            pending.push(("set choice text key", file.clone(), vec![st::set_choice_key(id, &c.id, "text", Value::from(key.as_str()))]));
                        }
                        if let Some((lang, v)) = cells {
                            let (f, e) = tx::set_cell(&lang, &c.text, &v);
                            pending.push(("set choice text", f, vec![e]));
                        }
                    }
                    kit::note(ui, tr("If (shown only then)"));
                    let mut conds = None;
                    match sf::list_editor(ui, &format!("{salt}-if"), Section::Cond, &c.cond, &cx) {
                        Ok(rows) => conds = rows,
                        Err(e) => self.scen.story.error = Some(e),
                    }
                    if let Some(rows) = conds {
                        pending.push(("set choice conditions", file.clone(), st::set_choice_list(id, &c.id, "if", &rows, !c.cond.is_empty())));
                    }
                    kit::note(ui, tr("Then (in order)"));
                    let mut actions = None;
                    match sf::list_editor(ui, &format!("{salt}-then"), Section::Action, &c.then, &cx) {
                        Ok(rows) => actions = rows,
                        Err(e) => self.scen.story.error = Some(e),
                    }
                    if let Some(rows) = actions {
                        pending.push(("set choice actions", file.clone(), st::set_choice_list(id, &c.id, "then", &rows, !c.then.is_empty())));
                    }
                });
            });
        });
        if let Some((from, to)) = moved {
            pending.push(("move choice", file.clone(), vec![st::move_choice(id, from, to)]));
        }
        if let Some(cid) = removed {
            pending.push(("delete choice", file.clone(), vec![st::drop_choice(id, &cid)]));
        }
        for (label, f, edits) in pending {
            self.story_apply(label, &f, &edits);
        }
    }

    /// The Inspector's form for a variable: what sets it and what reads it, each a link.
    fn var_summary(&mut self, ui: &mut Ui, name: &str) {
        let Some(pkg) = self.pkg.clone() else { return };
        ui.strong(format!("{}: {name}", tr("Variable")));
        let uses = vars::variable_uses(&pkg, name);
        if let Some(v) = pkg.vars.iter().find(|v| v.value.name == name) {
            ui.weak(format!("{}:{}", v.file, v.line));
        }
        ui.separator();
        let mut goto = None;
        for u in uses.iter().filter(|u| u.role != Role::Declared) {
            let verb = if u.role == Role::Write { tr("sets") } else { tr("reads") };
            ui.horizontal_wrapped(|ui| {
                ui.weak(verb);
                if ui.link(u.what.as_str()).clicked() {
                    goto = Some(holder_sel(&u.holder));
                }
                ui.weak(format!("{}:{}", u.file, u.line));
            });
        }
        if let Some(sel) = goto.flatten() {
            self.story_goto(sel);
        }
    }

}

/// Picks the trigger or dialog a snapshot option names: `trigger` the first trigger that applies to
/// the map, `dialog` the dialog of the map's story with the most choices. The Inspector shows it.
pub(super) fn snapshot_select(scen: &mut super::scenarios::ScenarioUi, pkg: Option<&Package>, map: &str, what: &str) {
    let Some(pkg) = pkg else { return };
    match what {
        "trigger" => {
            if let Some(t) = pkg.triggers.iter().find(|t| t.value.map.as_deref().is_none_or(|m| m == map)) {
                scen.story.sel = StorySel::Trigger(t.value.id.clone());
            }
        }
        _ => {
            let story = sg::build(pkg, map);
            if let Some(n) = story.nodes.iter().filter(|n| n.kind == Kind::Dialog).max_by_key(|n| n.choices.len()) {
                scen.story.sel = StorySel::Dialog(n.name.clone());
            }
        }
    }
    scen.focus = Focus::Story;
}

/// A variable's use holder as the selection it opens (`None` for the declaration).
pub(super) fn holder_sel(h: &Holder) -> Option<StorySel> {
    match h {
        Holder::Declared => None,
        Holder::Trigger(id) => Some(StorySel::Trigger(id.clone())),
        Holder::Choice(d, _) => Some(StorySel::Dialog(d.clone())),
    }
}

/// An arrowhead at `to`, along the edge, at zoom `z`.
fn arrow(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke, z: f32) {
    let d = to - from;
    if d.length() < 1.0 {
        return;
    }
    let u = d.normalized();
    let n = Vec2::new(-u.y, u.x);
    let tip = to;
    painter.line_segment([tip, tip - u * 9.0 * z + n * 4.5 * z], stroke);
    painter.line_segment([tip, tip - u * 9.0 * z - n * 4.5 * z], stroke);
}

/// The zoom at which a graph of `w` by `h` points fits `avail` (at most 100%, at least `ZOOM_MIN`).
fn fit_zoom(w: f32, h: f32, avail: Vec2) -> f32 {
    ((avail.x - 12.0) / w).min((avail.y - 12.0) / h).clamp(ZOOM_MIN, 1.0)
}
