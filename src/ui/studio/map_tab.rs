//! The Map tab: the map of the open package, drawn by the game's own map renderer
//! (`ui::editor::canvas`) into the tab's rectangle. egui allocates the rectangle and takes the
//! input (pan, zoom, click, drag, hover); the map is drawn with macroquad after the egui pass and
//! before egui's meshes, clipped to the rectangle, so nothing of it shows over another dock.
//!
//! The drag to move: a left press on the selected army, point, hero or building, then a drag. A
//! ghost outline follows the cell under the pointer (a building keeps the grab offset, so its
//! footprint moves with the pointer). On release the move is one `SetCell` edit; the ghost stays
//! until the package is compiled again.

use egui::{Align2, Color32, ComboBox, CornerRadius, FontId, PointerButton, Sense, TextStyle, Ui};
use egui_macroquad::egui;
use macroquad::prelude as mq;
use razdor::editor::geometry::CellRect;
use razdor::i18n::tr;
use razdor::pack::load::Package;
use razdor::studio::map::{self, hit, map_ids, outline_of, MapView, Selection};
use razdor::studio::Edit;

use super::{Viewer, WARNING_AMBER};
use crate::ui::dt_art::DtArt;
use crate::ui::editor::canvas::{draw_map, Cam, Overview};

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

/// The Map tab's view: the camera, the rectangle and the hovered cell.
#[derive(Default)]
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
}

/// Shows map `id` on the Map tab: the camera fits it, and the selection goes.
pub fn switch_map(maps: &mut MapView, canvas: &mut Canvas, selection: &mut Option<Selection>, id: &str) {
    if maps.map.as_deref() != Some(id) {
        maps.show(id);
        canvas.fit = true;
        canvas.hover = None;
        canvas.moving = None;
        *selection = None;
    }
}

/// The accent colour of the studio (the selection outline).
fn accent() -> mq::Color {
    mq::Color::from_rgba(0x4f, 0x8e, 0xdc, 255)
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

impl Viewer<'_> {
    /// The Map tab: the stale banners, the map picker, the canvas (its input here, its drawing in
    /// [`draw`]), and the hovered cell at the bottom left.
    pub(super) fn map_tab(&mut self, ui: &mut Ui) {
        self.map_picker(ui);
        for stale in &self.ws.stale {
            banner(ui, &stale.banner());
        }
        let status = ui.text_style_height(&TextStyle::Body) + 6.0;
        let size = egui::vec2(ui.available_width(), (ui.available_height() - status).max(60.0));
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        let to_px = |p: egui::Pos2| mq::vec2(p.x * ppp, p.y * ppp);
        let pkg = self.pkg.clone();
        let canvas = &mut *self.canvas;
        canvas.rect = Some(rect);
        canvas.ppp = ppp;
        // A click on the map picks what it hits: decided in the match, applied after it.
        let mut pick: Option<Option<Selection>> = None;

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
                                    Some(at) => match self.ws.apply(Edit::SetCell { at, cell: m.target }) {
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

        match pick {
            Some(Some(sel)) => self.select(sel),
            Some(None) => *self.selection = None,
            None => {}
        }
        ui.horizontal(|ui| {
            if let Some((x, y)) = self.canvas.hover {
                ui.weak(format!("{x}, {y}"));
            }
        });
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

/// Whether a cell is on the map.
fn inside(good: &razdor::studio::map::Good, (x, y): (i32, i32)) -> bool {
    let s = &good.compiled.scenario;
    x >= 0 && y >= 0 && (x as u32) < s.width() && (y as u32) < s.height()
}

/// Draws the shown map into the Map tab's rectangle, with the selection, the move's ghost and the
/// hovered cell. Runs after the egui pass and before egui is drawn over it.
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
