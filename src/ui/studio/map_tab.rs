//! The Map tab: the map of the open package, drawn by the game's own map renderer
//! (`ui::editor::canvas`) into the tab's rectangle. egui allocates the rectangle and takes the
//! input (pan, zoom, click, drag, hover); the map is drawn with macroquad after the egui pass and
//! before egui's meshes, clipped to the rectangle, so nothing of it shows over another dock.
//!
//! The tools (the bar at the top of the tab). Select: a left press on the selected army, point,
//! hero or building and a drag moves it (one `SetCell` edit on release); a click selects. Building,
//! Army, Point and Object: a click places a new table at the cell (`studio::place`). Terrain: a
//! left drag paints the palette's kind with the brush (one undo step per stroke; the picture shows
//! the paint at once and the rebuild confirms it).
//!
//! The overlays: the live placement errors and warnings as red and amber outlines (`Canvas::marks`,
//! the diagnostics mapped by `studio::map::marks`), the reach overlay (the unreached story cells
//! hatched red), the Footprints outlines and the Grid. Hovering a marked object or an unreached
//! cell shows its messages in a tooltip.

use egui::{Align2, Color32, ComboBox, CornerRadius, FontId, PointerButton, Sense, TextStyle, Ui};
use egui_macroquad::egui;
use macroquad::prelude as mq;
use razdor::editor::geometry::CellRect;
use razdor::i18n::tr;
use razdor::pack::compile::{ARMY_MODELS, BUILDING_NAMES, OBJECT_KINDS};
use razdor::pack::diag::Severity;
use razdor::pack::load::Package;
use razdor::pack::reach::{Named, Site};
use razdor::pack::terrain::DEFAULT_PALETTE;
use razdor::studio::map::{self, footprint_cells_of, hit, map_ids, outline_of, Mark, MapView, Selection};
use razdor::studio::Place;
use razdor::trf;

use super::{Viewer, WARNING_AMBER};
use crate::ui::dt_art::DtArt;
use crate::ui::editor::canvas::{draw_map, Cam, Overview};

/// The tools of the Map tab's bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Building,
    Army,
    Point,
    Object,
    Terrain,
}

impl Tool {
    /// Every tool, in the bar's order.
    pub const ALL: [Tool; 6] = [Tool::Select, Tool::Building, Tool::Army, Tool::Point, Tool::Object, Tool::Terrain];

    /// The tool's name in the bar (`tr`).
    pub fn title(self) -> &'static str {
        match self {
            Tool::Select => tr("Select"),
            Tool::Building => tr("Building"),
            Tool::Army => tr("Army"),
            Tool::Point => tr("Point"),
            Tool::Object => tr("Object"),
            Tool::Terrain => tr("Terrain"),
        }
    }

    /// The tool named `name` in a snapshot option (`tool=terrain`).
    pub fn named(name: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| format!("{t:?}").eq_ignore_ascii_case(name))
    }
}

/// The brush sizes the terrain tool offers, in cells a side.
pub const BRUSHES: [u32; 4] = [1, 3, 5, 9];

/// A move of a table: what it is, where it was, where the pointer's cell puts it now, the grab
/// offset of a building (its cell minus the pressed cell), and the compile the move was applied
/// before (`None` while the button is still down).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Move {
    pub sel: Selection,
    pub origin: (i32, i32),
    pub target: (i32, i32),
    pub grab: (i32, i32),
    pub serial: Option<u64>,
}

/// The Map tab's view: the camera, the rectangle and the hovered cell, the tools and the overlays.
pub struct Canvas {
    cam: Option<Cam>,
    /// Fit the whole map on the next frame (the map was switched).
    pub fit: bool,
    /// A cell-unit point to centre the view on (a row of the Scene dock was clicked).
    pub centre: Option<(f32, f32)>,
    /// The tab's rectangle in egui points, and the pixels per point it was laid out with.
    rect: Option<egui::Rect>,
    ppp: f32,
    hover: Option<(i32, i32)>,
    overview: Overview,
    overview_serial: u64,
    /// The move in progress, or the one applied and not compiled yet.
    pub moving: Option<Move>,
    /// The tool the bar picked.
    pub tool: Tool,
    /// The brush size of the terrain tool (1, 3, 5 or 9).
    pub brush: u32,
    /// The terrain code the brush paints (an index into `pack::terrain::DEFAULT_PALETTE`).
    pub code: u8,
    /// The kinds the placing tools add: a building type, an army model, an object kind.
    pub building_kind: String,
    pub army_model: String,
    pub object_kind: String,
    /// The overlays' switches (the bar's toggles; not saved).
    pub grid: bool,
    pub footprints: bool,
    pub reach: bool,
    /// The live diagnostics on the map's tables (`studio::map::marks`).
    pub marks: Vec<Mark>,
    /// The story cells the hero cannot reach on foot (`pack::reach`), of the map shown.
    pub unreached: Vec<Site>,
}

impl Default for Canvas {
    fn default() -> Canvas {
        Canvas {
            cam: None,
            fit: false,
            centre: None,
            rect: None,
            ppp: 1.0,
            hover: None,
            overview: Overview::default(),
            overview_serial: 0,
            moving: None,
            tool: Tool::Select,
            brush: 1,
            code: 6,
            building_kind: BUILDING_NAMES[2].to_string(),
            army_model: ARMY_MODELS[0].0.to_string(),
            object_kind: OBJECT_KINDS[3].0.to_string(),
            grid: false,
            footprints: false,
            reach: false,
            marks: Vec::new(),
            unreached: Vec::new(),
        }
    }
}

/// Shows map `id` on the Map tab: the camera fits it, and the selection goes.
pub fn switch_map(maps: &mut MapView, canvas: &mut Canvas, selection: &mut Option<Selection>, id: &str) {
    if maps.map.as_deref() != Some(id) {
        maps.show(id);
        canvas.fit = true;
        canvas.hover = None;
        canvas.moving = None;
        canvas.marks.clear();
        canvas.unreached.clear();
        *selection = None;
    }
}

/// The accent colour of the studio (the selection outline).
fn accent() -> mq::Color {
    mq::Color::from_rgba(0x4f, 0x8e, 0xdc, 255)
}

/// The colour of a diagnostic's outline: red for an error, amber for a warning.
fn mark_colour(severity: Severity) -> mq::Color {
    match severity {
        Severity::Error => mq::Color::from_rgba(0xe0, 0x5a, 0x5a, 255),
        Severity::Warning => mq::Color::from_rgba(0xe8, 0xb0, 0x4f, 255),
    }
}

/// The things a drag can move: the hero, buildings, armies and lanterns (not the terrain's objects).
fn movable(sel: &Selection) -> bool {
    !matches!(sel, Selection::Object(_))
}

/// A stale file's banner: amber, with the parser's line (see `studio::Stale`).
fn banner(ui: &mut Ui, text: &str) {
    egui::Frame::new().fill(WARNING_AMBER).corner_radius(CornerRadius::same(3)).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
        ui.colored_label(Color32::from_rgb(0x20, 0x18, 0x08), text);
    });
}

/// The cells of a brush of `size` centred on `(x, y)` (the editor's brush: the extra cell goes
/// down and right on an even size, which the odd sizes do not have).
fn brush_cells(x: i32, y: i32, size: u32) -> Vec<(i64, i64)> {
    let r = CellRect::brush(x, y, size);
    (r.y0..=r.y1).flat_map(|yy| (r.x0..=r.x1).map(move |xx| (i64::from(xx), i64::from(yy)))).collect()
}

impl Viewer<'_> {
    /// The Map tab: the map picker and the tool bar, the stale banners, the canvas (its input here,
    /// its drawing in [`draw`]), and the hovered cell at the bottom left.
    pub(super) fn map_tab(&mut self, ui: &mut Ui) {
        self.map_picker(ui);
        self.tool_bar(ui);
        for stale in &self.ws.stale {
            banner(ui, &stale.banner());
        }
        let status = ui.text_style_height(&TextStyle::Body) + 6.0;
        let size = egui::vec2(ui.available_width(), (ui.available_height() - status).max(60.0));
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        let to_px = |p: egui::Pos2| mq::vec2(p.x * ppp, p.y * ppp);
        let pkg = self.pkg.clone();
        let tool = self.canvas.tool;
        let canvas = &mut *self.canvas;
        canvas.rect = Some(rect);
        canvas.ppp = ppp;
        // A click on the map picks what it hits, a placing click places, a painted cell is applied
        // to the shown compile after the match: all decided here, applied after it.
        let mut pick: Option<Option<Selection>> = None;
        let mut place_at: Option<(i32, i32)> = None;
        let mut painted: Vec<(u32, u32, u8)> = Vec::new();
        let mut paint_cells: Option<Vec<(i64, i64)>> = None;
        let mut paint_end = false;
        let mut tip: Option<String> = None;

        match self.maps.current() {
            Some(good) => {
                let s = &good.compiled.scenario;
                let (w, h) = (s.width(), s.height());
                let view = mq::Rect::new(rect.min.x * ppp, rect.min.y * ppp, rect.width() * ppp, rect.height() * ppp);
                let fresh = canvas.cam.is_none();
                let cam = canvas.cam.get_or_insert_with(|| Cam::new(view, w, h));
                cam.view = view;
                if fresh || std::mem::take(&mut canvas.fit) {
                    cam.fit(w, h);
                }
                if let Some((x, y)) = canvas.centre.take() {
                    cam.centre = mq::vec2(x, y);
                }
                if resp.hovered() {
                    let scroll = ui.input(|i| i.raw_scroll_delta.y);
                    if let (true, Some(p)) = (scroll != 0.0, resp.hover_pos()) {
                        cam.zoom_at(1.0015f32.powf(scroll), to_px(p));
                    }
                }
                if resp.dragged_by(PointerButton::Middle) || resp.dragged_by(PointerButton::Secondary) {
                    let d = ui.input(|i| i.pointer.delta());
                    cam.pan(mq::vec2(d.x * ppp, d.y * ppp));
                }
                cam.clamp(w, h);
                let hover = resp.hover_pos().map(|p| cam.cell_at(to_px(p))).filter(|&(x, y)| x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h);
                canvas.hover = hover;

                let entry = pkg.as_ref().and_then(|p| p.maps.get(&good.map));
                let spec = entry.and_then(|m| m.spec.as_ref());
                if let (Some(spec), Some(cell)) = (spec, hover) {
                    tip = tip_of(&canvas.marks, canvas.reach, &canvas.unreached, &good.compiled, spec, cell);
                }
                match tool {
                    Tool::Select => {
                        if resp.drag_started_by(PointerButton::Primary) {
                            if let (Some(spec), Some(p)) = (spec, ui.input(|i| i.pointer.press_origin())) {
                                let press = cam.cell_at(to_px(p));
                                if let Some(sel) = self.selection.clone().filter(movable) {
                                    if hit(&good.compiled, spec, press).as_ref() == Some(&sel) {
                                        if let Some([x, y]) = map::cell_of(spec, &sel) {
                                            let origin = (x as i32, y as i32);
                                            let grab = if matches!(sel, Selection::Building(_)) { (press.0 - origin.0, press.1 - origin.1) } else { (0, 0) };
                                            canvas.moving = Some(Move { sel, origin, target: origin, grab, serial: None });
                                        }
                                    }
                                }
                            }
                        }
                        if resp.dragged_by(PointerButton::Primary) {
                            if let (Some(m), Some(p)) = (canvas.moving.as_mut(), resp.interact_pointer_pos()) {
                                if m.serial.is_none() {
                                    let c = cam.cell_at(to_px(p));
                                    m.target = (c.0 - m.grab.0, c.1 - m.grab.1);
                                }
                            }
                        }
                        if resp.drag_stopped_by(PointerButton::Primary) {
                            if let Some(mut m) = canvas.moving.take() {
                                if m.serial.is_none() && m.target != m.origin {
                                    let inside = inside(good, m.target);
                                    match (inside, spec, entry) {
                                        (false, _, _) => self.log.push(tr("Outside the map: the move is not made").to_string()),
                                        (true, Some(spec), Some(entry)) => match map::locator_of(&entry.file, spec, &m.sel) {
                                            Some(at) => match self.ws.apply(razdor::studio::Edit::SetCell { at, cell: m.target }) {
                                                Ok(()) => {
                                                    self.changed = true;
                                                    m.serial = Some(good.serial);
                                                    canvas.moving = Some(m);
                                                }
                                                Err(e) => self.log.push(e),
                                            },
                                            None => self.log.push(tr("This object has no table in the file").to_string()),
                                        },
                                        _ => {}
                                    }
                                }
                            }
                        }
                        if resp.clicked() {
                            pick = Some(match (hover, spec) {
                                (Some(cell), Some(spec)) => hit(&good.compiled, spec, cell),
                                _ => None,
                            });
                        }
                    }
                    Tool::Terrain => {
                        if resp.drag_started_by(PointerButton::Primary) {
                            if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
                                let c = cam.cell_at(to_px(p));
                                paint_cells = Some(brush_cells(c.0, c.1, canvas.brush));
                            }
                        } else if resp.dragged_by(PointerButton::Primary) || resp.clicked() {
                            if let Some(c) = hover {
                                paint_cells = Some(brush_cells(c.0, c.1, canvas.brush));
                            }
                        }
                        if resp.drag_stopped_by(PointerButton::Primary) || resp.clicked() {
                            paint_end = true;
                        }
                    }
                    _ => {
                        if resp.clicked() {
                            place_at = hover;
                        }
                    }
                }
                if let Some(cells) = &paint_cells {
                    let code = canvas.code;
                    let Some(map_id) = self.maps.map.clone() else { return };
                    match self.ws.paint(&map_id, cells, code) {
                        Ok(()) => {
                            for &(x, y) in cells {
                                if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                                    painted.push((x as u32, y as u32, code));
                                }
                            }
                            self.changed = true;
                        }
                        Err(e) => self.log.push(e),
                    }
                }
                if paint_end {
                    self.ws.paint_end();
                }
            }
            None => {
                let msg = if !self.has_install {
                    tr("Install not found: set the game folder in the game's settings")
                } else if self.ws.package().is_none() {
                    tr("The package could not be read: see Output.")
                } else if !self.map_problems.is_empty() {
                    tr("The map does not compile: see Problems.")
                } else if self.maps.map.is_none() {
                    tr("No map in the package.")
                } else {
                    tr("Compiling the map...")
                };
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, msg, FontId::proportional(16.0), Color32::from_gray(0xb0));
                canvas.hover = None;
            }
        }

        if !painted.is_empty() {
            self.maps.repaint(&painted);
        }
        if let Some(cell) = place_at {
            self.place_at(cell, tool);
        }
        match pick {
            Some(Some(sel)) => self.select(sel),
            Some(None) => *self.selection = None,
            None => {}
        }
        if let Some(text) = tip {
            resp.on_hover_text(text);
        }
        ui.horizontal(|ui| {
            if let Some((x, y)) = self.canvas.hover {
                ui.weak(format!("{x}, {y}"));
            }
        });
    }

    /// Adds a table of the placing tool's kind at `cell` (one undo step), and selects it.
    fn place_at(&mut self, (x, y): (i32, i32), tool: Tool) {
        let Some(map_id) = self.maps.map.clone() else { return };
        let Some(good) = self.maps.current() else { return };
        let s = &good.compiled.scenario;
        if x < 0 || y < 0 || x as u32 >= s.width() || y as u32 >= s.height() {
            return;
        }
        let cell = (x as u32, y as u32);
        let kind = match tool {
            Tool::Building => razdor::studio::Kind::Building,
            Tool::Army => razdor::studio::Kind::Army,
            Tool::Point => razdor::studio::Kind::Point,
            Tool::Object => razdor::studio::Kind::Object,
            Tool::Select | Tool::Terrain => return,
        };
        let file = format!("maps/{map_id}/map.toml");
        let existing = self.ws.spans(&file).map(|s| s.iter().filter(|(l, _)| l.kind == kind).count()).unwrap_or(0);
        let building = self.canvas.building_kind.clone();
        let army = self.canvas.army_model.clone();
        let object = self.canvas.object_kind.clone();
        let what = match tool {
            Tool::Building => Place::Building { kind: &building, cell },
            Tool::Army => Place::Army { model: &army, cell },
            Tool::Point => Place::Point { cell },
            _ => Place::Object { kind: &object, cell },
        };
        match self.ws.place(&map_id, what) {
            Ok(id) => {
                let sel = match (tool, id) {
                    (Tool::Building, Some(id)) => Selection::Building(id),
                    (Tool::Army, Some(id)) => Selection::Army(id),
                    (Tool::Point, _) => Selection::Point(existing),
                    _ => Selection::Object(existing),
                };
                self.log.push(trf!("Added {what} at ({x}, {y})", what = sel_name(&sel), x, y));
                self.changed = true;
                self.select(sel);
            }
            Err(e) => self.log.push(e),
        }
    }

    /// The bar of tools and the overlay switches, under the map picker.
    fn tool_bar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            for tool in Tool::ALL {
                if ui.selectable_label(self.canvas.tool == tool, tool.title()).clicked() {
                    self.canvas.tool = tool;
                }
            }
            ui.separator();
            match self.canvas.tool {
                Tool::Building => {
                    ComboBox::from_id_salt("studio-tool-building").selected_text(self.canvas.building_kind.clone()).show_ui(ui, |ui| {
                        for name in BUILDING_NAMES {
                            if ui.selectable_label(self.canvas.building_kind == name, name).clicked() {
                                self.canvas.building_kind = name.to_string();
                            }
                        }
                    });
                }
                Tool::Army => {
                    ComboBox::from_id_salt("studio-tool-army").selected_text(self.canvas.army_model.clone()).show_ui(ui, |ui| {
                        for (name, _, _) in ARMY_MODELS {
                            if ui.selectable_label(self.canvas.army_model == name, name).clicked() {
                                self.canvas.army_model = name.to_string();
                            }
                        }
                    });
                }
                Tool::Object => {
                    ComboBox::from_id_salt("studio-tool-object").selected_text(self.canvas.object_kind.clone()).show_ui(ui, |ui| {
                        for (name, _, _) in OBJECT_KINDS {
                            if ui.selectable_label(self.canvas.object_kind == name, name).clicked() {
                                self.canvas.object_kind = name.to_string();
                            }
                        }
                    });
                }
                Tool::Terrain => {
                    ui.label(tr("Brush"));
                    for size in BRUSHES {
                        if ui.selectable_label(self.canvas.brush == size, format!("{size}×{size}")).clicked() {
                            self.canvas.brush = size;
                        }
                    }
                }
                Tool::Select | Tool::Point => {}
            }
            ui.separator();
            ui.checkbox(&mut self.canvas.reach, tr("Reach")).on_hover_text(tr("Cells the hero cannot walk to, on the map's story"));
            ui.checkbox(&mut self.canvas.grid, tr("Grid"));
            ui.checkbox(&mut self.canvas.footprints, tr("Footprints"));
        });
        if self.canvas.tool == Tool::Terrain {
            ui.horizontal_wrapped(|ui| {
                for (i, (name, rgb)) in DEFAULT_PALETTE.iter().enumerate() {
                    let colour = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                    ui.painter().rect_filled(rect, CornerRadius::same(2), colour);
                    if self.canvas.code == i as u8 {
                        ui.painter().rect_stroke(rect, CornerRadius::same(2), egui::Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
                    }
                    if resp.on_hover_text(*name).clicked() {
                        self.canvas.code = i as u8;
                    }
                }
            });
        }
    }

    /// The combo box of the maps.
    fn map_picker(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(tr("Map"));
            let Some(pkg) = self.pkg.clone() else {
                ui.weak(tr("The package could not be read: see Output."));
                return;
            };
            let ids = map_ids(&pkg);
            let current = self.maps.map.clone().unwrap_or_default();
            let mut picked = None;
            ComboBox::from_id_salt("studio-map").selected_text(current.as_str()).show_ui(ui, |ui| {
                for id in &ids {
                    if ui.selectable_label(*id == current, id.as_str()).clicked() {
                        picked = Some(id.clone());
                    }
                }
            });
            if let Some(id) = picked {
                switch_map(self.maps, self.canvas, self.selection, &id);
            }
        });
    }
}

/// The name of a selection for a message: `hero`, `army toll`, `point #2`.
fn sel_name(sel: &Selection) -> String {
    match sel {
        Selection::Hero => "hero".into(),
        Selection::Building(id) => format!("building {id}"),
        Selection::Army(id) => format!("army {id}"),
        Selection::Point(i) => format!("point #{}", i + 1),
        Selection::Object(i) => format!("object #{}", i + 1),
    }
}

/// The tooltip of a cell: the messages of the diagnostics on the table under it, or the story's
/// reason when it is an unreached cell.
fn tip_of(marks: &[Mark], reach: bool, unreached: &[Site], compiled: &razdor::pack::compile::Compiled, spec: &razdor::pack::map::MapFile, cell: (i32, i32)) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    if let Some(sel) = hit(compiled, spec, cell) {
        for m in marks.iter().filter(|m| m.sel == sel) {
            lines.push(format!("{}: {}", if m.severity == Severity::Error { tr("Error") } else { tr("Warning") }, m.message));
        }
    }
    if reach {
        for site in unreached.iter().filter(|s| site_has(s, cell)) {
            lines.push(trf!("Cannot be reached on foot: {what}", what = site.what.as_str()));
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Whether a site names the cell (a cell, or an area that holds it).
fn site_has(site: &Site, (x, y): (i32, i32)) -> bool {
    match site.named {
        Named::Cell(c) => c == (x, y),
        Named::Area(ax, ay, aw, ah) => x >= ax && y >= ay && x < ax.saturating_add(aw) && y < ay.saturating_add(ah),
    }
}

/// Whether a cell is on the map.
fn inside(good: &razdor::studio::map::Good, (x, y): (i32, i32)) -> bool {
    let s = &good.compiled.scenario;
    x >= 0 && y >= 0 && (x as u32) < s.width() && (y as u32) < s.height()
}

/// Draws the shown map into the Map tab's rectangle: the terrain and the objects, the overlays
/// (Footprints, Grid, the reach hatching, the diagnostics' outlines), the selection, the move's
/// ghost and the hovered cell. Runs after the egui pass and before egui is drawn over it.
pub fn draw(canvas: &mut Canvas, maps: &MapView, pkg: Option<&Package>, selection: Option<&Selection>, art: Option<&DtArt>) {
    let (Some(_), Some(cam)) = (canvas.rect, canvas.cam.as_mut()) else { return };
    let Some(good) = maps.current() else { return };
    if canvas.overview_serial != good.serial {
        canvas.overview = Overview::default();
        canvas.overview_serial = good.serial;
    }
    let tex = canvas.overview.texture(&good.doc);
    scissor(Some(cam.view));
    draw_map(&good.doc, art, cam, tex.as_ref());
    let spec = pkg.and_then(|p| p.maps.get(&good.map)).and_then(|m| m.spec.as_ref());
    if canvas.footprints {
        for (x, y) in footprint_cells_of(&good.compiled) {
            outline(cam, CellRect::brush(x, y, 1), 1.0, mq::Color::new(1.0, 1.0, 1.0, 0.35));
        }
    }
    if canvas.grid && cam.zoom >= 0.6 {
        grid(cam, good.compiled.scenario.width(), good.compiled.scenario.height());
    }
    if canvas.reach {
        for site in &canvas.unreached {
            hatch_site(cam, site);
        }
    }
    if let Some(spec) = spec {
        for m in &canvas.marks {
            for r in outline_of(&good.compiled, spec, &m.sel) {
                outline(cam, r, 2.0, mark_colour(m.severity));
            }
        }
    }
    if let (Some(spec), Some(sel)) = (spec, selection) {
        for r in outline_of(&good.compiled, spec, sel) {
            outline(cam, r, 2.0, accent());
        }
    }
    if let (Some(spec), Some(m)) = (spec, canvas.moving.as_ref()) {
        let (dx, dy) = (m.target.0 - m.origin.0, m.target.1 - m.origin.1);
        let colour = if inside(good, m.target) { mq::Color::new(1.0, 1.0, 1.0, 0.75) } else { mq::Color::new(0.9, 0.35, 0.35, 0.9) };
        for r in outline_of(&good.compiled, spec, &m.sel) {
            outline(cam, CellRect { x0: r.x0 + dx, y0: r.y0 + dy, x1: r.x1 + dx, y1: r.y1 + dy }, 2.0, colour);
        }
    }
    if let Some((x, y)) = canvas.hover {
        outline(cam, CellRect::brush(x, y, 1), 1.0, mq::Color::new(1.0, 1.0, 1.0, 0.4));
    }
    scissor(None);
}

/// The reach hatching of one unreached site: each of its cells crossed by two diagonals, red (an
/// area is hatched up to a few thousand cells: the rest of a huge area is not drawn).
fn hatch_site(cam: &Cam, site: &Site) {
    let red = mq::Color::new(0.9, 0.25, 0.25, 0.9);
    let cells: Vec<(i32, i32)> = match site.named {
        Named::Cell(c) => vec![c],
        Named::Area(x, y, w, h) => {
            let (x1, y1) = (x.saturating_add(w).min(x + 200), y.saturating_add(h).min(y + 200));
            (y..y1).flat_map(|yy| (x..x1).map(move |xx| (xx, yy))).take(4000).collect()
        }
    };
    for (x, y) in cells {
        let r = cam.rect(CellRect::brush(x, y, 1));
        mq::draw_line(r.x, r.y, r.x + r.w, r.y + r.h, 1.0, red);
        mq::draw_line(r.x + r.w, r.y, r.x, r.y + r.h, 1.0, red);
    }
}

/// The cell grid, the lines on the cell edges that are in view.
fn grid(cam: &Cam, w: u32, h: u32) {
    let line = mq::Color::new(1.0, 1.0, 1.0, 0.12);
    let tl = cam.to_cells(mq::vec2(cam.view.x, cam.view.y));
    let br = cam.to_cells(mq::vec2(cam.view.x + cam.view.w, cam.view.y + cam.view.h));
    let x0 = (tl.x.floor() as i32).max(0);
    let x1 = (br.x.ceil() as i32).min(w as i32);
    let y0 = (tl.y.floor() as i32).max(0);
    let y1 = (br.y.ceil() as i32).min(h as i32);
    for x in x0..=x1 {
        let a = cam.to_screen(mq::vec2(x as f32, y0 as f32));
        let b = cam.to_screen(mq::vec2(x as f32, y1 as f32));
        mq::draw_line(a.x, a.y, b.x, b.y, 1.0, line);
    }
    for y in y0..=y1 {
        let a = cam.to_screen(mq::vec2(x0 as f32, y as f32));
        let b = cam.to_screen(mq::vec2(x1 as f32, y as f32));
        mq::draw_line(a.x, a.y, b.x, b.y, 1.0, line);
    }
}

fn outline(cam: &Cam, r: CellRect, thick: f32, color: mq::Color) {
    let s = cam.rect(r);
    mq::draw_rectangle_lines(s.x, s.y, s.w, s.h, thick, color);
}

/// Cuts the drawing off outside `clip` (screen units), or lifts the cut (`None`).
fn scissor(clip: Option<mq::Rect>) {
    let dpi = mq::screen_dpi_scale();
    let gl = unsafe { mq::get_internal_gl() }.quad_gl;
    gl.scissor(clip.map(|r| ((r.x * dpi) as i32, (r.y * dpi) as i32, (r.w * dpi).ceil() as i32, (r.h * dpi).ceil() as i32)));
}
