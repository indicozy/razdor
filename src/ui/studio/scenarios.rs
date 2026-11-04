//! The scenario forms (P3): the package settings and the map header in the Inspector, the campaign
//! graph on the Campaign screen (with its selected node or edge in the Inspector), the named
//! characters dock and the Texts screen. Every change is one undo step through `Workspace::apply_doc`
//! (or `set_texts` for a rename across files); the edits are computed by `studio::scenario`,
//! `studio::campaign_graph`, `studio::characters` and `studio::texts`.

use std::collections::BTreeMap;

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use egui_macroquad::egui;
use razdor::i18n::tr;
use razdor::pack::map::Victory;
use razdor::pack::triggers as pt;
use razdor::studio::campaign_graph::{self as cg, Graph};
use razdor::studio::characters::{self as chars, SiteKind};
use razdor::studio::doc::DocEdit;
use razdor::studio::scenario::{self as sc, VictoryDraft};
use razdor::studio::texts::{self as tx, Status};
use razdor::trf;
use toml_edit::Value;

use super::forms;
use super::inspector::spec_now;
use super::{Jump, Viewer, ERROR_RED, WARNING_AMBER};

/// The colours of the graph.
const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x8e, 0xdc);
const NODE_FILL: Color32 = Color32::from_rgb(0x2b, 0x30, 0x3d);
const NODE_START: Color32 = Color32::from_rgb(0x2f, 0x5d, 0x3a);
const GREY: Color32 = Color32::from_rgb(0x8a, 0x8f, 0x9c);

/// Where an edge's label sits: on the edge at 40–60% of its length, staggered by the edge's index
/// (the three positions and two offsets across the line) so that neighbouring labels do not overlap.
pub(super) fn edge_label_at(from: Pos2, to: Pos2, index: usize) -> Pos2 {
    let t = [0.5, 0.42, 0.58][index % 3];
    let d = to - from;
    let normal = if d.length() > 0.0 { Vec2::new(-d.y, d.x).normalized() } else { Vec2::ZERO };
    let offset = [0.0, -12.0, 12.0][(index / 3) % 3];
    from + d * t + normal * offset
}

/// The node's size on the canvas, in points.
const NODE: Vec2 = Vec2::new(176.0, 58.0);

/// Which thing the Campaign screen has selected (the Inspector shows it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CampSel {
    /// The graph's background: the carry flags and the start.
    #[default]
    Graph,
    Node(String),
    /// An edge: its map and its position in the map's list.
    Edge(String, usize),
}

/// What the Inspector shows: the selected table (the default), or one of the scenario forms.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Table,
    Package,
    Header,
    Campaign,
    /// A trigger or a dialog of the story (`story::StoryUi`'s selection).
    Story,
    /// A unit of the package (the Units screen's selection, `ScenarioUi::unit`).
    Units,
    /// An item of the package (the Items screen's selection, `ScenarioUi::item`).
    Items,
}

/// The state of the scenario screens, kept across frames.
#[derive(Default)]
pub struct ScenarioUi {
    pub focus: Focus,
    pub camp: CampSel,
    /// The node positions saved in `.editor/layout.json`, read once.
    saved: Option<BTreeMap<String, [f32; 2]>>,
    /// The node being linked from its output port (the new edge's source).
    link_from: Option<String>,
    /// The conditions text of the selected edge (one inline table per line), and the edge it is for.
    edge_if: String,
    edge_if_for: Option<(String, usize)>,
    /// The last rejected input's message, shown under the form that rejected it.
    pub error: Option<String>,
    pub filter: String,
    pub new_lang: String,
    pub rename_to: String,
    pub character: Option<String>,
    pub new_map: String,
    /// The palette's new kind (the colour is the field next to it).
    pub new_kind: String,
    /// The story screens' state: the selection and the wiring in progress.
    pub story: super::story::StoryUi,
    /// A main screen or dock the story asks to show after the dock is drawn (a usage's click).
    pub want_tab: Option<super::Tab>,
    /// The Units and Items screens (P5): the selected record of each, the list's filter, the new
    /// record's id and base, the rename's new id, and the last refusal shown under the form (`error`).
    pub unit: Option<String>,
    pub item: Option<String>,
    pub record_filter: String,
    pub new_id: String,
    pub new_base: String,
    pub record_rename: String,
    /// The price's steps are open (the snapshot option `units` opens them).
    pub price_open: bool,
    /// The game's prices of the package, for the package they were made of (`ui::studio::records`).
    pub priced: Option<(std::sync::Arc<razdor::pack::load::Package>, razdor::studio::records::Priced)>,
}

impl ScenarioUi {
}

/// The texts of the language the studio shows (any language when that one is missing).
fn texts_of(pkg: Option<&razdor::pack::load::Package>) -> BTreeMap<String, String> {
    let Some(p) = pkg else { return BTreeMap::new() };
    p.texts.get(razdor::pack::open::lang()).or_else(|| p.texts.values().next()).cloned().unwrap_or_default()
}

impl Viewer<'_> {
    /// Applies `edits` to `file` as one undo step; a rejected edit is logged and shown in the form.
    fn scen_apply(&mut self, scen_error: &mut Option<String>, label: &str, file: &str, edits: &[DocEdit]) {
        match self.ws.apply_doc(file, label, edits) {
            Ok(()) => {
                *scen_error = None;
                self.changed = true;
            }
            Err(e) => {
                *scen_error = Some(e.clone());
                self.log.push(e);
            }
        }
    }

    /// The package settings form (the Inspector's Package focus).
    pub(super) fn package_form(&mut self, ui: &mut Ui) {
        ui.strong(tr("Package settings"));
        ui.weak(tr("package.toml: each change is one undo step (Ctrl+Z)."));
        ui.separator();
        let Some(pkg) = self.pkg.clone() else {
            ui.colored_label(ERROR_RED, tr("The package could not be read: see Problems."));
            return;
        };
        let Some(m) = pkg.manifest.clone() else {
            ui.colored_label(ERROR_RED, tr("package.toml does not parse: see Problems."));
            return;
        };
        let texts = texts_of(Some(pkg.as_ref()));
        let keys: Vec<String> = texts.keys().cloned().collect();
        let file = "package.toml";
        let mut edits: Vec<(&str, Option<Value>, bool)> = Vec::new();
        egui::Grid::new(ui.id().with("pkg-grid")).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Id"));
            ui.monospace(m.id.as_str());
            ui.end_row();
            ui.label(tr("Version"));
            if let Some(v) = forms::text_field(ui, "pkg-version", &m.version, 120.0) {
                edits.push(("version", Some(Value::from(v)), false));
            }
            ui.end_row();
            ui.label(tr("Title"));
            if let Some(v) = forms::text_key(ui, "pkg-title", &m.title, &keys, &texts) {
                edits.push(("title", Some(Value::from(v)), false));
            }
            ui.end_row();
            ui.label(tr("Description"));
            ui.vertical(|ui| {
                let mut on = m.description.is_some();
                if let Some(on2) = forms::optional(ui, tr("Has a description"), on) {
                    on = on2;
                    let key = format!("{}.description", m.id.replace('-', "_"));
                    edits.push(("description", if on { Some(Value::from(key)) } else { None }, false));
                }
                if let Some(d) = &m.description {
                    if let Some(v) = forms::text_key(ui, "pkg-description", d, &keys, &texts) {
                        edits.push(("description", Some(Value::from(v)), false));
                    }
                }
            });
            ui.end_row();
            ui.label(tr("Author"));
            if let Some(v) = forms::text_field(ui, "pkg-author", &m.author, 180.0) {
                edits.push(("author", Some(Value::from(v)), false));
            }
            ui.end_row();
            ui.label(tr("Licence"));
            if let Some(v) = forms::choice(ui, "pkg-license", &m.license, &[sc::LICENCE]) {
                edits.push(("license", Some(Value::from(v)), false));
            }
            ui.end_row();
            ui.label(tr("Base"));
            if let Some(v) = forms::choice(ui, "pkg-base", &m.base, &razdor::pack::manifest::BASES) {
                edits.push(("base", Some(Value::from(v)), false));
            }
            ui.end_row();
        });
        ui.weak(trf!("Requires: {list}", list = if m.requires.is_empty() { "-".to_string() } else { m.requires.join(", ") }));
        ui.weak(tr("The id is read only: it names the package in saves."));
        for (key, value, _) in edits {
            match sc::package_field(key, value) {
                Ok(edit) => {
                    let mut err = None;
                    self.scen_apply(&mut err, &format!("set {key}"), file, &[edit]);
                    self.scen.error = err;
                }
                Err(e) => self.scen.error = Some(e),
            }
        }
        self.show_scen_error(ui);
    }

    fn show_scen_error(&mut self, ui: &mut Ui) {
        if let Some(e) = self.scen.error.clone() {
            ui.colored_label(ERROR_RED, e);
        }
    }

    /// The map header form (the Inspector's Header focus): title, description, start date, the
    /// relations grid, the terrain palette, and the victory.
    pub(super) fn header_form(&mut self, ui: &mut Ui) {
        ui.strong(tr("Map settings"));
        ui.weak(tr("The map's header and victory: each change is one undo step (Ctrl+Z)."));
        ui.separator();
        let Some(map) = self.maps.map.clone() else {
            ui.weak(tr("No map to show."));
            return;
        };
        let file = format!("maps/{map}/map.toml");
        let Some(spec) = spec_now(self.ws, self.pkg.as_deref(), &map) else {
            ui.colored_label(ERROR_RED, tr("The map file could not be read."));
            return;
        };
        let texts = texts_of(self.pkg.as_deref());
        let keys: Vec<String> = texts.keys().cloned().collect();
        let mut out: Vec<DocEdit> = Vec::new();
        let mut label = "set map header".to_string();
        egui::Grid::new(ui.id().with("hdr-grid")).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Title"));
            if let Some(v) = forms::text_key(ui, "hdr-title", &spec.title, &keys, &texts) {
                if let Ok(e) = sc::map_text_key("title", Some(&v)) {
                    out.push(e);
                    label = "set title".into();
                }
            }
            ui.end_row();
            ui.label(tr("Description"));
            ui.vertical(|ui| {
                let on = spec.description.is_some();
                if let Some(on2) = forms::optional(ui, tr("Has a description"), on) {
                    if on2 {
                        out.push(sc::map_text_key("description", Some(&format!("{map}.description"))).unwrap_or_else(|_| DocEdit::Remove { path: vec![], key: "description".into() }));
                    } else if let Ok(e) = sc::map_text_key("description", None) {
                        out.push(e);
                    }
                }
                if let Some(d) = &spec.description {
                    if let Some(v) = forms::text_key(ui, "hdr-description", d, &keys, &texts) {
                        if let Ok(e) = sc::map_text_key("description", Some(&v)) {
                            out.push(e);
                        }
                    }
                }
            });
            ui.end_row();
            ui.label(tr("Start date"));
            if let Some(v) = forms::text_field(ui, "hdr-date", &spec.start_date, 120.0) {
                match sc::start_date(&v) {
                    Ok(e) => {
                        out.push(e);
                        label = "set start date".into();
                    }
                    Err(e) => self.scen.error = Some(e),
                }
            }
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.strong(tr("Relations"));
        ui.weak(tr("Rows are the side, columns the side it is towards: player, ally, neighbour, enemy (−3 to 3)."));
        let names = [tr("Player"), tr("Ally"), tr("Neighbour"), tr("Enemy")];
        let mut grid = spec.relations;
        let mut changed = false;
        egui::Grid::new(ui.id().with("rel-grid")).num_columns(5).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("");
            for n in names {
                ui.weak(n);
            }
            ui.end_row();
            for (r, row) in grid.iter_mut().enumerate() {
                ui.weak(names[r]);
                for (c, cell) in row.iter_mut().enumerate() {
                    if let Some(v) = forms::int_field(ui, &format!("rel-{r}-{c}"), i64::from(*cell), -3, 3) {
                        *cell = v as i8;
                        changed = true;
                    }
                }
                ui.end_row();
            }
        });
        if changed {
            match sc::relations(grid) {
                Ok(e) => {
                    out.push(e);
                    label = "set relations".into();
                }
                Err(e) => self.scen.error = Some(e),
            }
        }
        ui.add_space(4.0);
        ui.strong(tr("Terrain palette"));
        ui.weak(tr("The colours that differ from the default palette."));
        let mut palette_out: Vec<DocEdit> = Vec::new();
        for (kind, colour) in &spec.terrain_palette {
            ui.horizontal(|ui| {
                ui.monospace(kind.as_str());
                if let Some(v) = forms::text_field(ui, &format!("pal-{kind}"), colour, 80.0) {
                    match sc::palette(kind, Some(&v)) {
                        Ok(e) => palette_out.push(e),
                        Err(e) => self.scen.error = Some(e),
                    }
                }
                if ui.small_button(tr("Remove")).clicked() {
                    if let Ok(e) = sc::palette(kind, None) {
                        palette_out.push(e);
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            let kinds: Vec<&str> = razdor::pack::terrain::DEFAULT_PALETTE.iter().map(|(k, _)| *k).filter(|k| !spec.terrain_palette.contains_key(*k)).collect();
            if self.scen.new_kind.is_empty() {
                self.scen.new_kind = kinds.first().map_or(String::new(), |k| (*k).to_string());
            }
            if let Some(p) = forms::choice(ui, "pal-new-kind", &self.scen.new_kind, &kinds) {
                self.scen.new_kind = p;
            }
            let kind = self.scen.new_kind.clone();
            if let Some(v) = forms::text_field(ui, "pal-new-colour", "", 80.0) {
                if !kind.is_empty() {
                    match sc::palette(&kind, Some(&v)) {
                        Ok(e) => palette_out.push(e),
                        Err(e) => self.scen.error = Some(e),
                    }
                }
            }
        });
        out.extend(palette_out);
        ui.add_space(4.0);
        ui.strong(tr("Victory"));
        let draft_now = victory_draft(spec.victory.get_ref());
        let mut draft = draft_now.clone();
        ui.horizontal(|ui| {
            ui.label(tr("Kind"));
            if let Some(k) = forms::choice(ui, "vic-kind", &draft.kind, &sc::VICTORY_KINDS) {
                draft.kind = k;
            }
        });
        match draft.kind.as_str() {
            "defeat_army" => {
                let ids: Vec<&str> = spec.army.iter().map(|a| a.get_ref().id.as_str()).collect();
                let current = draft.army.clone().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(tr("Army"));
                    if let Some(a) = forms::choice(ui, "vic-army", &current, &ids) {
                        draft.army = Some(a);
                    }
                });
            }
            "capture" => {
                let ids: Vec<&str> = spec.building.iter().map(|b| b.get_ref().id.as_str()).collect();
                let current = draft.building.clone().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(tr("Building"));
                    if let Some(b) = forms::choice(ui, "vic-building", &current, &ids) {
                        draft.building = Some(b);
                    }
                });
            }
            "reach" => {
                let [x, y] = draft.cell.unwrap_or([0, 0]);
                ui.horizontal(|ui| {
                    ui.label(tr("Cell"));
                    let nx = forms::int_field(ui, "vic-x", i64::from(x), 0, i64::from(map_size(self).0.saturating_sub(1)));
                    let ny = forms::int_field(ui, "vic-y", i64::from(y), 0, i64::from(map_size(self).1.saturating_sub(1)));
                    if nx.is_some() || ny.is_some() {
                        draft.cell = Some([nx.map_or(x, |v| v as u32), ny.map_or(y, |v| v as u32)]);
                    }
                });
            }
            _ => {}
        }
        let msg = draft.message.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(tr("Message"));
            if let Some(v) = forms::text_key(ui, "vic-message", &msg, &keys, &texts) {
                draft.message = Some(v);
            }
        });
        if draft != draft_now {
            let existing = victory_keys(&*self.ws, &file);
            match sc::victory(&draft, &existing) {
                Ok(edits) => {
                    out.extend(edits);
                    label = "set victory".into();
                }
                Err(e) => self.scen.error = Some(e),
            }
        }
        if !out.is_empty() {
            let mut err = None;
            self.scen_apply(&mut err, &label, &file, &out);
            self.scen.error = err.or(self.scen.error.take());
        }
        self.show_scen_error(ui);
    }

    /// The Campaign screen: the graph (nodes, ordered edges, hidden edges) with its toolbar.
    pub(super) fn campaign_screen(&mut self, ui: &mut Ui) {
        let camp_before = self.scen.camp.clone();
        self.campaign_body(ui);
        // A selection made on the graph puts the campaign form in the Inspector.
        if self.scen.camp != camp_before {
            self.scen.focus = Focus::Campaign;
        }
    }

    fn campaign_body(&mut self, ui: &mut Ui) {
        let Some(pkg) = self.pkg.clone() else {
            ui.colored_label(ERROR_RED, tr("The package could not be read: see Problems."));
            return;
        };
        let text = self.ws.text_of(cg::FILE).ok();
        let graph: Graph = cg::build(&pkg, text.as_deref(), razdor::pack::open::lang());
        if self.scen.saved.is_none() {
            self.scen.saved = Some(cg::read_layout(self.ws.read_editor_file(cg::LAYOUT).as_deref()));
        }
        ui.horizontal(|ui| {
            if !graph.exists {
                ui.label(tr("The package has no campaign.toml: it plays as one map."));
                if ui.button(tr("Create campaign.toml")).clicked() {
                    let mut ids: Vec<String> = pkg.maps.keys().cloned().collect();
                    ids.sort();
                    let start = ids.first().cloned().unwrap_or_default();
                    if start.is_empty() {
                        self.log.push(tr("The package has no map to start a campaign on.").to_string());
                    } else {
                        let text = sc::new_campaign(&start, &ids);
                        match self.ws.create_text_file(cg::FILE, &text, "create campaign") {
                            Ok(()) => self.changed = true,
                            Err(e) => self.log.push(e),
                        }
                    }
                }
                return;
            }
            let in_campaign: Vec<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
            let mut addable: Vec<String> = pkg.maps.keys().filter(|m| !in_campaign.contains(m)).cloned().collect();
            addable.sort();
            if !addable.is_empty() {
                let refs: Vec<&str> = addable.iter().map(String::as_str).collect();
                if let Some(m) = forms::choice(ui, "camp-add", &self.scen.new_map.clone(), &refs) {
                    self.scen.new_map = m;
                }
                let target = if self.scen.new_map.is_empty() { addable[0].clone() } else { self.scen.new_map.clone() };
                if ui.button(trf!("Add map {map}", map = target.as_str())).clicked() {
                    self.apply_camp(&format!("add map {target}"), vec![sc::add_node(&target)]);
                }
            }
            if ui.button(tr("Auto layout")).clicked() {
                self.scen.saved = Some(BTreeMap::new());
                let _ = self.ws.write_editor_file(cg::LAYOUT, &cg::layout_json(self.ws.read_editor_file(cg::LAYOUT).as_deref(), &BTreeMap::new()));
            }
            ui.weak(tr("Drag a node to move it. Drag from its right edge to another node to add a next edge."));
        });
        if let Some(e) = self.scen.error.clone() {
            ui.colored_label(ERROR_RED, e);
        }
        let saved = self.scen.saved.clone().unwrap_or_default();
        let positions = cg::layout(&graph, &saved);
        let size = ui.available_size();
        let (resp, painter) = ui.allocate_painter(Vec2::new(size.x.max(200.0), size.y.max(200.0)), Sense::click());
        let origin = resp.rect.min;
        let at = |p: [f32; 2]| Pos2::new(origin.x + p[0], origin.y + p[1]);
        let rect_of = |id: &str| positions.get(id).map(|p| Rect::from_min_size(at(*p), NODE));
        // Edges (drawn first, under the nodes).
        let mut hover_text: Option<String> = None;
        for (ei, e) in graph.edges.iter().enumerate() {
            let (Some(a), Some(b)) = (rect_of(&e.from), rect_of(&e.to)) else { continue };
            let (p1, p2) = (Pos2::new(a.right(), a.center().y), Pos2::new(b.left(), b.center().y));
            let selected = self.scen.camp == CampSel::Edge(e.from.clone(), e.index);
            let stroke = Stroke::new(if selected { 2.5 } else { 1.5 }, if selected { ACCENT } else { GREY });
            if e.from == e.to {
                painter.line_segment([p1, p1 + Vec2::new(18.0, 0.0)], stroke);
            } else {
                painter.line_segment([p1, p2], stroke);
                arrow(&painter, p1, p2, stroke);
            }
            let label = if e.conds.is_empty() { format!("{}", e.index + 1) } else { format!("{} if", e.index + 1) };
            let mid = edge_label_at(p1, p2, ei);
            let w = label.chars().count() as f32 * 7.0 + 8.0;
            painter.rect_filled(Rect::from_center_size(mid, Vec2::new(w, 16.0)), 3.0, Color32::from_rgb(0x1c, 0x1f, 0x26));
            painter.text(mid, Align2::CENTER_CENTER, label, FontId::proportional(12.0), if selected { ACCENT } else { Color32::from_gray(0xc8) });
            // Clicking near the line selects the edge.
            if let Some(pos) = resp.hover_pos() {
                if resp.clicked() && dist_to_segment(pos, p1, p2) < 6.0 {
                    self.scen.camp = CampSel::Edge(e.from.clone(), e.index);
                    self.scen.error = None;
                }
            }
        }
        for h in &graph.hidden {
            let Some(from) = h.from.as_deref().and_then(&rect_of) else { continue };
            let target = h.to.as_str();
            let p1 = Pos2::new(from.right(), from.center().y);
            let p2 = match rect_of(target) {
                Some(b) => Pos2::new(b.left(), b.center().y),
                None => Pos2::new(from.right() + 60.0, from.center().y + 40.0),
            };
            let stroke = Stroke::new(1.2, WARNING_AMBER);
            painter.extend(Shape::dashed_line(&[p1, p2], stroke, 6.0, 4.0));
            let mid = Pos2::new((p1.x + p2.x) / 2.0, (p1.y + p2.y) / 2.0);
            let hit = Rect::from_center_size(mid, Vec2::splat(16.0));
            let r = ui.interact(hit, ui.id().with(("hidden", &h.by, h.file.as_str(), h.line)), Sense::hover());
            if r.hovered() {
                hover_text = Some(trf!("set by {by} ({file}:{line})", by = h.by.as_str(), file = h.file.as_str(), line = h.line));
            }
            if r.clicked() {
                self.open = Some(h.file.clone());
                *self.jump = Some(Jump { line: h.line, range: None });
            }
        }
        // Nodes.
        for n in &graph.nodes {
            let Some(r) = rect_of(&n.id) else { continue };
            let selected = self.scen.camp == CampSel::Node(n.id.clone());
            let fill = if graph.start.as_deref() == Some(n.id.as_str()) { NODE_START } else { NODE_FILL };
            let border = if !n.known { ERROR_RED } else if selected { ACCENT } else { GREY };
            painter.rect_filled(r, egui::CornerRadius::same(4), fill);
            painter.rect_stroke(r, egui::CornerRadius::same(4), Stroke::new(if selected { 2.0 } else { 1.0 }, border), egui::StrokeKind::Inside);
            let title = n.title.clone().unwrap_or_default();
            painter.text(r.left_top() + Vec2::new(8.0, 6.0), Align2::LEFT_TOP, n.id.as_str(), FontId::proportional(14.0), Color32::WHITE);
            let mut sub = title.clone();
            if graph.start.as_deref() == Some(n.id.as_str()) {
                sub = format!("{} · {sub}", tr("start"));
            }
            if n.revisit {
                sub = format!("{} · {sub}", tr("revisit"));
            }
            painter.text(r.left_top() + Vec2::new(8.0, 30.0), Align2::LEFT_TOP, sub, FontId::proportional(11.0), GREY);
            // Output port.
            let port = Pos2::new(r.right(), r.center().y);
            painter.circle_filled(port, 5.0, ACCENT);
            let body = ui.interact(r.shrink(6.0), ui.id().with(("node", n.id.as_str())), Sense::click_and_drag());
            let port_resp = ui.interact(Rect::from_center_size(port, Vec2::splat(14.0)), ui.id().with(("port", n.id.as_str())), Sense::drag());
            if body.clicked() {
                self.scen.camp = CampSel::Node(n.id.clone());
                self.scen.error = None;
            }
            if body.dragged() {
                let d = body.drag_delta();
                let p = positions.get(&n.id).copied().unwrap_or([0.0, 0.0]);
                self.scen.saved.get_or_insert_with(BTreeMap::new).insert(n.id.clone(), [p[0] + d.x, p[1] + d.y]);
            }
            if body.drag_stopped() {
                self.save_layout();
            }
            if port_resp.drag_started() {
                self.scen.link_from = Some(n.id.clone());
            }
        }
        // A link being dragged: the rubber band, then the edge on release over a node.
        if let (Some(from), Some(pos)) = (self.scen.link_from.clone(), ui.input(|i| i.pointer.hover_pos())) {
            if let Some(r) = rect_of(&from) {
                painter.line_segment([Pos2::new(r.right(), r.center().y), pos], Stroke::new(1.5, ACCENT));
            }
            if ui.input(|i| i.pointer.any_released()) {
                let target = graph.nodes.iter().find(|n| rect_of(&n.id).is_some_and(|r| r.contains(pos))).map(|n| n.id.clone());
                self.scen.link_from = None;
                if let Some(to) = target {
                    self.apply_camp(&format!("add edge {from} to {to}"), vec![sc::add_edge(&from, &to)]);
                }
            }
        }
        if let Some(t) = hover_text {
            resp.on_hover_text(t);
        }
    }

    /// Writes the saved node positions (`.editor/layout.json`); not an undo step.
    fn save_layout(&mut self) {
        let saved = self.scen.saved.clone().unwrap_or_default();
        if let Err(e) = self.ws.write_editor_file(cg::LAYOUT, &cg::layout_json(self.ws.read_editor_file(cg::LAYOUT).as_deref(), &saved)) {
            self.log.push(e);
        }
    }

    /// Applies campaign edits as one undo step (the campaign file).
    fn apply_camp(&mut self, label: &str, edits: Vec<DocEdit>) {
        let mut err = None;
        self.scen_apply(&mut err, label, cg::FILE, &edits);
        self.scen.error = err;
    }

    /// The Inspector's campaign form: the graph's carry flags and start, a node's revisit and start,
    /// or an edge's order, target and conditions.
    pub(super) fn campaign_inspector(&mut self, ui: &mut Ui) {
        ui.strong(tr("Campaign"));
        let Some(pkg) = self.pkg.clone() else { return };
        let text = self.ws.text_of(cg::FILE).ok();
        let graph = cg::build(&pkg, text.as_deref(), razdor::pack::open::lang());
        if !graph.exists {
            ui.weak(tr("No campaign.toml: create it on the Campaign screen."));
            return;
        }
        match self.scen.camp.clone() {
            CampSel::Graph => {
                ui.weak(tr("Carry: what moves from map to map on a victory or a travel."));
                for (key, on) in graph.carry.flags.iter() {
                    let mut v = *on;
                    if ui.checkbox(&mut v, *key).changed() {
                        self.apply_camp(&format!("set carry {key}"), vec![sc::set_carry(key, v)]);
                    }
                }
                ui.label(trf!("Start: {start}", start = graph.start.clone().unwrap_or_else(|| "-".into())));
            }
            CampSel::Node(id) => {
                let Some(node) = graph.node(&id).cloned() else { return };
                ui.strong(id.as_str());
                let mut rv = node.revisit;
                if ui.checkbox(&mut rv, tr("Revisit: keeps the map's state when the hero leaves")).changed() {
                    self.apply_camp("set revisit", vec![sc::set_revisit(&id, rv)]);
                }
                if graph.start.as_deref() != Some(id.as_str()) && ui.button(tr("Make the start")).clicked() {
                    self.apply_camp("set start", vec![sc::set_start(&id)]);
                }
                ui.weak(tr("Next, in priority order:"));
                for e in graph.edges_from(&id) {
                    if ui.link(trf!("Next {n}: {to}", n = e.index + 1, to = e.to.as_str())).clicked() {
                        self.scen.camp = CampSel::Edge(id.clone(), e.index);
                    }
                }
                for h in graph.hidden.iter().filter(|h| h.from.as_deref() == Some(id.as_str())) {
                    ui.weak(trf!("{to} (dashed, {by})", to = h.to.as_str(), by = h.by.as_str()));
                }
            }
            CampSel::Edge(from, index) => {
                let Some(edge) = graph.edges.iter().find(|e| e.from == from && e.index == index).cloned() else {
                    self.scen.camp = CampSel::Graph;
                    return;
                };
                ui.strong(trf!("Edge: {from} → {to}", from = from.as_str(), to = edge.to.as_str()));
                ui.horizontal(|ui| {
                    ui.label(trf!("Priority {n}", n = index + 1));
                    if ui.add_enabled(index > 0, egui::Button::new(tr("Up"))).clicked() {
                        self.apply_camp("move edge up", vec![sc::move_edge(&from, index, index - 1)]);
                        self.scen.camp = CampSel::Edge(from.clone(), index - 1);
                    }
                    let last = graph.edges_from(&from).count().saturating_sub(1);
                    if ui.add_enabled(index < last, egui::Button::new(tr("Down"))).clicked() {
                        self.apply_camp("move edge down", vec![sc::move_edge(&from, index, index + 1)]);
                        self.scen.camp = CampSel::Edge(from.clone(), index + 1);
                    }
                    if ui.button(tr("Delete")).clicked() {
                        self.apply_camp("delete edge", vec![sc::delete_edge(&from, index)]);
                        self.scen.camp = CampSel::Node(from.clone());
                    }
                });
                if self.scen.edge_if_for.as_ref() != Some(&(from.clone(), index)) {
                    self.scen.edge_if = edge.conds.join("\n");
                    self.scen.edge_if_for = Some((from.clone(), index));
                }
                ui.weak(tr("Conditions (all must hold), one inline table per line (see the wiki). Empty: always taken."));
                ui.add(egui::TextEdit::multiline(&mut self.scen.edge_if).desired_rows(4).desired_width(f32::INFINITY).code_editor());
                if ui.button(tr("Apply conditions")).clicked() {
                    match parse_conds(&self.scen.edge_if) {
                        Ok(values) => {
                            self.apply_camp("set conditions", vec![sc::set_edge_if(&from, index, &values)]);
                            self.scen.error = None;
                        }
                        Err(e) => self.scen.error = Some(e),
                    }
                }
                self.show_scen_error(ui);
            }
        }
    }

    /// The named characters: the list, the places each is given, required and removed, the warnings
    /// and the rename across the files that name it.
    pub(super) fn characters_dock(&mut self, ui: &mut Ui) {
        let Some(pkg) = self.pkg.clone() else {
            ui.weak(tr("The package could not be read: see Problems."));
            return;
        };
        let text = self.ws.text_of(cg::FILE).ok();
        let order = cg::order(&cg::build(&pkg, text.as_deref(), razdor::pack::open::lang()));
        let list = chars::characters(&pkg);
        let warnings = chars::warnings(&list, &order);
        if list.is_empty() {
            ui.weak(tr("No named characters: a give_unit with a named id makes one."));
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for w in &warnings {
                if ui.link(egui::RichText::new(&w.message).color(WARNING_AMBER)).clicked() {
                    self.open = Some(w.file.clone());
                    *self.jump = Some(Jump { line: w.line, range: None });
                }
            }
            ui.separator();
            for c in &list {
                let selected = self.scen.character.as_deref() == Some(c.id.as_str());
                if ui.selectable_label(selected, format!("{} · {}/{}/{}", c.id, c.gives.len(), c.needs.len(), c.removes.len())).clicked() {
                    self.scen.character = Some(c.id.clone());
                    self.scen.rename_to = c.id.clone();
                }
                if !selected {
                    continue;
                }
                for (title, sites) in [(tr("Given"), &c.gives), (tr("Required"), &c.needs), (tr("Removed"), &c.removes)] {
                    ui.weak(title);
                    for s in sites.iter() {
                        let kind = match s.kind {
                            SiteKind::Give => "give",
                            SiteKind::Need => "need",
                            SiteKind::Remove => "remove",
                        };
                        if ui.link(format!("{}:{} · {} · {kind}", s.file, s.line, s.by)).clicked() {
                            self.open = Some(s.file.clone());
                            *self.jump = Some(Jump { line: s.line, range: None });
                        }
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(tr("Rename to"));
                    ui.add(egui::TextEdit::singleline(&mut self.scen.rename_to).desired_width(120.0));
                    if ui.button(tr("Rename")).clicked() {
                        let old = c.id.clone();
                        let new = self.scen.rename_to.trim().to_string();
                        let ws = &*self.ws;
                        let plan = chars::rename_plan(&pkg, &old, &new, &|f: &str| ws.text_of(f).ok());
                        match plan {
                            Ok(files) => match self.ws.set_texts(&format!("rename {old} to {new}"), files) {
                                Ok(()) => {
                                    self.scen.character = Some(new.clone());
                                    self.changed = true;
                                }
                                Err(e) => self.log.push(e),
                            },
                            Err(e) => self.log.push(e),
                        }
                    }
                });
            }
        });
    }

    /// The Texts screen: one row per key, one column per language (`texts/`), missing and undefined
    /// keys marked, a filter, and a new language.
    pub(super) fn texts_screen(&mut self, ui: &mut Ui) {
        let Some(pkg) = self.pkg.clone() else {
            ui.colored_label(ERROR_RED, tr("The package could not be read: see Problems."));
            return;
        };
        let table = tx::table_of(&pkg);
        ui.horizontal(|ui| {
            ui.label(tr("Filter"));
            ui.add(egui::TextEdit::singleline(&mut self.scen.filter).desired_width(180.0));
            ui.label(tr("New language"));
            ui.add(egui::TextEdit::singleline(&mut self.scen.new_lang).hint_text(tr("code, e.g. de")).desired_width(110.0));
            if ui.button(tr("Add language")).clicked() {
                let lang = self.scen.new_lang.trim().to_string();
                if !tx::valid_lang(&lang) {
                    self.log.push(trf!("{lang} is not a language code: lowercase letters, digits and -", lang = lang.as_str()));
                } else {
                    let keys: Vec<String> = table.rows.iter().map(|r| r.key.clone()).collect();
                    match self.ws.create_text_file(&format!("texts/{lang}.toml"), &tx::empty_file(&keys), &format!("add language {lang}")) {
                        Ok(()) => self.changed = true,
                        Err(e) => self.log.push(e),
                    }
                }
            }
        });
        ui.separator();
        let filter = self.scen.filter.to_lowercase();
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new(ui.id().with("texts-grid")).num_columns(table.langs.len() + 1).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
                ui.strong(tr("Key"));
                for l in &table.langs {
                    ui.strong(l.as_str());
                }
                ui.end_row();
                for (i, row) in table.rows.iter().enumerate() {
                    if !filter.is_empty() && !row.key.to_lowercase().contains(&filter) {
                        continue;
                    }
                    let colour = match &row.status {
                        Status::Undefined => ERROR_RED,
                        Status::Unused => GREY,
                        Status::Missing(_) => WARNING_AMBER,
                        Status::Complete => Color32::from_gray(0xe0),
                    };
                    let mut tip = match &row.status {
                        Status::Undefined => tr("Used by the package, defined in no language").to_string(),
                        Status::Unused => tr("Defined, never used").to_string(),
                        Status::Missing(langs) => trf!("Missing in {langs}", langs = langs.join(", ")),
                        Status::Complete => String::new(),
                    };
                    if let Some((file, line)) = &row.used_at {
                        tip = format!("{tip}\n{file}:{line}");
                    }
                    ui.label(egui::RichText::new(&row.key).color(colour).monospace()).on_hover_text(tip);
                    for (li, l) in table.langs.iter().enumerate() {
                        let current = row.cells.get(li).cloned().unwrap_or_default();
                        let salt = format!("tx-{i}-{l}");
                        let typed = ui.scope(|ui| {
                            ui.set_min_width(230.0);
                            forms::text_field(ui, &salt, &current, 220.0)
                        });
                        if let Some(v) = typed.inner {
                            let (file, edit) = tx::set_cell(l, &row.key, &v);
                            self.scen_apply_texts(&file, &edit);
                        }
                    }
                    ui.end_row();
                }
            });
        });
    }

    fn scen_apply_texts(&mut self, file: &str, edit: &DocEdit) {
        match self.ws.apply_doc(file, &edit.label(), std::slice::from_ref(edit)) {
            Ok(()) => self.changed = true,
            Err(e) => self.log.push(e),
        }
    }
}

/// The map's size in cells (the cell fields of the victory are clamped to it).
fn map_size(v: &Viewer) -> (u32, u32) {
    v.maps.current().map_or((200, 200), |g| (g.compiled.scenario.width(), g.compiled.scenario.height()))
}

/// The victory as the form holds it.
fn victory_draft(v: &Victory) -> VictoryDraft {
    match v {
        Victory::DefeatAll { message } => VictoryDraft { kind: "defeat_all".into(), army: None, building: None, cell: None, message: message.clone() },
        Victory::DefeatArmy { army, message } => VictoryDraft { kind: "defeat_army".into(), army: Some(army.clone()), building: None, cell: None, message: message.clone() },
        Victory::Capture { building, message } => VictoryDraft { kind: "capture".into(), army: None, building: Some(building.clone()), cell: None, message: message.clone() },
        Victory::Reach { cell, message } => VictoryDraft { kind: "reach".into(), army: None, building: None, cell: Some(*cell), message: message.clone() },
    }
}

/// The keys of the map file's `[victory]` table now.
fn victory_keys(ws: &razdor::studio::Workspace, file: &str) -> Vec<String> {
    let Ok(text) = ws.text_of(file) else { return Vec::new() };
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else { return Vec::new() };
    doc.get("victory").and_then(|i| i.as_table()).map(|t| t.iter().map(|(k, _)| k.to_string()).collect()).unwrap_or_default()
}

/// The conditions typed in the edge box (one inline table per line), checked by the trigger
/// vocabulary (`pack::triggers::parse_cond`); returns them as TOML values for the edit.
fn parse_conds(text: &str) -> Result<Vec<Value>, String> {
    let body: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let src = format!("if = [{}]", body.join(", "));
    let table: toml::Table = src.parse().map_err(|e: toml::de::Error| format!("the conditions do not parse: {e}"))?;
    let arr = table.get("if").and_then(toml::Value::as_array).ok_or("the conditions are a list of inline tables")?;
    for (i, c) in arr.iter().enumerate() {
        let t = c.as_table().ok_or_else(|| format!("condition {} is not an inline table", i + 1))?;
        pt::parse_cond(t).map_err(|e| format!("condition {}: {}", i + 1, e.message))?;
    }
    let doc: toml_edit::DocumentMut = src.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    Ok(doc.get("if").and_then(|i| i.as_array()).map(|a| a.iter().cloned().collect()).unwrap_or_default())
}

/// The arrow head at `to` of the segment `from` → `to`.
fn arrow(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke) {
    let d = (to - from).normalized();
    let n = Vec2::new(-d.y, d.x);
    let tip = to;
    let back = tip - d * 8.0;
    painter.line_segment([tip, back + n * 4.0], stroke);
    painter.line_segment([tip, back - n * 4.0], stroke);
}

/// The distance from `p` to the segment `a`–`b`.
fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq().max(1e-6);
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

