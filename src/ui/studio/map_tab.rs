//! The Map tab: the map of the open package, drawn by the game's own map renderer
//! (`ui::studio::canvas`) into the tab's rectangle. egui allocates the rectangle and takes the
//! input (pan, zoom, click, drag, hover); the map is drawn with macroquad after the egui pass and
//! before egui's meshes, clipped to the rectangle, so nothing of it shows over another dock.
//!
//! The tools (the bar at the top of the tab), after DTMapEdit's pages (`docs/reference/editor/
//! main-window.md` §4). Select: a left press on the selected army, point, hero or building and a
//! drag moves it; a click selects. Army, Point and Object: a click places a table. Buildings: a
//! click places one (`Erase` removes the building under the cell). Terrain: a left drag paints
//! the palette's kind with the brush. Hills: a click puts one hill, its footprint the brush size.
//! Forests: a drag plants trees (one per cell, a family of the palette) and Replace re-rolls only
//! the trees in the way. Erase (hills, forests) takes out what the brush covers. Items: not in the
//! package format yet. Rectangle and Fill paint terrain. Copy takes a rectangle; Ctrl+V arms a
//! paste that follows the pointer. Move (a mode, any tool): a click picks a table up, the next
//! click drops it. Burn everything asks first.
//!
//! The overlays: the live placement errors and warnings as red and amber outlines (`Canvas::marks`),
//! the reach overlay, the Footprints and Grid lines, the Passable grid, the Patrols and the Fog of
//! the start (`studio::overlay`). Hovering a marked object or an unreached cell shows its messages.
//! The Minimap dock draws the whole map with the view frame (`Canvas::mini`).

use egui::{Align2, Color32, ComboBox, CornerRadius, FontId, PointerButton, ScrollArea, Sense, TextStyle, Ui};
use egui_macroquad::egui;
use macroquad::prelude as mq;
use razdor::i18n::tr;
use razdor::pack::compile::{default_footprint, ARMY_MODELS, BUILDING_NAMES, OBJECT_KINDS};
use razdor::pack::diag::Severity;
use razdor::pack::load::Package;
use razdor::pack::map::MapFile;
use razdor::pack::reach::{Named, Site};
use razdor::pack::terrain::DEFAULT_PALETTE;
use razdor::studio::brush::{self, Forest, BRUSH_SIZES};
use razdor::studio::edit::{Edit, Locator};
use razdor::studio::geometry::CellRect;
use razdor::studio::map::{self, footprint_cells_of, hit, map_ids, outline_of, Good, Mark, MapView, Selection};
use razdor::studio::overlay::{self, Layers};
use razdor::studio::region::{self, Clip, Stamp};
use razdor::studio::{Place, Workspace};
use razdor::trf;

use super::{Viewer, WARNING_AMBER};
use crate::ui::dt_art::DtArt;
use super::canvas::{draw_map, Cam, Overview};

/// The tools of the Map tab's bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Army,
    Point,
    Object,
    Building,
    Terrain,
    Hills,
    Forests,
    /// Items lying on the ground: the package format has no such table (`docs/OPEN_QUESTIONS.md`).
    Items,
    Rect,
    Fill,
    /// Copy: a drag takes a rectangle of the map (Ctrl+C copies it, Ctrl+V pastes it).
    Region,
}

impl Tool {
    /// Every tool, in the bar's order.
    pub const ALL: [Tool; 12] = [
        Tool::Select,
        Tool::Army,
        Tool::Point,
        Tool::Object,
        Tool::Building,
        Tool::Terrain,
        Tool::Hills,
        Tool::Forests,
        Tool::Items,
        Tool::Rect,
        Tool::Fill,
        Tool::Region,
    ];

    /// The tool's name in the bar (`tr`).
    pub fn title(self) -> &'static str {
        match self {
            Tool::Select => tr("Select"),
            Tool::Army => tr("Army"),
            Tool::Point => tr("Point"),
            Tool::Object => tr("Object"),
            Tool::Building => tr("Buildings"),
            Tool::Terrain => tr("Terrain"),
            Tool::Hills => tr("Hills"),
            Tool::Forests => tr("Forests"),
            Tool::Items => tr("Items"),
            Tool::Rect => tr("Rectangle"),
            Tool::Fill => tr("Fill"),
            Tool::Region => tr("Copy"),
        }
    }

    /// The tool named `name` in a snapshot option (`tool=terrain`).
    pub fn named(name: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| format!("{t:?}").eq_ignore_ascii_case(name))
    }

}

/// A move of a table: what it is, where it was, where the pointer's cell puts it now, the grab
/// offset (the pressed cell minus its origin), and the compile the move was applied before (`None`
/// while the table is still held or dragged).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Move {
    pub sel: Selection,
    pub origin: (i32, i32),
    pub target: (i32, i32),
    pub grab: (i32, i32),
    pub serial: Option<u64>,
}

/// A stroke of the forests or erase brush: its cells so far (one square per entry, inside the map).
#[derive(Clone, Debug, Default)]
struct Stroke {
    cells: Vec<(i64, i64)>,
}

/// The Map tab's view: the camera, the rectangle and the hovered cell, the tools and the overlays.
pub struct Canvas {
    cam: Option<Cam>,
    /// Fit the whole map on the next frame (the map was switched).
    pub fit: bool,
    /// A cell-unit point to centre the view on (a row of the Scene dock was clicked, the minimap).
    pub centre: Option<(f32, f32)>,
    /// The tab's rectangle in egui points, and the pixels per point it was laid out with.
    rect: Option<egui::Rect>,
    pub ppp: f32,
    hover: Option<(i32, i32)>,
    overview: Overview,
    overview_serial: u64,
    /// The move in progress, or the one applied and not compiled yet.
    pub moving: Option<Move>,
    /// The tool the bar picked.
    pub tool: Tool,
    /// The brush size of the terrain, hills and forests tools (`BRUSH_SIZES`).
    pub brush: u32,
    /// The terrain code the brush paints (an index into `pack::terrain::DEFAULT_PALETTE`).
    pub code: u8,
    /// The kinds the placing tools add: a building type, an army model, an object kind.
    pub building_kind: String,
    pub army_model: String,
    pub object_kind: String,
    /// The forest the forests tool plants.
    pub forest: Forest,
    /// Modes: Erase (hills, forests, buildings: DTMapEdit's Delete), tree replacement, ignore
    /// mountains (on: no warning when a building covers one), Move.
    pub erase: bool,
    pub replace_trees: bool,
    pub ignore_mountains: bool,
    pub move_mode: bool,
    /// The overlays' switches (the bar's toggles; not saved).
    pub grid: bool,
    pub footprints: bool,
    pub reach: bool,
    pub passable: bool,
    pub patrols: bool,
    pub fog: bool,
    /// Whether the tab was drawn in this frame (the snapshot's canvas is taken only then).
    pub shown: bool,
    /// The live diagnostics on the map's tables (`studio::map::marks`).
    pub marks: Vec<Mark>,
    /// The story cells the hero cannot reach on foot (`pack::reach`), of the map shown.
    pub unreached: Vec<Site>,
    /// The Copy tool's rectangle, the copied region, and whether Ctrl+V has armed a paste.
    pub region: Option<CellRect>,
    pub clip: Option<Clip>,
    pub paste: bool,
    /// The package's stamps (`None` until the Stamps dock lists them again).
    pub stamps: Option<Vec<Stamp>>,
    pub stamp_name: String,
    /// The Burn everything question is open.
    pub burn_asked: bool,
    /// The minimap's rectangle in egui points, when its dock is shown this frame.
    pub mini: Option<egui::Rect>,
    /// The forests or erase stroke in progress.
    stroke: Option<Stroke>,
    /// The rectangle tool's first cell while it is dragged.
    rect_from: Option<(i32, i32)>,
    /// The derived layers of the compile they were made for (`overlay::Layers`).
    layers: Option<(u64, Layers)>,
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
            forest: Forest::Family(0),
            erase: false,
            replace_trees: false,
            ignore_mountains: true,
            move_mode: false,
            grid: false,
            footprints: false,
            reach: false,
            passable: false,
            patrols: false,
            fog: false,
            shown: false,
            marks: Vec::new(),
            unreached: Vec::new(),
            region: None,
            clip: None,
            paste: false,
            stamps: None,
            stamp_name: String::new(),
            burn_asked: false,
            mini: None,
            stroke: None,
            rect_from: None,
            layers: None,
        }
    }
}

impl Canvas {
    /// The canvas's rectangle in pixels, when the tab is on screen this frame (a picture of it).
    pub fn shown_view(&self) -> Option<mq::Rect> {
        if !self.shown {
            return None;
        }
        self.cam.as_ref().map(|c| c.view)
    }
}

/// Shows map `id` on the Map tab: the camera fits it, and the selection goes.
pub fn switch_map(maps: &mut MapView, canvas: &mut Canvas, selection: &mut Option<Selection>, id: &str) {
    if maps.map.as_deref() != Some(id) {
        maps.show(id);
        canvas.fit = true;
        canvas.hover = None;
        canvas.moving = None;
        canvas.stroke = None;
        canvas.rect_from = None;
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

/// The cells of a brush of `size` centred on `(x, y)` (the brush: the extra cell goes
/// down and right on an even size, which the odd sizes do not have).
fn brush_cells(x: i32, y: i32, size: u32) -> Vec<(i64, i64)> {
    brush::square(i64::from(x), i64::from(y), size)
}

/// A deferred operation: decided while the canvas is borrowed, run after the match.
enum Act {
    /// A hill at the hovered cell, with the brush size.
    Hill((i64, i64), u32),
    /// A stroke of forests or of erase, its cells.
    Forests(Vec<(i64, i64)>),
    Erase(Vec<(i64, i64)>),
    /// A building under the cell is removed.
    BuildingErase((i64, i64)),
    /// A rectangle filled with the palette's kind.
    Rect((i32, i32), (i32, i32)),
    Fill((i64, i64)),
    /// The Copy tool's rectangle (`None`: the rectangle is cleared).
    Region(Option<CellRect>),
    /// Burn everything (confirmed).
    Burn,
    Paste((i64, i64)),
}

impl Viewer<'_> {
    /// The Map tab: the map picker and the tool bar, the stale banners, the canvas (its input here,
    /// its drawing in [`draw`]), and the hovered cell at the bottom left.
    pub(super) fn map_tab(&mut self, ui: &mut Ui) {
        self.map_picker(ui);
        self.tool_bar(ui);
        let burn = self.burn_question(ui.ctx());
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
        canvas.shown = true;
        canvas.ppp = ppp;
        // A click on the map picks what it hits, a placing click places, a painted cell is applied
        // to the shown compile after the match: all decided here, applied after it.
        let mut pick: Option<Option<Selection>> = None;
        let mut place_at: Option<(i32, i32)> = None;
        let mut painted: Vec<(u32, u32, u8)> = Vec::new();
        let mut paint_cells: Option<Vec<(i64, i64)>> = None;
        let mut paint_end = false;
        let mut act: Option<Act> = None;
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
                let press: Option<(i32, i32)> = ui.input(|i| i.pointer.press_origin()).map(|p| cam.cell_at(to_px(p)));
                let press64 = press.map(|(x, y)| (i64::from(x), i64::from(y)));

                let entry = pkg.as_ref().and_then(|p| p.maps.get(&good.map));
                let spec = entry.and_then(|m| m.spec.as_ref());
                if let (Some(spec), Some(cell)) = (spec, hover) {
                    tip = tip_of(&canvas.marks, canvas.reach, &canvas.unreached, &good.compiled, spec, cell);
                }
                let square_inside = |cell: (i64, i64), size: u32| CellRect::brush(cell.0 as i32, cell.1 as i32, size).inside(w, h);
                let hover64 = hover.map(|(x, y)| (i64::from(x), i64::from(y)));

                if canvas.paste && canvas.clip.is_some() {
                    // A paste armed with Ctrl+V: a click puts the clip's top left corner on the cell.
                    if resp.clicked_by(PointerButton::Primary) {
                        act = hover64.map(Act::Paste);
                    }
                } else if canvas.move_mode {
                    if let (Some(spec), true) = (spec, resp.clicked_by(PointerButton::Primary)) {
                        let holding = canvas.moving.as_ref().is_some_and(|m| m.serial.is_none());
                        if holding {
                            if let Some(mut m) = canvas.moving.take() {
                                if inside(good, m.target) && m.target != m.origin {
                                    match map::locator_of(&entry.map_or_else(String::new, |e| e.file.clone()), spec, &m.sel) {
                                        Some(at) => match self.ws.apply(razdor::studio::Edit::SetCell { at, cell: m.target }) {
                                            Ok(()) => {
                                                self.changed = true;
                                                m.serial = Some(good.serial);
                                                canvas.moving = Some(m);
                                            }
                                            Err(e) => self.log.push(e),
                                        },
                                        None => self.log.push(tr("This object has no table in the file").to_string()),
                                    }
                                }
                            }
                        } else if let Some(cell) = hover {
                            if let Some(sel) = pick_any(&good.compiled, spec, cell) {
                                if let Some([x, y]) = map::cell_of(spec, &sel) {
                                    let origin = (x as i32, y as i32);
                                    let grab = (cell.0 - origin.0, cell.1 - origin.1);
                                    canvas.moving = Some(Move { sel, origin, target: origin, grab, serial: None });
                                }
                            }
                        }
                    }
                    if let (Some(m), Some(c)) = (canvas.moving.as_mut(), hover) {
                        if m.serial.is_none() {
                            m.target = (c.0 - m.grab.0, c.1 - m.grab.1);
                        }
                    }
                } else {
                    match tool {
                        Tool::Select => {
                            if resp.drag_started_by(PointerButton::Primary) {
                                if let (Some(spec), Some(press)) = (spec, press) {
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
                                if let Some(c) = press {
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
                        Tool::Hills => {
                            if resp.clicked_by(PointerButton::Primary) {
                                if let Some(c) = hover64 {
                                    if canvas.erase {
                                        act = Some(Act::Erase(vec![c]));
                                    } else if square_inside(brush::hill_centre(c.0, c.1, canvas.brush), canvas.brush) {
                                        act = Some(Act::Hill(c, canvas.brush));
                                    } else {
                                        self.log.push(tr("The brush square reaches outside the map").to_string());
                                    }
                                }
                            }
                        }
                        Tool::Forests => {
                            // A stroke: each press or drag adds the brush square under the pointer
                            // (when it is inside the map), and the release applies the stroke as one step.
                            let size = canvas.brush;
                            let inside_map = |c: (i64, i64)| CellRect::brush(c.0 as i32, c.1 as i32, size).inside(w, h);
                            let add = |stroke: &mut Stroke, c: (i64, i64)| {
                                if inside_map(c) {
                                    for cell in brush_cells(c.0 as i32, c.1 as i32, size) {
                                        if !stroke.cells.contains(&cell) {
                                            stroke.cells.push(cell);
                                        }
                                    }
                                }
                            };
                            if resp.drag_started_by(PointerButton::Primary) {
                                let mut st = Stroke::default();
                                if let Some(c) = press64 {
                                    add(&mut st, c);
                                }
                                canvas.stroke = Some(st);
                            }
                            if resp.dragged_by(PointerButton::Primary) {
                                if let (Some(c), Some(st)) = (hover64, canvas.stroke.as_mut()) {
                                    add(st, c);
                                }
                            }
                            if resp.clicked_by(PointerButton::Primary) {
                                let mut st = Stroke::default();
                                if let Some(c) = hover64 {
                                    add(&mut st, c);
                                }
                                canvas.stroke = Some(st);
                            }
                            if resp.drag_stopped_by(PointerButton::Primary) || resp.clicked_by(PointerButton::Primary) {
                                if let Some(st) = canvas.stroke.take() {
                                    if !st.cells.is_empty() {
                                        act = Some(if canvas.erase { Act::Erase(st.cells) } else { Act::Forests(st.cells) });
                                    }
                                }
                            }
                        }
                        Tool::Building => {
                            if resp.clicked_by(PointerButton::Primary) {
                                if canvas.erase {
                                    act = hover64.map(Act::BuildingErase);
                                } else {
                                    place_at = hover;
                                }
                            }
                        }
                        Tool::Rect => {
                            if resp.drag_started_by(PointerButton::Primary) {
                                canvas.rect_from = press;
                            }
                            if resp.drag_stopped_by(PointerButton::Primary) {
                                if let (Some(a), Some(b)) = (canvas.rect_from.take(), hover) {
                                    act = Some(Act::Rect(a, b));
                                }
                            }
                        }
                        Tool::Fill => {
                            if resp.clicked_by(PointerButton::Primary) {
                                act = hover64.map(Act::Fill);
                            }
                        }
                        Tool::Region => {
                            if resp.drag_started_by(PointerButton::Primary) {
                                canvas.rect_from = press;
                            }
                            if resp.drag_stopped_by(PointerButton::Primary) {
                                if let (Some(a), Some(b)) = (canvas.rect_from.take(), hover) {
                                    act = Some(Act::Region(Some(CellRect::spanning(a, b))));
                                }
                            }
                            if resp.clicked_by(PointerButton::Primary) {
                                act = Some(Act::Region(None));
                            }
                        }
                        Tool::Items => {}
                        _ => {
                            if resp.clicked() {
                                place_at = hover;
                            }
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
        if let Some(a) = act {
            self.run(a);
        }
        if let Some(choice) = burn {
            if choice {
                self.run(Act::Burn);
            }
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

    /// The operations of the tools, run once the canvas's input is decided (the tool, the map
    /// shown). Each is one undo step.
    fn run(&mut self, act: Act) {
        let Some(map) = self.maps.map.clone() else { return };
        let file = format!("maps/{map}/map.toml");
        let size = self.maps.current().map(|g| (g.compiled.scenario.width(), g.compiled.scenario.height()));
        let Some((w, h)) = size else { return };
        let code = self.canvas.code;
        match act {
            Act::Hill(c, size) => {
                let centre = brush::hill_centre(c.0, c.1, size);
                if self.on_water(centre) {
                    self.log.push(tr("A hill stands on land: the centre is on water").to_string());
                    return;
                }
                let text = self.text(&file);
                let edits = brush::hill_edits(&file, &text, centre, size);
                self.commit("hills", &map, edits, Vec::new());
            }
            Act::Forests(cells) => {
                let (land, water): (Vec<(i64, i64)>, Vec<(i64, i64)>) = cells.into_iter().partition(|&c| !self.on_water(c));
                if !water.is_empty() {
                    self.log.push(trf!("{n} cells on water kept free: trees stand on land", n = water.len()));
                }
                if land.is_empty() {
                    return;
                }
                let cells = land;
                let text = self.text(&file);
                let (forest, replace, size) = (self.canvas.forest, self.canvas.replace_trees, self.canvas.brush);
                let edits = brush::forest_edits(&file, &text, &cells, forest, replace, size);
                self.commit("forests", &map, edits, Vec::new());
            }
            Act::Erase(cells) => {
                let inside: Vec<(i64, i64)> = cells.into_iter().filter(|&(x, y)| x >= 0 && y >= 0 && x < i64::from(w) && y < i64::from(h)).collect();
                let text = self.text(&file);
                let mut edits = brush::plant_erase_edits(&file, &text, &inside);
                // Hills: the cells' hill covers are taken with the plants (DT's delete on the hills page
                // takes the hill whose footprint covers the cell; the forests page takes the plant).
                if self.canvas.tool == Tool::Hills {
                    edits = inside.iter().flat_map(|&c| brush::hill_erase_edits(&file, &text, c)).collect();
                }
                self.commit("erase", &map, edits, Vec::new());
            }
            Act::BuildingErase(c) => {
                let ids: Vec<String> = match self.maps.current() {
                    Some(good) => brush::building_erase_ids(&buildings_of(good), (c.0 as i32, c.1 as i32)),
                    None => Vec::new(),
                };
                if ids.is_empty() {
                    self.log.push(tr("No building covers that cell").to_string());
                    return;
                }
                let edits = ids.into_iter().map(|id| Edit::Remove { at: Locator { file: file.clone(), kind: razdor::studio::Kind::Building, id: Some(id), index: 0 } }).collect();
                self.commit("delete building", &map, edits, Vec::new());
            }
            Act::Rect(a, b) => {
                let cells: Vec<(i64, i64, u8)> = brush::rect_cells(a, b, w, h).into_iter().map(|(x, y)| (x, y, code)).collect();
                if !cells.is_empty() {
                    self.commit("fill rectangle", &map, Vec::new(), cells);
                }
            }
            Act::Fill(c) => {
                let Ok(i) = self.ws.open_terrain(&map) else { return };
                let t = &self.ws.terrains[i].terrain;
                let region = brush::flood(&t.cells, w, h, (c.0 as u32, c.1 as u32));
                let want = t.cells[(c.1 as u32 * w + c.0 as u32) as usize];
                if want == code {
                    return;
                }
                let cells: Vec<(i64, i64, u8)> = region.into_iter().map(|(x, y)| (i64::from(x), i64::from(y), code)).collect();
                self.commit("fill", &map, Vec::new(), cells);
            }
            Act::Region(rect) => {
                self.canvas.region = rect;
                self.canvas.paste = false;
            }
            Act::Paste(at) => {
                let Some(clip) = self.canvas.clip.clone() else { return };
                match region::paste(self.ws, &map, &clip, at) {
                    Ok(n) => {
                        self.log.push(trf!("Pasted {n} objects", n = n));
                        self.changed = true;
                        let painted: Vec<(u32, u32, u8)> = (0..clip.h)
                            .flat_map(|y| (0..clip.w).map(move |x| (x, y)))
                            .filter_map(|(x, y)| {
                                let (tx, ty) = (at.0 + i64::from(x), at.1 + i64::from(y));
                                (tx >= 0 && ty >= 0 && tx < i64::from(w) && ty < i64::from(h)).then(|| (tx as u32, ty as u32, clip.codes[(y * clip.w + x) as usize]))
                            })
                            .collect();
                        self.maps.repaint(&painted);
                    }
                    Err(e) => self.log.push(e),
                }
            }
            Act::Burn => {
                let text = self.text(&file);
                let edits = brush::burn_tree_edits(&file, &text);
                let Ok(i) = self.ws.open_terrain(&map) else { return };
                let cells = brush::burn_cells(&self.ws.terrains[i].terrain.cells, w);
                self.commit("burn everything", &map, edits, cells);
            }
        }
    }

    /// Whether a cell of the shown map is water (the package's rule: no object stands on it).
    fn on_water(&self, (x, y): (i64, i64)) -> bool {
        let Some(good) = self.maps.current() else { return false };
        let s = &good.compiled.scenario;
        x >= 0 && y >= 0 && x < i64::from(s.width()) && y < i64::from(s.height()) && s.terrain_at(x as u32, y as u32).is_some_and(|c| c <= 2)
    }

    /// The text of a package file as the studio has it (the open document's, else the disk's).
    fn text(&self, file: &str) -> String {
        self.ws.text_of(file).unwrap_or_default()
    }

    /// Applies a tool's edits and terrain cells as one undo step, and logs a failure.
    fn commit(&mut self, label: &str, map: &str, edits: Vec<Edit>, cells: Vec<(i64, i64, u8)>) {
        let painted: Vec<(u32, u32, u8)> = cells.iter().filter(|c| c.0 >= 0 && c.1 >= 0).map(|&(x, y, c)| (x as u32, y as u32, c)).collect();
        match brush::commit(self.ws, label, map, edits, cells) {
            Ok(()) => {
                self.changed = true;
                if !painted.is_empty() {
                    self.maps.repaint(&painted);
                }
            }
            Err(e) => self.log.push(e),
        }
    }

    /// Saves the Copy tool's rectangle as a stamp under the name typed in the bar.
    fn save_stamp(&mut self) {
        let Some(map) = self.maps.map.clone() else { return };
        let Some(rect) = self.canvas.region else { return };
        let name = self.canvas.stamp_name.trim().to_string();
        let result = region::copy(self.ws, &map, rect).and_then(|clip| region::save_stamp(&self.ws.root, &name, &clip));
        match result {
            Ok(()) => {
                self.log.push(trf!("Saved the stamp {name}", name = name.as_str()));
                self.canvas.stamps = None;
            }
            Err(e) => self.log.push(e),
        }
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
            _ => return,
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
        // A building over a mountain is placed all the same: the warning is the only check (DT §14.2).
        if tool == Tool::Building && !self.canvas.ignore_mountains {
            let size = default_footprint(&building).unwrap_or((1, 1));
            let text = self.text(&file);
            if brush::covers_mountains(&text, (i64::from(x), i64::from(y)), size) {
                self.log.push(tr("Warning: the building covers a mountain (placed anyway)").to_string());
            }
        }
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

    /// The question of Burn everything: `Some(true)` when it is confirmed this frame.
    fn burn_question(&mut self, ctx: &egui::Context) -> Option<bool> {
        if !self.canvas.burn_asked {
            return None;
        }
        let mut answer = None;
        egui::Window::new(tr("Burn everything?")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(tr("Forests become dead trees and the ground above water becomes scorched land. One undo step."));
            ui.horizontal(|ui| {
                if ui.button(tr("Burn")).clicked() {
                    answer = Some(true);
                }
                if ui.button(tr("Cancel")).clicked() {
                    answer = Some(false);
                }
            });
        });
        if answer.is_some() {
            self.canvas.burn_asked = false;
        }
        answer
    }

    /// The bar of tools, the modes and the overlay switches, under the map picker.
    fn tool_bar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            for tool in Tool::ALL {
                let selected = self.canvas.tool == tool;
                if tool == Tool::Items {
                    ui.add_enabled(false, egui::Button::new(tool.title())).on_disabled_hover_text(tr("not in the package format yet"));
                } else if ui.selectable_label(selected, tool.title()).clicked() {
                    self.canvas.tool = tool;
                    self.canvas.paste = false;
                }
            }
            ui.separator();
            if ui.selectable_label(self.canvas.move_mode, tr("Move")).on_hover_text(tr("A click picks a table up, the next click drops it")).clicked() {
                self.canvas.move_mode = !self.canvas.move_mode;
                self.canvas.moving = None;
            }
            if ui.button(tr("Burn everything")).clicked() {
                self.canvas.burn_asked = true;
            }
        });
        ui.horizontal_wrapped(|ui| {
            let tool = self.canvas.tool;
            match tool {
                Tool::Building => {
                    ComboBox::from_id_salt("studio-tool-building").selected_text(self.canvas.building_kind.clone()).show_ui(ui, |ui| {
                        for name in BUILDING_NAMES {
                            if ui.selectable_label(self.canvas.building_kind == name, name).clicked() {
                                self.canvas.building_kind = name.to_string();
                            }
                        }
                    });
                    ui.checkbox(&mut self.canvas.erase, tr("Erase")).on_hover_text(tr("A click removes the building that covers the cell"));
                    ui.checkbox(&mut self.canvas.ignore_mountains, tr("Ignore mountains")).on_hover_text(tr("Off: a building over a mountain is warned about"));
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
                Tool::Hills | Tool::Forests => {
                    ui.label(tr("Brush"));
                    for size in BRUSH_SIZES {
                        if ui.selectable_label(self.canvas.brush == size, format!("{size}×{size}")).clicked() {
                            self.canvas.brush = size;
                        }
                    }
                    ui.checkbox(&mut self.canvas.erase, tr("Erase")).on_hover_text(tr("Delete: the hills or plants the brush covers"));
                    if tool == Tool::Forests {
                        ComboBox::from_id_salt("studio-forest").selected_text(forest_label(self.canvas.forest)).show_ui(ui, |ui| {
                            for f in (0..10).map(Forest::Family).chain(std::iter::once(Forest::Thicket)) {
                                if ui.selectable_label(self.canvas.forest == f, forest_label(f)).clicked() {
                                    self.canvas.forest = f;
                                }
                            }
                        });
                        ui.checkbox(&mut self.canvas.replace_trees, tr("Replace trees")).on_hover_text(tr("Only the trees and thickets the chosen family replaces are re-rolled"));
                    }
                }
                Tool::Terrain => {
                    ui.label(tr("Brush"));
                    for size in BRUSH_SIZES {
                        if ui.selectable_label(self.canvas.brush == size, format!("{size}×{size}")).clicked() {
                            self.canvas.brush = size;
                        }
                    }
                }
                Tool::Region => {
                    let has_region = self.canvas.region.is_some();
                    if ui.add_enabled(has_region, egui::Button::new(tr("Copy"))).on_hover_text(tr("Ctrl+C copies the rectangle")).clicked() {
                        copy_region(self.ws, self.maps, self.canvas, self.log);
                    }
                    if ui.add_enabled(self.canvas.clip.is_some(), egui::Button::new(tr("Paste"))).on_hover_text(tr("Ctrl+V: the clip follows the pointer; Esc stops")).clicked() {
                        arm_paste(self.canvas, self.log);
                    }
                    ui.add(egui::TextEdit::singleline(&mut self.canvas.stamp_name).hint_text(tr("Stamp name")).desired_width(120.0));
                    let name_ok = region::valid_name(&self.canvas.stamp_name);
                    if ui.add_enabled(has_region && name_ok, egui::Button::new(tr("Save stamp"))).clicked() {
                        self.save_stamp();
                    }
                    if ui.add_enabled(has_region, egui::Button::new(tr("Clear"))).clicked() {
                        self.canvas.region = None;
                    }
                }
                Tool::Rect => {
                    ui.weak(tr("Fills the rectangle with the palette's kind below"));
                }
                Tool::Fill => {
                    ui.weak(tr("Fills the connected cells of the clicked kind with the palette's kind below"));
                }
                Tool::Select | Tool::Point | Tool::Items => {}
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.canvas.reach, tr("Reach")).on_hover_text(tr("Cells the hero cannot walk to, on the map's story"));
            ui.checkbox(&mut self.canvas.grid, tr("Grid"));
            ui.checkbox(&mut self.canvas.passable, tr("Passable")).on_hover_text(tr("The walk grid: normal, hard, harder, impassable, building, water"));
            ui.checkbox(&mut self.canvas.patrols, tr("Patrols")).on_hover_text(tr("The patrol loops of the army goals in the triggers"));
            ui.checkbox(&mut self.canvas.fog, tr("Fog")).on_hover_text(tr("What the hero and the lit lanterns see at the start"));
            ui.checkbox(&mut self.canvas.footprints, tr("Footprints"));
        });
        if matches!(self.canvas.tool, Tool::Terrain | Tool::Rect | Tool::Fill) {
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

    /// The Minimap dock: the whole map small, the view frame, and a click or drag moves the view.
    pub(super) fn minimap(&mut self, ui: &mut Ui) {
        let Some(good) = self.maps.current() else {
            ui.weak(tr("Nothing to show: the map does not compile."));
            return;
        };
        let (w, h) = (good.compiled.scenario.width() as f32, good.compiled.scenario.height() as f32);
        let avail = ui.available_size();
        let k = (avail.x / w).min(avail.y / h).max(1.0);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w * k, h * k), Sense::click_and_drag());
        self.canvas.mini = Some(rect);
        if resp.clicked() || resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.canvas.centre = Some(((p.x - rect.min.x) / k, (p.y - rect.min.y) / k));
            }
        }
    }

    /// The Stamps dock: the package's saved regions, each with a small picture; Paste arms it.
    pub(super) fn stamps_dock(&mut self, ui: &mut Ui) {
        if self.canvas.stamps.is_none() {
            self.canvas.stamps = Some(region::list_stamps(&self.ws.root));
        }
        let stamps = self.canvas.stamps.clone().unwrap_or_default();
        ui.weak(tr("Copy a rectangle, name it and save it: the stamps of this package"));
        if stamps.is_empty() {
            ui.weak(tr("No stamps yet."));
            return;
        }
        let mut armed: Option<Stamp> = None;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for stamp in &stamps {
                ui.horizontal(|ui| {
                    stamp_picture(ui, &stamp.clip);
                    ui.vertical(|ui| {
                        ui.strong(stamp.name.as_str());
                        ui.weak(format!("{}×{}", stamp.clip.w, stamp.clip.h));
                        if ui.button(tr("Paste")).clicked() {
                            armed = Some(stamp.clone());
                        }
                    });
                });
            }
        });
        if let Some(stamp) = armed {
            self.canvas.clip = Some(stamp.clip);
            self.canvas.paste = true;
            self.canvas.tool = Tool::Region;
            self.focus_map = true;
        }
    }

}

/// The picture of a stamp's terrain in a dock row (its cells in the palette's colours).
fn stamp_picture(ui: &mut Ui, clip: &Clip) {
    // One square per cell, the picture at most 120 cells on a side.
    let k = (120.0 / clip.w.max(clip.h).max(1) as f32).clamp(1.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(clip.w as f32 * k, clip.h as f32 * k), Sense::hover());
    let painter = ui.painter_at(rect);
    for y in 0..clip.h {
        for x in 0..clip.w {
            let code = clip.codes[(y * clip.w + x) as usize] as usize;
            let rgb = DEFAULT_PALETTE[code.min(15)].1;
            let cell = egui::Rect::from_min_size(rect.min + egui::vec2(x as f32 * k, y as f32 * k), egui::vec2(k, k));
            painter.rect_filled(cell, CornerRadius::ZERO, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        }
    }
}

/// Copies the Copy tool's rectangle into the clip (Ctrl+C), and says so.
pub(super) fn copy_region(ws: &mut Workspace, maps: &MapView, canvas: &mut Canvas, log: &mut Vec<String>) {
    let Some(map) = maps.map.clone() else { return };
    let Some(rect) = canvas.region else {
        log.push(tr("Drag a rectangle with the Copy tool first").to_string());
        return;
    };
    match region::copy(ws, &map, rect) {
        Ok(clip) => {
            log.push(trf!("Copied {w}×{h} cells", w = clip.w, h = clip.h));
            canvas.clip = Some(clip);
            canvas.paste = false;
        }
        Err(e) => log.push(e),
    }
}

/// Arms the paste (Ctrl+V): the clip follows the pointer until Esc or another copy.
pub(super) fn arm_paste(canvas: &mut Canvas, log: &mut Vec<String>) {
    if canvas.clip.is_some() {
        canvas.paste = true;
        canvas.tool = Tool::Region;
    } else {
        log.push(tr("Nothing copied yet: Ctrl+C copies the Copy tool's rectangle").to_string());
    }
}

/// The label of a forest choice in the bar: `Family 3` or `Thicket`.
fn forest_label(f: Forest) -> String {
    match f {
        Forest::Family(n) => format!("{} {n}", tr("Family")),
        Forest::Thicket => tr("Thicket").to_string(),
    }
}

/// The buildings of the compiled map as `(id, anchor, footprint size)`.
fn buildings_of(good: &Good) -> Vec<brush::BuildingFootprint> {
    let s = &good.compiled.scenario;
    good.compiled
        .names
        .buildings
        .iter()
        .filter_map(|(id, n)| {
            let b = s.buildings.get(*n as usize - 1)?;
            Some((id.clone(), (i32::from(b.x), i32::from(b.y)), (i32::from(b.size_x.max(1)), i32::from(b.size_y.max(1)))))
        })
        .collect()
}

/// Whether a cell is on the map (a move's target).
fn inside(good: &Good, (x, y): (i32, i32)) -> bool {
    let s = &good.compiled.scenario;
    x >= 0 && y >= 0 && (x as u32) < s.width() && (y as u32) < s.height()
}

/// What a click of the move mode picks up under a cell: an army, a building, the hero, a point,
/// then a terrain object (DTMapEdit's order: army, building, point).
fn pick_any(compiled: &razdor::pack::compile::Compiled, spec: &MapFile, cell: (i32, i32)) -> Option<Selection> {
    if let Some(sel) = hit(compiled, spec, cell) {
        return Some(sel);
    }
    let at = |c: &[u32; 2]| c[0] as i32 == cell.0 && c[1] as i32 == cell.1;
    if let Some(i) = spec.point.iter().position(|p| at(&p.get_ref().cell)) {
        return Some(Selection::Point(i));
    }
    spec.object.iter().position(|o| at(&o.get_ref().cell)).map(Selection::Object)
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
fn tip_of(marks: &[Mark], reach: bool, unreached: &[Site], compiled: &razdor::pack::compile::Compiled, spec: &MapFile, cell: (i32, i32)) -> Option<String> {
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

/// The colour of a walk-grid tile (`overlay::grid`); normal ground is not tinted.
fn grid_colour(kind: u8) -> Option<mq::Color> {
    match kind {
        1 => Some(mq::Color::new(0.91, 0.82, 0.31, 0.22)),
        2 => Some(mq::Color::new(0.91, 0.56, 0.31, 0.35)),
        3 => Some(mq::Color::new(0.88, 0.25, 0.25, 0.50)),
        4 => Some(mq::Color::new(0.50, 0.50, 0.56, 0.55)),
        5 => Some(mq::Color::new(0.43, 0.72, 0.88, 0.30)),
        6 => Some(mq::Color::new(0.16, 0.35, 0.63, 0.45)),
        _ => None,
    }
}

/// Draws the shown map into the Map tab's rectangle: the terrain and the objects, the overlays
/// (Footprints, Grid, the Passable grid, the Patrols, the Fog, the reach hatching, the diagnostics'
/// outlines), the selection, the move's ghost, the paste ghost and the hovered cell. Runs after the
/// egui pass and before egui is drawn over it. The minimap's picture and frame are drawn here too.
pub fn draw(canvas: &mut Canvas, maps: &MapView, pkg: Option<&Package>, selection: Option<&Selection>, art: Option<&DtArt>) {
    let Some(good) = maps.current() else { return };
    if canvas.overview_serial != good.serial {
        canvas.overview = Overview::default();
        canvas.overview_serial = good.serial;
    }
    let tex = canvas.overview.texture(&good.compiled.scenario, good.serial);
    let spec = pkg.and_then(|p| p.maps.get(&good.map)).and_then(|m| m.spec.as_ref());
    if let (Some(mini), Some(tex)) = (canvas.mini, tex.as_ref()) {
        draw_minimap(canvas, mini, tex, good);
    }
    let (Some(_), Some(cam)) = (canvas.rect, canvas.cam.as_mut()) else { return };
    if canvas.layers.as_ref().is_none_or(|(serial, _)| *serial != good.serial) {
        let hero = spec.map_or((0, 0), |s| (i64::from(s.hero.cell[0]), i64::from(s.hero.cell[1])));
        let patrols = pkg.map(|p| overlay::patrols(&p.triggers, &good.map)).unwrap_or_default();
        canvas.layers = Some((good.serial, Layers::of(&good.compiled.scenario, hero, patrols)));
    }
    let layers = canvas.layers.as_ref().map(|(_, l)| l);
    scissor(Some(cam.view));
    draw_map(&good.compiled.scenario, art, cam, tex.as_ref());
    let (w, h) = (good.compiled.scenario.width(), good.compiled.scenario.height());
    let visible = cam.visible(w, h);
    if let (Some(layers), Some(vis)) = (layers, visible) {
        if canvas.passable {
            for y in vis.y0..=vis.y1 {
                for x in vis.x0..=vis.x1 {
                    if let Some(c) = grid_colour(layers.grid[(y as u32 * w + x as u32) as usize]) {
                        let r = cam.rect(CellRect::brush(x, y, 1));
                        mq::draw_rectangle(r.x, r.y, r.w, r.h, c);
                    }
                }
            }
        }
        if canvas.fog {
            for y in vis.y0..=vis.y1 {
                for x in vis.x0..=vis.x1 {
                    if !layers.revealed[(y as u32 * w + x as u32) as usize] {
                        let r = cam.rect(CellRect::brush(x, y, 1));
                        mq::draw_rectangle(r.x, r.y, r.w, r.h, mq::Color::new(0.0, 0.0, 0.0, 0.55));
                    }
                }
            }
        }
    }
    if canvas.footprints {
        for (x, y) in footprint_cells_of(&good.compiled) {
            outline(cam, CellRect::brush(x, y, 1), 1.0, mq::Color::new(1.0, 1.0, 1.0, 0.35));
        }
    }
    if canvas.grid && cam.zoom >= 0.6 {
        grid(cam, w, h);
    }
    if let (true, Some(layers)) = (canvas.patrols, layers) {
        for p in &layers.patrols {
            patrol(cam, &p.army, &p.cells);
        }
    }
    if let Some(spec) = spec {
        hero_marker(cam, spec.hero.cell);
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
    if let Some(region) = canvas.region {
        outline(cam, region, 2.0, accent());
    }
    if let (Some(from), Some(to)) = (canvas.rect_from, canvas.hover) {
        outline(cam, CellRect::spanning(from, to), 2.0, mq::Color::new(1.0, 1.0, 1.0, 0.7));
    }
    if let (true, Some(clip), Some((x, y))) = (canvas.paste, canvas.clip.as_ref(), canvas.hover) {
        for cy in 0..clip.h as i32 {
            for cx in 0..clip.w as i32 {
                let code = clip.codes[(cy as u32 * clip.w + cx as u32) as usize].min(15) as usize;
                let rgb = DEFAULT_PALETTE[code].1;
                let r = cam.rect(CellRect::brush(x + cx, y + cy, 1));
                mq::draw_rectangle(r.x, r.y, r.w, r.h, mq::Color::from_rgba(rgb[0], rgb[1], rgb[2], 150));
            }
        }
        let r = CellRect { x0: x, y0: y, x1: x + clip.w as i32 - 1, y1: y + clip.h as i32 - 1 };
        outline(cam, r, 2.0, mq::Color::new(1.0, 1.0, 1.0, 0.9));
    }
    if let Some((x, y)) = canvas.hover {
        let size = match canvas.tool {
            Tool::Terrain | Tool::Hills | Tool::Forests => canvas.brush,
            _ => 1,
        };
        let r = CellRect::brush(x, y, size);
        outline(cam, r, 1.0, mq::Color::new(1.0, 1.0, 1.0, 0.4));
    }
    scissor(None);
}

/// The minimap: the terrain picture stretched over its rectangle, the view frame in accent.
fn draw_minimap(canvas: &Canvas, mini: egui::Rect, tex: &mq::Texture2D, good: &Good) {
    let ppp = canvas.ppp;
    let r = mq::Rect::new(mini.min.x * ppp, mini.min.y * ppp, mini.width() * ppp, mini.height() * ppp);
    let (w, h) = (good.compiled.scenario.width() as f32, good.compiled.scenario.height() as f32);
    scissor(Some(r));
    mq::draw_texture_ex(tex, r.x, r.y, mq::WHITE, mq::DrawTextureParams { dest_size: Some(mq::vec2(r.w, r.h)), ..Default::default() });
    if let Some(cam) = canvas.cam.as_ref() {
        let tl = cam.to_cells(mq::vec2(cam.view.x, cam.view.y));
        let br = cam.to_cells(mq::vec2(cam.view.x + cam.view.w, cam.view.y + cam.view.h));
        let (kx, ky) = (r.w / w, r.h / h);
        mq::draw_rectangle_lines(r.x + tl.x.max(0.0) * kx, r.y + tl.y.max(0.0) * ky, (br.x - tl.x) * kx, (br.y - tl.y) * ky, 2.0, accent());
    }
    scissor(None);
}

/// A patrol: the loop through its cells (the goal's order, closed) and a dot on each cell.
fn patrol(cam: &Cam, army: &str, cells: &[(i32, i32)]) {
    let colour = mq::Color::new(0.95, 0.75, 0.30, 0.9);
    let centres: Vec<mq::Vec2> = cells.iter().map(|&(x, y)| cam.to_screen(mq::vec2(x as f32 + 0.5, y as f32 + 0.5))).collect();
    for pair in centres.windows(2) {
        mq::draw_line(pair[0].x, pair[0].y, pair[1].x, pair[1].y, 2.0, colour);
    }
    if let (Some(first), Some(last)) = (centres.first(), centres.last()) {
        if centres.len() > 2 {
            mq::draw_line(last.x, last.y, first.x, first.y, 2.0, colour);
        }
        mq::draw_text(army, first.x + 4.0, first.y - 4.0, 16.0, colour);
    }
    for c in &centres {
        mq::draw_circle(c.x, c.y, 3.0, colour);
    }
}

/// The hero's start: a gold disc with an H on his cell. The world view draws his figure, which needs
/// a running game; the studio marks the cell instead (the Select tool moves it like an army).
fn hero_marker(cam: &Cam, cell: [u32; 2]) {
    let r = cam.rect(CellRect::brush(cell[0] as i32, cell[1] as i32, 1));
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let radius = (r.w.min(r.h) * 0.42).max(3.0);
    mq::draw_circle(cx, cy, radius, mq::Color::from_rgba(0xe8, 0xc0, 0x4f, 255));
    mq::draw_circle_lines(cx, cy, radius, 1.5, mq::Color::from_rgba(0x20, 0x18, 0x08, 255));
    let size = (radius * 1.4).round().max(8.0) as u16;
    let dims = mq::measure_text("H", None, size, 1.0);
    mq::draw_text("H", cx - dims.width / 2.0, cy + dims.height / 2.0, f32::from(size), mq::Color::from_rgba(0x20, 0x18, 0x08, 255));
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
