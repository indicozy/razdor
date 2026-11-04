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

use std::collections::{BTreeMap, BTreeSet};

use egui::{Align2, Color32, ComboBox, CornerRadius, FontId, PointerButton, RichText, ScrollArea, Sense, TextStyle, Ui};
use egui_phosphor::regular as ic;
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
use razdor::studio::layers;
use razdor::studio::geometry::CellRect;
use razdor::studio::map::{self, footprint_cells_of, footprint_rects, hit, map_ids, outline_of, Good, Mark, MapView, Selection};
use razdor::studio::overlay::{self, Layers};
use razdor::studio::region::{self, Clip, Stamp};
use razdor::studio::{Place, Workspace};
use razdor::trf;

use super::{Viewer, WARNING_AMBER};
use crate::ui::dt_art::DtArt;
use super::canvas::{draw_map_layers, Cam, Overview};

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
    /// The eraser (E): a stroke takes the plants under the brush, the hills when no plant is there.
    Eraser,
    /// The eyedropper (I, or Alt while painting): a click takes the cell's terrain.
    Eyedrop,
    /// Stamp (S): a click places the armed stamp (the Штампы tab arms one).
    Stamp,
    /// Hand (H): a drag pans the view.
    Hand,
}

/// A list of cells, as the Map tab's tools partition them.
type Cells = Vec<(i64, i64)>;

/// A palette choice: a terrain kind, the hills, a forest family or a thicket, or a decorative object
/// kind. The brush tool paints what the last choice names (`Pick::apply`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Terrain(u8),
    Hills,
    Forest(Forest),
    Object(&'static str),
}

impl Pick {
    /// The tool the choice works with.
    pub fn tool(self) -> Tool {
        match self {
            Pick::Terrain(_) => Tool::Terrain,
            Pick::Hills => Tool::Hills,
            Pick::Forest(_) => Tool::Forests,
            Pick::Object(_) => Tool::Object,
        }
    }

    /// Makes the choice the current one: its tool, and the terrain code, forest or object kind it names.
    pub fn apply(self, canvas: &mut Canvas) {
        canvas.tool = self.tool();
        canvas.paste = false;
        match self {
            Pick::Terrain(code) => canvas.code = code,
            Pick::Hills => {}
            Pick::Forest(f) => canvas.forest = f,
            Pick::Object(kind) => canvas.object_kind = kind.to_string(),
        }
    }
}

impl Tool {
    /// Every tool, in the bar's order.
    pub const ALL: [Tool; 16] = [
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
        Tool::Eraser,
        Tool::Eyedrop,
        Tool::Stamp,
        Tool::Hand,
    ];

    /// Whether the tool places one table at a click (its ghost follows the pointer, Esc cancels).
    pub fn places(self) -> bool {
        matches!(self, Tool::Army | Tool::Building | Tool::Point)
    }

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
            Tool::Eraser => tr("Eraser"),
            Tool::Eyedrop => tr("Eyedropper"),
            Tool::Stamp => tr("Stamp"),
            Tool::Hand => tr("Hand"),
        }
    }

    /// The tool named `name` in a snapshot option (`tool=terrain`).
    pub fn named(name: &str) -> Option<Tool> {
        // `brush` and `forest` are the names the tool strip gives the paint tools (Terrain and Forests).
        let name = match name {
            "brush" => "terrain",
            "forest" => "forests",
            other => other,
        };
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

/// A stroke of the forests or erase brush: its cells so far (one per entry, inside the map), and the
/// set of them, so a cell is listed once.
#[derive(Clone, Debug, Default)]
struct Stroke {
    cells: Vec<(i64, i64)>,
    seen: BTreeSet<(i64, i64)>,
}

/// The Map tab's view: the camera, the rectangle and the hovered cell, the tools and the overlays.
pub struct Canvas {
    cam: Option<Cam>,
    /// Fit the whole map on the next frame (the map was switched).
    pub fit: bool,
    /// A cell-unit point to centre the view on (a row of the Scene dock was clicked, the minimap).
    pub centre: Option<(f32, f32)>,
    /// The tab's rectangle in egui points, and the pixels per point it was laid out with.
    pub(super) rect: Option<egui::Rect>,
    pub ppp: f32,
    pub(super) hover: Option<(i32, i32)>,
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
    /// The cells a Scores line selects (`scores.rs`), outlined on the canvas (not saved).
    pub score_cells: Vec<(i64, i64)>,
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
    /// The AI viewer («Просмотр ИИ»): its mode, its run and the day the slider shows.
    pub ai: super::aiview::AiView,
    /// The brush's shape (round or square) and the scatter density in per cent (objects).
    pub shape: brush::Shape,
    pub density: u32,
    /// The layers of each map, keyed by the map's id (`studio::layers`; not saved).
    pub layer_sets: layers::PerMap,
    /// The last cell a brush stroke or a Shift line starts from; the stroke's scatter seed.
    pub paint_last: Option<(i64, i64)>,
    pub stroke_seed: u64,
    /// The palette's last choice and the one before it (X swaps them).
    pub pick: Pick,
    pub pick_prev: Pick,
    /// What the status bar shows for the hovered cell: its terrain's name and the object under it.
    pub hover_info: Option<(String, Option<String>)>,
    /// The palette's object thumbnails: the install's sprite as pixels (made once per kind) and the
    /// texture the frame loads from them. Nothing is written to disk.
    pub thumb_src: BTreeMap<&'static str, Option<egui::ColorImage>>,
    pub thumb_tex: BTreeMap<&'static str, egui::TextureHandle>,
    /// The palette's search text.
    pub palette_query: String,
    /// The snapshot's hovered cell (`hover=x,y`), when no pointer is over the canvas.
    pub snap_hover: Option<(i32, i32)>,
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
            score_cells: Vec::new(),
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
            layer_sets: Default::default(),
            ai: super::aiview::AiView::default(),
            shape: brush::Shape::Square,
            density: 100,
            paint_last: None,
            stroke_seed: 1,
            pick: Pick::Terrain(6),
            pick_prev: Pick::Hills,
            hover_info: None,
            thumb_src: BTreeMap::new(),
            thumb_tex: BTreeMap::new(),
            palette_query: String::new(),
            snap_hover: None,
        }
    }
}

impl Canvas {
    /// Makes the palette's thumbnails from the install's sprites, once per object kind: the sprite's
    /// pixels, kept in memory only. Without an install nothing is made (the palette shows the icon).
    pub fn prepare_thumbs(&mut self, art: Option<&DtArt>) {
        let Some(art) = art else { return };
        for &(kind, class, sprite) in OBJECT_KINDS.iter() {
            if self.thumb_src.contains_key(kind) {
                continue;
            }
            let image = art.object(class, sprite).map(|tex| {
                let data = tex.get_texture_data();
                egui::ColorImage::from_rgba_unmultiplied([usize::from(data.width), usize::from(data.height)], &data.bytes)
            });
            self.thumb_src.insert(kind, image);
        }
    }

    /// Zooms the view by `factor` about its centre (the zoom commands).
    pub fn zoom_by(&mut self, factor: f32) {
        if let Some(cam) = self.cam.as_mut() {
            let v = cam.view;
            cam.zoom_at(factor, mq::vec2(v.x + v.w / 2.0, v.y + v.h / 2.0));
        }
    }

    /// The zoom now (1.0 is 100 %; 1.0 before the view is laid out).
    pub fn zoom(&self) -> f32 {
        self.cam.as_ref().map_or(1.0, |c| c.zoom)
    }


    /// The palette's texture of an object kind: loaded from its pixels the first time it is shown.
    pub fn thumbnail(&mut self, ctx: &egui::Context, kind: &'static str) -> Option<egui::TextureHandle> {
        if let Some(tex) = self.thumb_tex.get(kind) {
            return Some(tex.clone());
        }
        let image = self.thumb_src.get(kind)?.clone()?;
        let tex = ctx.load_texture(format!("studio-thumb-{kind}"), image, egui::TextureOptions::LINEAR);
        self.thumb_tex.insert(kind, tex.clone());
        Some(tex)
    }

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
        self.options_bar(ui);
        // The AI viewer's strip (its controls only while the mode is on); the map takes the rest.
        let ready = match (&self.pkg, &self.install, &self.maps.map) {
            (_, None, _) => Err(tr("The AI viewer needs the game's files: set RAZDOR_DT_DIR")),
            (Some(_), Some(_), Some(_)) => Ok(()),
            _ => Err(tr("No map to show.")),
        };
        match self.canvas.ai.strip(ui, ready) {
            super::aiview::Req::Start => {
                if let (Some(pkg), Some(install), Some(map)) = (self.pkg.clone(), self.install.clone(), self.maps.map.clone()) {
                    self.canvas.ai.start(pkg, install, &map);
                }
            }
            super::aiview::Req::Cancel => self.canvas.ai.cancel(),
            super::aiview::Req::None => {}
        }
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
        let set = self.layer_set();
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
        let mut eyedrop: Option<u8> = None;
        let mut cancel_placement = false;

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
                // Ctrl+wheel zooms; Space, the Hand tool, the middle and the right button pan.
                let (ctrl, space, alt, shift) = ui.input(|i| (i.modifiers.command, i.key_down(egui::Key::Space), i.modifiers.alt, i.modifiers.shift));
                if resp.hovered() {
                    let scroll = ui.input(|i| i.raw_scroll_delta.y);
                    if let (true, true, Some(p)) = (scroll != 0.0, ctrl, resp.hover_pos()) {
                        cam.zoom_at(1.0015f32.powf(scroll), to_px(p));
                    }
                }
                let panning = resp.dragged_by(PointerButton::Middle) || resp.dragged_by(PointerButton::Secondary) || ((space || tool == Tool::Hand) && resp.dragged_by(PointerButton::Primary));
                if panning {
                    let d = ui.input(|i| i.pointer.delta());
                    cam.pan(mq::vec2(d.x * ppp, d.y * ppp));
                }
                // A right click (no drag) cancels a placement: the tool goes back to Select.
                if resp.clicked_by(PointerButton::Secondary) && tool.places() {
                    cancel_placement = true;
                }
                cam.clamp(w, h);
                // The snapshot's `hover=x,y` stands in for a pointer over that cell (a snapshot has no pointer).
                let hover = resp.hover_pos().map(|p| cam.cell_at(to_px(p))).or(canvas.snap_hover).filter(|&(x, y)| x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h);
                canvas.hover = hover;
                let press: Option<(i32, i32)> = ui.input(|i| i.pointer.press_origin()).map(|p| cam.cell_at(to_px(p)));
                let press64 = press.map(|(x, y)| (i64::from(x), i64::from(y)));

                let entry = pkg.as_ref().and_then(|p| p.maps.get(&good.map));
                let spec = entry.and_then(|m| m.spec.as_ref());
                if let (Some(spec), Some(cell)) = (spec, hover) {
                    tip = tip_of(&canvas.marks, canvas.reach, &canvas.unreached, &good.compiled, spec, cell);
                }
                // The status bar's terrain and the top object under the pointer.
                canvas.hover_info = hover.map(|c| {
                    let code = good.compiled.scenario.terrain_at(c.0 as u32, c.1 as u32).unwrap_or(0);
                    let object = spec.and_then(|sp| pick_any(&good.compiled, sp, c)).map(|sel| sel_name(&sel));
                    (terrain_title(code).to_string(), object)
                });
                let square_inside = |cell: (i64, i64), size: u32| CellRect::brush(cell.0 as i32, cell.1 as i32, size).inside(w, h);
                let hover64 = hover.map(|(x, y)| (i64::from(x), i64::from(y)));

                if panning {
                    // The drag pans the view: no tool acts on it.
                } else if canvas.paste && canvas.clip.is_some() {
                    // A paste armed with Ctrl+V or a stamp: a click puts the clip's top left corner on the cell.
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
                                    if let Some(sel) = self.selection.clone().filter(movable).filter(|s| set.allows(s)) {
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
                                // The stack under the cell, the active layer first; a hidden or locked layer is passed over.
                                pick = Some(match (hover, spec) {
                                    (Some(cell), Some(spec)) => {
                                        let mut stack = stack_at(&good.compiled, spec, cell);
                                        stack.sort_by_key(|sel| layers::layer_of(sel) != set.active);
                                        set.first_pickable(&stack)
                                    }
                                    _ => None,
                                });
                            }
                        }
                        Tool::Terrain => {
                            // The brush paints the cells under the path between two samples (no gaps), a
                            // Shift click draws the line from the last point, and Alt picks the cell's terrain.
                            let (size, shape) = (canvas.brush, canvas.shape);
                            if resp.drag_started_by(PointerButton::Primary) {
                                if let Some(c) = press64 {
                                    if alt {
                                        eyedrop = good.compiled.scenario.terrain_at(c.0 as u32, c.1 as u32);
                                    } else {
                                        paint_cells = Some(brush::footprint(c.0, c.1, size, shape));
                                        canvas.paint_last = Some(c);
                                    }
                                }
                            } else if resp.dragged_by(PointerButton::Primary) {
                                if let (Some(c), Some(last)) = (hover64, canvas.paint_last) {
                                    paint_cells = Some(brush::stroke(last, c, size, shape));
                                    canvas.paint_last = Some(c);
                                }
                            } else if resp.clicked_by(PointerButton::Primary) {
                                if let Some(c) = hover64 {
                                    if alt {
                                        eyedrop = good.compiled.scenario.terrain_at(c.0 as u32, c.1 as u32);
                                    } else {
                                        let from = canvas.paint_last.filter(|_| shift).unwrap_or(c);
                                        paint_cells = Some(brush::stroke(from, c, size, shape));
                                        canvas.paint_last = Some(c);
                                    }
                                }
                            }
                            if resp.drag_stopped_by(PointerButton::Primary) || resp.clicked_by(PointerButton::Primary) {
                                paint_end = true;
                            }
                        }
                        Tool::Eyedrop => {
                            if resp.clicked_by(PointerButton::Primary) {
                                if let Some(c) = hover64 {
                                    eyedrop = good.compiled.scenario.terrain_at(c.0 as u32, c.1 as u32);
                                }
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
                        Tool::Forests | Tool::Eraser => {
                            // A stroke: the brush's cells along the path between two samples, one undo step at the
                            // release. The density keeps a share of the planted cells; the eraser takes all of them.
                            let (size, shape) = (canvas.brush, canvas.shape);
                            let erase_stroke = canvas.erase || tool == Tool::Eraser;
                            let inside_map = |c: (i64, i64)| CellRect::brush(c.0 as i32, c.1 as i32, size).inside(w, h);
                            let add_path = |st: &mut Stroke, from: (i64, i64), to: (i64, i64)| {
                                for c in brush::line_cells(from, to) {
                                    if inside_map(c) {
                                        for cell in brush::footprint(c.0, c.1, size, shape) {
                                            if st.seen.insert(cell) {
                                                st.cells.push(cell);
                                            }
                                        }
                                    }
                                }
                            };
                            if resp.drag_started_by(PointerButton::Primary) {
                                canvas.stroke_seed = canvas.stroke_seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                                let mut st = Stroke::default();
                                if let Some(c) = press64 {
                                    add_path(&mut st, c, c);
                                    canvas.paint_last = Some(c);
                                }
                                canvas.stroke = Some(st);
                            }
                            if resp.dragged_by(PointerButton::Primary) {
                                if let (Some(c), Some(last)) = (hover64, canvas.paint_last) {
                                    if let Some(st) = canvas.stroke.as_mut() {
                                        add_path(st, last, c);
                                    }
                                    canvas.paint_last = Some(c);
                                }
                            }
                            if resp.clicked_by(PointerButton::Primary) {
                                canvas.stroke_seed = canvas.stroke_seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                                let mut st = Stroke::default();
                                if let Some(c) = hover64 {
                                    let from = canvas.paint_last.filter(|_| shift).unwrap_or(c);
                                    add_path(&mut st, from, c);
                                    canvas.paint_last = Some(c);
                                }
                                canvas.stroke = Some(st);
                            }
                            if resp.drag_stopped_by(PointerButton::Primary) || resp.clicked_by(PointerButton::Primary) {
                                if let Some(st) = canvas.stroke.take() {
                                    if !st.cells.is_empty() {
                                        let seed = canvas.stroke_seed;
                                        act = Some(if erase_stroke {
                                            Act::Erase(st.cells)
                                        } else {
                                            Act::Forests(brush::scatter(&st.cells, canvas.density, seed))
                                        });
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
                        Tool::Army | Tool::Point | Tool::Object => {
                            if resp.clicked() {
                                place_at = hover;
                            }
                        }
                        Tool::Hand | Tool::Stamp => {}
                    }
                }
                if paint_cells.is_some() && !set.pickable("terrain") {
                    paint_cells = None;
                    self.log.push(tr("The layer is hidden or locked: nothing is painted").to_string());
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
                canvas.hover_info = None;
            }
        }

        if !painted.is_empty() {
            self.maps.repaint(&painted);
        }
        if let Some(cell) = place_at {
            let layer = match tool {
                Tool::Army => layers::id_of(layers::Kind::Armies),
                Tool::Point => layers::id_of(layers::Kind::Points),
                Tool::Building => layers::id_of(layers::Kind::Buildings),
                _ => layers::id_of(layers::Kind::Objects),
            };
            if set.pickable(layer) {
                self.place_at(cell, tool);
            } else {
                self.log.push(tr("The layer is hidden or locked: nothing is placed").to_string());
            }
        }
        if let Some(a) = act {
            if act_layer(&a).is_none_or(|layer| set.pickable(layer)) {
                self.run(a);
            } else {
                self.log.push(tr("The layer is hidden or locked: nothing is changed").to_string());
            }
        }
        if let Some(code) = eyedrop {
            self.pick(Pick::Terrain(code));
        }
        if cancel_placement {
            self.canvas.tool = Tool::Select;
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
                let (land, water): (Cells, Cells) = cells.into_iter().partition(|&c| !self.on_water(c));
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
                // takes the hill whose footprint covers the cell; the forests page takes the plant). The
                // eraser takes a hill only when the stroke finds no plant.
                if self.canvas.tool == Tool::Hills || (self.canvas.tool == Tool::Eraser && edits.is_empty()) {
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

    /// The options bar above the canvas (spec §5.2): the map picker, then the options of the active tool
    /// (the brush's size, presets, shape and density; the forest family; the army, building and object
    /// kinds; the copy controls) and the modes at the end (Move, Burn everything).
    fn options_bar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            self.map_picker(ui);
            ui.separator();
            let tool = self.canvas.tool;
            match tool {
                Tool::Terrain | Tool::Eraser | Tool::Hills | Tool::Forests => {
                    brush_controls(ui, self.canvas, tool != Tool::Terrain && tool != Tool::Eraser);
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
                    if tool == Tool::Hills || tool == Tool::Forests {
                        ui.checkbox(&mut self.canvas.erase, tr("Erase")).on_hover_text(tr("Delete: the hills or plants the brush covers"));
                    }
                    if tool == Tool::Terrain {
                        let mut only = false;
                        ui.add_enabled(false, egui::Checkbox::new(&mut only, tr("Only in selection"))).on_disabled_hover_text(tr("Comes in UX4"));
                    }
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
                Tool::Object => {
                    ComboBox::from_id_salt("studio-tool-object").selected_text(self.canvas.object_kind.clone()).show_ui(ui, |ui| {
                        for (name, _, _) in OBJECT_KINDS {
                            if ui.selectable_label(self.canvas.object_kind == name, name).clicked() {
                                self.canvas.object_kind = name.to_string();
                            }
                        }
                    });
                }
                Tool::Select => {
                    if ui.selectable_label(self.canvas.move_mode, tr("Move")).on_hover_text(tr("A click picks a table up, the next click drops it")).clicked() {
                        self.canvas.move_mode = !self.canvas.move_mode;
                        self.canvas.moving = None;
                    }
                }
                Tool::Fill => {
                    ui.weak(tr("Fills the connected cells of the clicked kind with the palette's kind"));
                }
                Tool::Eyedrop => {
                    ui.weak(tr("Click a cell: its terrain becomes the brush (Alt while painting does the same)"));
                }
                Tool::Stamp => {
                    ui.weak(tr("Pick a stamp in the Stamps tab, then click where it goes"));
                }
                Tool::Hand => {
                    ui.weak(tr("Drag to pan; Space and the middle button pan with any tool"));
                }
                Tool::Point => {
                    ui.weak(tr("Click places a point"));
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
                    ui.weak(tr("Fills the rectangle with the palette's kind"));
                }
                Tool::Items => {
                    ui.add_enabled(false, egui::Button::new(tool.title())).on_disabled_hover_text(tr("not in the package format yet"));
                }
            }
            if ui.button(tr("Burn everything")).clicked() {
                self.canvas.burn_asked = true;
            }
        });
    }

    /// The palette (spec §5.3, the Палитра tab): the terrain swatches with names, the objects with the
    /// install's thumbnails (name and icon without an install), a search field, and X swapping the last
    /// two picks. A click makes the choice the current one.
    pub(super) fn palette_dock(&mut self, ui: &mut Ui) {
        ui.add(egui::TextEdit::singleline(&mut self.canvas.palette_query).hint_text(tr("Search")).desired_width(f32::INFINITY));
        ui.add_space(4.0);
        let query = self.canvas.palette_query.trim().to_lowercase();
        let shown = |name: &str| query.is_empty() || name.to_lowercase().contains(&query);
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(egui::RichText::new(tr("Terrain").to_uppercase()).size(11.0).color(super::theme::FAINT).strong());
            for code in 0..DEFAULT_PALETTE.len() as u8 {
                let name = terrain_title(code);
                if !shown(name) {
                    continue;
                }
                let on = self.canvas.pick == Pick::Terrain(code);
                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), super::theme::ROW_H), Sense::click());
                palette_row_frame(ui, r, &resp, on);
                let [red, green, blue] = DEFAULT_PALETTE[code as usize].1;
                let chip = egui::Rect::from_min_size(r.min + egui::vec2(6.0, 5.0), egui::vec2(14.0, 14.0));
                ui.painter().rect_filled(chip, CornerRadius::same(2), Color32::from_rgb(red, green, blue));
                ui.painter().text(r.min + egui::vec2(28.0, r.height() / 2.0), Align2::LEFT_CENTER, name, FontId::proportional(super::theme::TEXT_SIZE), if on { super::theme::TEXT } else { super::theme::DIM });
                if resp.clicked() {
                    self.pick(Pick::Terrain(code));
                }
                resp.on_hover_text(name);
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new(tr("Objects").to_uppercase()).size(11.0).color(super::theme::FAINT).strong());
            let objects = [
                (Pick::Hills, "hills", tr("Hills"), ic::MOUNTAINS),
                (Pick::Forest(Forest::Family(0)), "trees", tr("Forest"), ic::TREE),
                (Pick::Forest(Forest::Family(8)), "dead_trees", tr("Dead forest"), ic::TREE),
                (Pick::Forest(Forest::Thicket), "thicket", tr("Thicket"), ic::PLANT),
                (Pick::Object("mountains"), "mountains", tr("Mountains"), ic::MOUNTAINS),
                (Pick::Object("stones"), "stones", tr("Stones"), ic::CUBE),
            ];
            for (pick, kind, name, icon) in objects {
                if !shown(name) {
                    continue;
                }
                let on = self.canvas.pick == pick;
                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), Sense::click());
                palette_row_frame(ui, r, &resp, on);
                let thumb = egui::Rect::from_min_size(r.min + egui::vec2(4.0, 4.0), egui::vec2(36.0, 36.0));
                match self.canvas.thumbnail(ui.ctx(), kind) {
                    Some(tex) => {
                        ui.painter().image(tex.id(), thumb, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        ui.painter().rect_filled(thumb, CornerRadius::same(3), super::theme::RAISED);
                        ui.painter().text(thumb.center(), Align2::CENTER_CENTER, icon, FontId::proportional(18.0), super::theme::FAINT);
                    }
                }
                ui.painter().text(r.min + egui::vec2(48.0, r.height() / 2.0), Align2::LEFT_CENTER, name, FontId::proportional(super::theme::TEXT_SIZE), if on { super::theme::TEXT } else { super::theme::DIM });
                if resp.clicked() {
                    self.pick(pick);
                }
                resp.on_hover_text(name);
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new(tr("X swaps the last two picks")).size(super::theme::SMALL).color(super::theme::FAINT));
        });
    }

    /// Makes a palette choice the current one; the one before it is kept for X.
    fn pick(&mut self, pick: Pick) {
        if self.canvas.pick != pick {
            self.canvas.pick_prev = self.canvas.pick;
            self.canvas.pick = pick;
        }
        pick.apply(self.canvas);
    }

    /// The layers of the shown map (spec §5.4, the Слои tab): an eye and a lock per layer (Alt+click on
    /// an eye shows only that layer), the active layer by its name, and the overlay group of view-only
    /// switches.
    pub(super) fn layers_dock(&mut self, ui: &mut Ui) {
        let Some(map) = self.maps.map.clone() else {
            ui.weak(tr("No map to show."));
            return;
        };
        let mut set = self.canvas.layer_sets.remove(&map).unwrap_or_default();
        let defs = layers::fixed();
        let all: Vec<String> = defs.iter().map(|d| d.id.clone()).collect();
        let alt = ui.input(|i| i.modifiers.alt);
        for def in &defs {
            let on = set.active == def.id;
            let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), super::theme::ROW_H + 2.0), Sense::hover());
            if on {
                ui.painter().rect_filled(r, CornerRadius::same(4), super::theme::HOVER);
            }
            let mut row = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(egui::vec2(4.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
            let visible = set.visible(&def.id);
            if row.add(egui::Button::new(RichText::new(if visible { ic::EYE } else { ic::EYE_SLASH }).size(super::theme::ICON)).frame(false)).on_hover_text(tr("Show or hide the layer (Alt+click: only this layer)")).clicked() {
                if alt {
                    set.solo(&def.id, &all);
                } else {
                    set.toggle_visible(&def.id);
                }
            }
            let locked = set.locked(&def.id);
            let lock = if locked { ic::LOCK_SIMPLE } else { ic::LOCK_SIMPLE_OPEN };
            if row.add(egui::Button::new(RichText::new(lock).size(super::theme::ICON)).frame(false)).on_hover_text(tr("Lock the layer: no picks, no edits")).clicked() {
                set.toggle_locked(&def.id);
            }
            let text = egui::RichText::new(tr(def.name)).color(if on { super::theme::TEXT } else { super::theme::DIM });
            if row.add(egui::Label::new(text).sense(Sense::click())).clicked() {
                set.active = def.id.clone();
            }
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("Overlays").to_uppercase()).size(11.0).color(super::theme::FAINT).strong());
        ui.checkbox(&mut self.canvas.reach, tr("Reach")).on_hover_text(tr("Cells the hero cannot walk to, on the map's story"));
        ui.checkbox(&mut self.canvas.grid, tr("Grid"));
        ui.checkbox(&mut self.canvas.passable, tr("Passable")).on_hover_text(tr("The walk grid: normal, hard, harder, impassable, building, water"));
        ui.checkbox(&mut self.canvas.patrols, tr("Patrols")).on_hover_text(tr("The patrol loops of the army goals in the triggers"));
        ui.checkbox(&mut self.canvas.fog, tr("Fog")).on_hover_text(tr("What the hero and the lit lanterns see at the start"));
        ui.checkbox(&mut self.canvas.footprints, tr("Footprints"));
        self.canvas.layer_sets.insert(map, set);
    }

    /// The layers of the shown map, read (a map with none has the defaults).
    pub(super) fn layer_set(&self) -> layers::Set {
        self.maps.map.as_ref().and_then(|m| self.canvas.layer_sets.get(m).cloned()).unwrap_or_default()
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
            // The stamp tool places the armed clip on the next click.
            self.canvas.clip = Some(stamp.clip);
            self.canvas.paste = true;
            self.canvas.tool = Tool::Stamp;
            self.focus_map = true;
        }
    }

}

/// The brush's controls on the options bar: the size slider and the DT presets (1 to 6 and 9), the
/// shape (square or round), and the density in per cent when the tool scatters (`density`).
fn brush_controls(ui: &mut Ui, canvas: &mut Canvas, density: bool) {
    ui.label(egui::RichText::new(tr("Brush")).color(super::theme::DIM).size(super::theme::SMALL));
    ui.spacing_mut().slider_width = 90.0;
    ui.add(egui::Slider::new(&mut canvas.brush, 1..=15).integer()).on_hover_text(tr("[ and ] make it smaller and larger"));
    for size in BRUSH_SIZES {
        if ui.add(egui::Button::new(RichText::new(size.to_string()).size(12.0)).min_size(egui::vec2(22.0, super::theme::BTN_H)).selected(canvas.brush == size)).on_hover_text(trf!("Size {size}", size = size)).clicked() {
            canvas.brush = size;
        }
    }
    if ui.add(egui::Button::new(RichText::new(ic::SQUARE).size(super::theme::ICON)).selected(canvas.shape == brush::Shape::Square)).on_hover_text(tr("Square brush")).clicked() {
        canvas.shape = brush::Shape::Square;
    }
    if ui.add(egui::Button::new(RichText::new(ic::CIRCLE).size(super::theme::ICON)).selected(canvas.shape == brush::Shape::Round)).on_hover_text(tr("Round brush")).clicked() {
        canvas.shape = brush::Shape::Round;
    }
    if density {
        ui.label(egui::RichText::new(tr("Density")).color(super::theme::DIM).size(super::theme::SMALL));
        ui.add(egui::Slider::new(&mut canvas.density, 0..=100).step_by(10.0).suffix(" %")).on_hover_text(tr("Shift+[ and Shift+] change it by 10 %"));
    }
}

/// A palette row's background: the active choice and the hovered row are filled, the rest are not.
fn palette_row_frame(ui: &Ui, rect: egui::Rect, resp: &egui::Response, on: bool) {
    if on || resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(4), if on { super::theme::HOVER } else { super::theme::RAISED });
    }
    if on {
        ui.painter().rect_stroke(rect, CornerRadius::same(4), egui::Stroke::new(1.0, super::theme::ACCENT), egui::StrokeKind::Inside);
    }
}

/// The name of a terrain kind of the 16-kind palette (`pack::terrain::DEFAULT_PALETTE`), as the palette
/// shows it.
fn terrain_title(code: u8) -> &'static str {
    match code {
        0 => tr("Shallow water and fords"),
        1 => tr("Coastal water"),
        2 => tr("Deep sea"),
        3 => tr("Lava fields"),
        4 => tr("Road"),
        5 => tr("Lowland grass"),
        6 => tr("Plain grass"),
        7 => tr("Dry plain"),
        8 => tr("Marsh"),
        9 => tr("Impassable swamp"),
        10 => tr("Sand dunes"),
        11 => tr("Clay soil"),
        12 => tr("Stony soil"),
        13 => tr("Scorched land"),
        14 => tr("Snowy ground"),
        _ => tr("Impassable snowdrifts"),
    }
}

/// The tool strip (spec §5.1): one button per tool, top to bottom, with its icon and its tooltip
/// («name · shortcut»). The active tool is highlighted; the selection tools and the trigger zone are
/// shown disabled. Returns the command of a clicked button; the caller runs it.
pub(super) fn tool_strip(ui: &mut Ui, active: Tool, tip: &dyn Fn(&str) -> String) -> Option<&'static str> {
    // (command, icon, the tool it selects, why it is disabled ("" when it works))
    type Slot = (&'static str, &'static str, Option<Tool>, &'static str);
    const GROUPS: [&[Slot]; 6] = [
        &[("tool-select", ic::CURSOR, Some(Tool::Select), "")],
        &[
            ("tool-brush", ic::PAINT_BRUSH, Some(Tool::Terrain), ""),
            ("tool-eraser", ic::ERASER, Some(Tool::Eraser), ""),
            ("tool-fill", ic::PAINT_BUCKET, Some(Tool::Fill), ""),
            ("tool-eyedrop", ic::EYEDROPPER, Some(Tool::Eyedrop), ""),
        ],
        &[
            ("tool-marquee", ic::SELECTION, None, "Comes in UX4"),
            ("tool-lasso", ic::LASSO, None, "Comes in UX4"),
            ("tool-wand", ic::MAGIC_WAND, None, "Comes in UX4"),
        ],
        &[
            ("tool-stamp", ic::STAMP, Some(Tool::Stamp), ""),
            ("tool-army", ic::SWORD, Some(Tool::Army), ""),
            ("tool-building", ic::BUILDINGS, Some(Tool::Building), ""),
        ],
        &[
            ("tool-point", ic::MAP_PIN, Some(Tool::Point), ""),
            ("tool-trigger", ic::POLYGON, None, "not in the package format yet"),
        ],
        &[("tool-hand", ic::HAND, Some(Tool::Hand), "")],
    ];
    let mut clicked = None;
    ui.vertical_centered(|ui| {
        for (gi, group) in GROUPS.iter().enumerate() {
            if gi > 0 {
                ui.add_space(3.0);
                ui.separator();
                ui.add_space(3.0);
            }
            for &(id, icon, tool, why) in group.iter() {
                let on = tool == Some(active);
                let button = egui::Button::new(RichText::new(icon).size(super::theme::ICON + 2.0)).min_size(egui::vec2(34.0, 30.0)).selected(on);
                let text = tip(id);
                let resp = if tool.is_some() { ui.add(button) } else { ui.add_enabled(false, button) };
                let resp = if why.is_empty() { resp.on_hover_text(text) } else { resp.on_disabled_hover_text(format!("{text}\n{}", tr(why))) };
                if resp.clicked() && tool.is_some() {
                    clicked = Some(id);
                }
            }
        }
    });
    clicked
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

/// The layer an operation edits, `None` for those that edit no layer's tables.
fn act_layer(act: &Act) -> Option<&'static str> {
    match act {
        Act::Hill(..) | Act::Forests(_) | Act::Erase(_) => Some(layers::id_of(layers::Kind::Objects)),
        Act::BuildingErase(_) => Some(layers::id_of(layers::Kind::Buildings)),
        Act::Rect(..) | Act::Fill(_) => Some(layers::id_of(layers::Kind::Terrain)),
        Act::Region(_) | Act::Burn | Act::Paste(_) => None,
    }
}

/// Everything a click under a cell can pick, the top one first: the hit (an army, a building, the hero, a
/// point), then the point and the terrain object on the cell.
fn stack_at(compiled: &razdor::pack::compile::Compiled, spec: &MapFile, cell: (i32, i32)) -> Vec<Selection> {
    let mut out: Vec<Selection> = hit(compiled, spec, cell).into_iter().collect();
    let at = |c: &[u32; 2]| c[0] as i32 == cell.0 && c[1] as i32 == cell.1;
    if let Some(i) = spec.point.iter().position(|p| at(&p.get_ref().cell)) {
        out.push(Selection::Point(i));
    }
    if let Some(i) = spec.object.iter().position(|o| at(&o.get_ref().cell)) {
        out.push(Selection::Object(i));
    }
    out
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
    // What the footprint preview needs, copied before the camera borrows the canvas.
    let preview = Preview { tool: canvas.tool, brush: canvas.brush, shape: canvas.shape, army: canvas.army_model.clone(), building: canvas.building_kind.clone(), hover: canvas.hover, paste: canvas.paste };
    let (Some(_), Some(cam)) = (canvas.rect, canvas.cam.as_mut()) else { return };
    if canvas.layers.as_ref().is_none_or(|(serial, _)| *serial != good.serial) {
        let hero = spec.map_or((0, 0), |s| (i64::from(s.hero.cell[0]), i64::from(s.hero.cell[1])));
        let patrols = pkg.map(|p| overlay::patrols(&p.triggers, &good.map)).unwrap_or_default();
        canvas.layers = Some((good.serial, Layers::of(&good.compiled.scenario, hero, patrols)));
    }
    let layers = canvas.layers.as_ref().map(|(_, l)| l);
    scissor(Some(cam.view));
    let set = maps.map.as_ref().and_then(|m| canvas.layer_sets.get(m)).cloned().unwrap_or_default();
    let shown = [set.visible("terrain"), set.visible("objects"), set.visible("buildings"), set.visible("armies")];
    draw_map_layers(&good.compiled.scenario, art, cam, tex.as_ref(), shown);
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
    for &(x, y) in &canvas.score_cells {
        outline(cam, CellRect::brush(x as i32, y as i32, 1), 2.0, mq::Color::new(1.0, 0.85, 0.2, 0.95));
    }
    if let (Some(spec), true) = (spec, set.visible("points")) {
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
    super::aiview::draw(cam, &canvas.ai);
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
    if let (Some((x, y)), false) = (preview.hover, preview.paste) {
        footprint_preview(cam, &preview, x, y);
    }
    scissor(None);
}

/// The footprint under the pointer (spec §5.5, §5.7): the cells the brush or the hill will touch, filled and
/// outlined before the first click; a placing tool shows its ghost (the building's footprint, or the cell with
/// the army's or point's name).
/// The values the footprint preview reads from the canvas (copied, so the camera can borrow the canvas).
struct Preview {
    tool: Tool,
    brush: u32,
    shape: brush::Shape,
    army: String,
    building: String,
    hover: Option<(i32, i32)>,
    paste: bool,
}

fn footprint_preview(cam: &Cam, preview: &Preview, x: i32, y: i32) {
    let (xi, yi) = (i64::from(x), i64::from(y));
    let to_cells = |r: CellRect| -> Vec<(i64, i64)> { r.cells().map(|(cx, cy)| (i64::from(cx), i64::from(cy))).collect() };
    let cells: Vec<(i64, i64)> = match preview.tool {
        Tool::Terrain | Tool::Forests | Tool::Eraser => brush::footprint(xi, yi, preview.brush, preview.shape),
        Tool::Hills => {
            let c = brush::hill_centre(xi, yi, preview.brush);
            to_cells(CellRect::brush(c.0 as i32, c.1 as i32, preview.brush))
        }
        Tool::Building => {
            let (w, h) = default_footprint(&preview.building).unwrap_or((1, 1));
            let mut out = Vec::new();
            for r in footprint_rects((x, y), (w as i32, h as i32)) {
                out.extend(to_cells(r));
            }
            out
        }
        _ => vec![(xi, yi)],
    };
    // A placing tool's ghost is strong (it is the only thing under the pointer); a brush's footprint is light.
    let (fill, edge, thick) = if preview.tool.places() {
        (mq::Color::new(0.31, 0.55, 1.0, 0.55), mq::Color::new(1.0, 1.0, 1.0, 0.95), 2.0)
    } else {
        (mq::Color::new(0.31, 0.55, 1.0, 0.30), mq::Color::new(1.0, 1.0, 1.0, 0.75), 1.0)
    };
    for (cx, cy) in cells {
        let r = cam.rect(CellRect::brush(cx as i32, cy as i32, 1));
        mq::draw_rectangle(r.x, r.y, r.w, r.h, fill);
        mq::draw_rectangle_lines(r.x, r.y, r.w, r.h, thick, edge);
    }
    if preview.tool.places() {
        let label = match preview.tool {
            Tool::Army => preview.army.clone(),
            Tool::Building => preview.building.clone(),
            _ => tr("Point").to_string(),
        };
        let p = cam.to_screen(mq::vec2(x as f32, y as f32));
        mq::draw_text(&label, p.x + 6.0, p.y - 4.0, 14.0, mq::Color::new(0.9, 0.93, 0.96, 0.9));
    }
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
