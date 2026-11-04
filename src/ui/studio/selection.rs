//! Selections on the Map tab (UX4, spec §5.6): the gestures' shapes, the commands on a selection
//! (copy, cut, paste, paste in place, delete, select all, deselect, invert, save as a stamp), the
//! floating paste, the marching ants and the stamp prompt. The model is `studio::select` (cells and
//! their outline), the copy and the edits are `studio::region`; this file holds the Map tab's side.
//!
//! The selection belongs to the map shown: `Canvas::sel` is one map's cells, and a switch of map
//! clears it (`map_tab::switch_map`). Selecting changes no file and makes no undo step; each edit
//! of the selection's contents is one undo step (`studio::region`).

use egui::{Align2, Color32, Stroke, TextEdit, Ui};
use egui_macroquad::egui;
use egui_phosphor::regular as ic;
use macroquad::prelude as mq;
use razdor::i18n::tr;
use razdor::trf;
use razdor::studio::geometry::CellRect;
use razdor::studio::layers;
use razdor::studio::map::MapView;
use razdor::studio::region::{self, Target};
use razdor::studio::select::{self, Edge, Mask, Mode};
use razdor::studio::Workspace;

use super::map_tab::{Canvas, Tool};
use crate::ui::studio::theme;

/// The shape a selecting gesture makes: a rectangle between two cells, a polygon through cell
/// positions (cell units), or the magic wand's start cell and whether it is contiguous.
#[derive(Clone, Debug, PartialEq)]
pub enum SelShape {
    Rect((i32, i32), (i32, i32)),
    Lasso(Vec<(f64, f64)>),
    Wand((u32, u32), bool),
}

/// The selection the canvas shows (`None` when nothing is selected).
pub fn set_selection(canvas: &mut Canvas, sel: Option<Mask>) {
    canvas.sel_edges = sel.as_ref().map_or_else(Vec::new, Mask::outline);
    canvas.sel = sel.filter(|m| !m.is_empty());
}

/// The cells a paint may change, when «Только в выделении» is on and a selection exists.
pub fn within(canvas: &Canvas) -> Option<&Mask> {
    if canvas.only_sel {
        canvas.sel.as_ref()
    } else {
        None
    }
}

/// The layers of the map shown (the eye and the lock of each); a layer with no instance is visible
/// and unlocked.
pub fn set_of(canvas: &Canvas, maps: &MapView) -> layers::Set {
    maps.map.as_ref().and_then(|m| canvas.layer_sets.get(m).cloned()).unwrap_or_default()
}

/// The target of a copy, cut, paste or clear: the visible unlocked layers, every cell.
pub fn target_of(set: &layers::Set) -> Target {
    Target { terrain: set.pickable("terrain"), objects: set.pickable("objects"), buildings: set.pickable("buildings"), armies: set.pickable("armies"), points: set.pickable("points"), within: None }
}

/// The width and height of the map shown, once it is compiled.
fn dims(maps: &MapView) -> Option<(u32, u32)> {
    maps.current().map(|g| (g.compiled.scenario.width(), g.compiled.scenario.height()))
}

/// Combines a new shape with the selection by `mode`; an empty result is no selection.
pub fn apply_shape(canvas: &mut Canvas, shape: Mask, mode: Mode) {
    let mut next = canvas.sel.clone().unwrap_or_else(|| Mask::empty(shape.width(), shape.height()));
    next.combine(&shape, mode);
    set_selection(canvas, Some(next));
}

/// Select all: every cell of the map shown.
pub fn select_all(canvas: &mut Canvas, maps: &MapView) {
    if let Some((w, h)) = dims(maps) {
        set_selection(canvas, Some(Mask::all(w, h)));
    }
}

/// Deselect: no cell selected.
pub fn deselect(canvas: &mut Canvas) {
    set_selection(canvas, None);
}

/// Invert: the cells that were not selected are, and the reverse (no selection inverts to all).
pub fn invert(canvas: &mut Canvas, maps: &MapView) {
    if let Some((w, h)) = dims(maps) {
        let mut next = canvas.sel.clone().unwrap_or_else(|| Mask::empty(w, h));
        next.invert();
        set_selection(canvas, Some(next));
    }
}

/// The mask of a shape on the map (the wand reads the terrain picture; `None` when it cannot).
pub fn shape_mask(ws: &mut Workspace, map: &str, w: u32, h: u32, shape: &SelShape) -> Result<Mask, String> {
    Ok(match shape {
        SelShape::Rect(a, b) => Mask::of_rect(w, h, CellRect::spanning(*a, *b)),
        SelShape::Lasso(pts) => Mask::of_polygon(w, h, pts),
        SelShape::Wand(start, contiguous) => {
            let i = ws.open_terrain(map)?;
            Mask::of_wand(&ws.terrains[i].terrain.cells, w, h, *start, *contiguous)
        }
    })
}

/// The origin cell of a selection's bounding box (where a clip copied from it goes back to).
fn origin_of(sel: &Mask) -> (i64, i64) {
    sel.bounds().map_or((0, 0), |r| (i64::from(r.x0), i64::from(r.y0)))
}

/// Copies the selection to the clip (Ctrl+C). Nothing changes in the map.
pub fn copy(ws: &mut Workspace, maps: &MapView, canvas: &mut Canvas, set: &layers::Set, log: &mut Vec<String>) {
    let (Some(map), Some(sel)) = (maps.map.clone(), canvas.sel.clone()) else {
        log.push(tr("Select cells first: M, L or W").to_string());
        return;
    };
    match region::copy_selection(ws, &map, &sel, &target_of(set)) {
        Ok(clip) => {
            log.push(trf!("Copied {n} cells", n = sel.count()));
            canvas.clip_origin = origin_of(&sel);
            canvas.clip = Some(clip);
            canvas.paste = false;
            canvas.float = false;
        }
        Err(e) => log.push(e),
    }
}

/// Cuts the selection's contents on the visible unlocked layers (Ctrl+X): the copy goes to the clip,
/// the contents are removed as one undo step. Returns whether the map changed.
pub fn cut(ws: &mut Workspace, maps: &mut MapView, canvas: &mut Canvas, set: &layers::Set, log: &mut Vec<String>) -> bool {
    let (Some(map), Some(sel)) = (maps.map.clone(), canvas.sel.clone()) else {
        log.push(tr("Select cells first: M, L or W").to_string());
        return false;
    };
    let target = target_of(set);
    match region::cut(ws, &map, &sel, &target) {
        Ok(clip) => {
            canvas.clip_origin = origin_of(&sel);
            canvas.clip = Some(clip);
            canvas.paste = false;
            canvas.float = false;
            clear_repaint(ws, maps, &map, &sel, &target);
            log.push(trf!("Cut {n} cells", n = sel.count()));
            true
        }
        Err(e) => {
            log.push(e);
            false
        }
    }
}

/// Delete (Delete, Backspace): clears the selection's contents on the visible unlocked layers, one
/// undo step. Returns whether the map changed.
pub fn delete(ws: &mut Workspace, maps: &mut MapView, canvas: &mut Canvas, set: &layers::Set, log: &mut Vec<String>) -> bool {
    let (Some(map), Some(sel)) = (maps.map.clone(), canvas.sel.clone()) else { return false };
    let target = target_of(set);
    match region::clear(ws, &map, &sel, &target, "delete") {
        Ok(n) => {
            clear_repaint(ws, maps, &map, &sel, &target);
            if n > 0 {
                log.push(trf!("Deleted {n} objects", n = n));
            }
            true
        }
        Err(e) => {
            log.push(e);
            false
        }
    }
}

/// Repaints the terrain a cut or a delete changed: the selected cells, when the terrain layer is
/// allowed (and the target has no limit of its own).
fn clear_repaint(ws: &mut Workspace, maps: &mut MapView, map: &str, sel: &Mask, target: &Target) {
    if target.terrain {
        let cells: Vec<(i64, i64)> = sel.cells().into_iter().filter(|&(x, y)| target.cell_ok(x, y)).collect();
        repaint_terrain(ws, maps, map, &cells);
    }
}

/// Repaints the given cells of the map shown with the terrain the workspace holds for them.
pub fn repaint_terrain(ws: &mut Workspace, maps: &mut MapView, map: &str, cells: &[(i64, i64)]) {
    let Ok(i) = ws.open_terrain(map) else { return };
    let t = &ws.terrains[i].terrain;
    let painted: Vec<(u32, u32, u8)> = cells
        .iter()
        .filter(|&&(x, y)| x >= 0 && y >= 0 && x < i64::from(t.width) && y < i64::from(t.height))
        .map(|&(x, y)| (x as u32, y as u32, t.cells[(y as u32 * t.width + x as u32) as usize]))
        .collect();
    if !painted.is_empty() {
        maps.repaint(&painted);
    }
}

/// Arms a paste of the clip (Ctrl+V): it follows the pointer as a floating piece until Enter or a
/// click commits it, or Esc drops it.
pub fn arm_paste(canvas: &mut Canvas, log: &mut Vec<String>) {
    if canvas.clip.is_some() {
        canvas.paste = true;
        canvas.float = true;
    } else {
        log.push(tr("Nothing copied yet: select cells and press Ctrl+C").to_string());
    }
}

/// Pastes the clip with its top left corner on cell `at` (one undo step), on the visible unlocked
/// layers. A stamp placed by the Stamp tool is limited to the selection when «Только в выделении»
/// is on. Returns whether the map changed.
pub fn commit(ws: &mut Workspace, maps: &mut MapView, canvas: &mut Canvas, set: &layers::Set, at: (i64, i64), log: &mut Vec<String>) -> bool {
    let (Some(map), Some(clip)) = (maps.map.clone(), canvas.clip.clone()) else { return false };
    let mut target = target_of(set);
    if canvas.tool == Tool::Stamp {
        target.within = within(canvas).cloned();
    }
    match region::paste_to(ws, &map, &clip, at, &target) {
        Ok(n) => {
            log.push(trf!("Pasted {n} objects", n = n));
            let cells: Vec<(i64, i64)> = (0..clip.h).flat_map(|y| (0..clip.w).map(move |x| (i64::from(x), i64::from(y)))).map(|(x, y)| (at.0 + x, at.1 + y)).collect();
            repaint_terrain(ws, maps, &map, &cells);
            true
        }
        Err(e) => {
            log.push(e);
            false
        }
    }
}

/// Commits the floating paste at `at`, if one is floating: a click or Enter. A floating paste ends
/// with its commit; a stamp of the Stamp tool stays armed for the next click.
pub fn commit_float(ws: &mut Workspace, maps: &mut MapView, canvas: &mut Canvas, set: &layers::Set, at: (i64, i64), log: &mut Vec<String>) -> bool {
    let changed = commit(ws, maps, canvas, set, at, log);
    if canvas.float {
        canvas.paste = false;
        canvas.float = false;
    }
    changed
}

/// Pastes the clip where it was copied from (Ctrl+Shift+V). Returns whether the map changed.
pub fn paste_in_place(ws: &mut Workspace, maps: &mut MapView, canvas: &mut Canvas, set: &layers::Set, log: &mut Vec<String>) -> bool {
    let at = canvas.clip_origin;
    canvas.paste = false;
    canvas.float = false;
    commit(ws, maps, canvas, set, at, log)
}

/// Saves the selection as a stamp under `name` in the package's stamp library (`.editor/stamps`,
/// through `studio::guarded`), and refreshes the Stamps tab.
pub fn save_stamp(ws: &mut Workspace, maps: &MapView, canvas: &mut Canvas, set: &layers::Set, name: &str, log: &mut Vec<String>) {
    let (Some(map), Some(sel)) = (maps.map.clone(), canvas.sel.clone()) else {
        log.push(tr("Select cells first: M, L or W").to_string());
        return;
    };
    let name = name.trim().to_string();
    let result = region::copy_selection(ws, &map, &sel, &target_of(set)).and_then(|clip| region::save_stamp(&ws.root, &name, &clip));
    match result {
        Ok(()) => {
            log.push(trf!("Saved the stamp {name}", name = name.as_str()));
            canvas.stamps = None;
        }
        Err(e) => log.push(e),
    }
}

/// The stamp prompt (Ctrl+Alt+S or the bar's button): a name, then Save or Cancel. Returns nothing;
/// it changes the canvas's prompt flag and saves when asked.
pub fn stamp_prompt(ctx: &egui::Context, ws: &mut Workspace, maps: &MapView, canvas: &mut Canvas, set: &layers::Set, log: &mut Vec<String>) {
    if !canvas.stamp_prompt {
        return;
    }
    let mut answer = None;
    egui::Window::new(tr("Save selection as stamp")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.label(tr("The stamp is saved in this package and shows in the Stamps tab."));
        ui.add(TextEdit::singleline(&mut canvas.stamp_name).hint_text(tr("Stamp name")).desired_width(260.0));
        let ok = region::valid_name(&canvas.stamp_name);
        if !ok && !canvas.stamp_name.trim().is_empty() {
            ui.colored_label(theme::DIM, tr("Letters, digits, spaces, - and _ (up to 40)"));
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(ok, egui::Button::new(tr("Save"))).clicked() {
                answer = Some(true);
            }
            if ui.button(tr("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    match answer {
        Some(true) => {
            canvas.stamp_prompt = false;
            let name = canvas.stamp_name.clone();
            save_stamp(ws, maps, canvas, set, &name, log);
        }
        Some(false) => canvas.stamp_prompt = false,
        None => {}
    }
}

/// The bar's selection controls, under the tool's mode buttons: the mode (Replace, Add, Subtract;
/// Shift and Alt override it while selecting), the wand's contiguous switch, the count, and the
/// commands on the selection. `tip` gives a command's tooltip (name and shortcut). Returns the
/// command the bar asked for, which the caller runs.
pub fn bar(ui: &mut Ui, canvas: &mut Canvas, wand: bool, tip: &dyn Fn(&str) -> String) -> Option<&'static str> {
    let mut asked = None;
    let modes = [(Mode::Replace, ic::SELECTION, tr("Replace")), (Mode::Add, ic::SELECTION_PLUS, tr("Add")), (Mode::Subtract, ic::SELECTION_SLASH, tr("Subtract"))];
    for (mode, icon, label) in modes {
        let button = egui::Button::new(egui::RichText::new(icon).size(theme::ICON)).selected(canvas.sel_mode == mode);
        if ui.add(button).on_hover_text(label).clicked() {
            canvas.sel_mode = mode;
        }
    }
    if wand {
        ui.checkbox(&mut canvas.wand_contiguous, tr("Contiguous")).on_hover_text(tr("Off: every cell of the clicked terrain on the map"));
    }
    match canvas.sel.as_ref().map(Mask::count) {
        Some(n) => ui.label(egui::RichText::new(trf!("{n} cells", n = n)).color(theme::DIM).size(theme::SMALL)),
        None => ui.label(egui::RichText::new(tr("No selection")).color(theme::DIM).size(theme::SMALL)),
    };
    let has = canvas.sel.is_some();
    let items: [(&'static str, &'static str, bool); 8] = [
        ("select-all", ic::SELECTION_ALL, true),
        ("deselect", ic::SELECTION_BACKGROUND, has),
        ("invert", ic::SELECTION_INVERSE, true),
        ("copy", ic::COPY, has),
        ("cut", ic::SCISSORS, has),
        ("paste", ic::CLIPBOARD, canvas.clip.is_some()),
        ("delete", ic::TRASH, has),
        ("save-stamp", ic::STAMP, has),
    ];
    for (id, icon, enabled) in items {
        let resp = ui.add_enabled(enabled, egui::Button::new(egui::RichText::new(icon).size(theme::ICON)));
        if resp.on_hover_text(tip(id)).clicked() {
            asked = Some(id);
        }
    }
    asked
}

/// The marching ants (spec §5.6): the selection's outline as a dashed line that moves. Two passes
/// (white over black, half a dash apart) keep the dashes visible on any terrain. `edges` are cell
/// corners; `time` (seconds) moves the dashes. Drawn over the map with egui's painter.
pub fn draw_ants(painter: &egui::Painter, cam: &super::canvas::Cam, ppp: f32, edges: &[Edge], time: f64) {
    let offset = ((time * 12.0) % 8.0) as f32;
    let to_pt = |c: (i32, i32)| {
        let p = cam.to_screen(mq::vec2(c.0 as f32, c.1 as f32));
        egui::pos2(p.x / ppp, p.y / ppp)
    };
    let dark = Stroke::new(1.0, Color32::from_rgb(0x18, 0x18, 0x18));
    let light = Stroke::new(1.0, Color32::from_rgb(0xf4, 0xf4, 0xf4));
    for e in edges {
        let pts = [to_pt(e.a), to_pt(e.b)];
        painter.extend(egui::Shape::dashed_line_with_offset(&pts, light, &[4.0], &[4.0], offset));
        painter.extend(egui::Shape::dashed_line_with_offset(&pts, dark, &[4.0], &[4.0], offset + 4.0));
    }
}

/// The lasso in progress, as a polyline over the map.
pub fn draw_lasso(painter: &egui::Painter, cam: &super::canvas::Cam, ppp: f32, points: &[(f64, f64)]) {
    if points.len() < 2 {
        return;
    }
    let pts: Vec<egui::Pos2> = points
        .iter()
        .map(|&(x, y)| {
            let p = cam.to_screen(mq::vec2(x as f32, y as f32));
            egui::pos2(p.x / ppp, p.y / ppp)
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(1.5, Color32::from_rgb(0xf4, 0xf4, 0xf4))));
}

/// The outline a drag is making (the ants' preview while the pointer is down): the selection as it
/// would be combined with the shape.
pub fn preview_edges(ws: &mut Workspace, map: &str, sel: Option<&Mask>, w: u32, h: u32, shape: &SelShape, mode: Mode) -> Vec<Edge> {
    let Ok(shape) = shape_mask(ws, map, w, h, shape) else { return Vec::new() };
    let mut next = sel.cloned().unwrap_or_else(|| Mask::empty(w, h));
    next.combine(&shape, mode);
    next.outline()
}

/// The snapshot's selection options (`sel=`, applied once the map is compiled): `rect`, `lasso`,
/// `wand` (the start is the hovered cell, else the middle of the map), `paint` (a rectangle with a
/// stroke across it, painted only inside), `float` (a rectangle copied and floating over the map).
pub fn apply_snapshot(ws: &mut Workspace, maps: &mut MapView, canvas: &mut Canvas, set: &layers::Set, log: &mut Vec<String>) {
    if canvas.snap_sel.is_none() {
        return;
    }
    // The option waits until the map is compiled (its size is needed).
    let (Some((w, h)), Some(map)) = (dims(maps), maps.map.clone()) else { return };
    let Some(kind) = canvas.snap_sel.take() else { return };
    let (cx, cy) = (w as i32 / 2, h as i32 / 2);
    let rect = CellRect { x0: cx - 9, y0: cy - 6, x1: cx + 9, y1: cy + 6 };
    match kind.as_str() {
        "rect" => {
            canvas.tool = Tool::Marquee;
            set_selection(canvas, Some(Mask::of_rect(w, h, rect)));
        }
        "lasso" => {
            canvas.tool = Tool::Lasso;
            let pts: Vec<(f64, f64)> = (0..28)
                .map(|i| {
                    let t = f64::from(i) / 28.0 * std::f64::consts::TAU;
                    (f64::from(cx) + 0.5 + 15.0 * t.cos() + 3.0 * (3.0 * t).sin(), f64::from(cy) + 0.5 + 9.0 * t.sin())
                })
                .collect();
            set_selection(canvas, Some(Mask::of_polygon(w, h, &pts)));
        }
        "wand" => {
            canvas.tool = Tool::Wand;
            let start = canvas.snap_hover.unwrap_or((cx, cy));
            let start = (start.0.max(0) as u32, start.1.max(0) as u32);
            canvas.wand_contiguous = true;
            if let Ok(i) = ws.open_terrain(&map) {
                let m = Mask::of_wand(&ws.terrains[i].terrain.cells, w, h, start, true);
                set_selection(canvas, Some(m));
            }
        }
        "paint" => {
            canvas.tool = Tool::Terrain;
            canvas.only_sel = true;
            set_selection(canvas, Some(Mask::of_rect(w, h, rect)));
            let stroke = razdor::studio::brush::stroke((i64::from(cx) - 14, i64::from(cy) - 3), (i64::from(cx) + 14, i64::from(cy) + 3), canvas.brush, canvas.shape);
            let cells = select::keep_inside(stroke, within(canvas));
            if let Err(e) = ws.paint(&map, &cells, 14) {
                log.push(e);
            }
            ws.paint_end();
            repaint_terrain(ws, maps, &map, &cells);
        }
        "float" => {
            canvas.tool = Tool::Marquee;
            set_selection(canvas, Some(Mask::of_rect(w, h, rect)));
            if let Some(sel) = canvas.sel.clone() {
                match region::copy_selection(ws, &map, &sel, &target_of(set)) {
                    Ok(clip) => {
                        canvas.clip_origin = origin_of(&sel);
                        canvas.clip = Some(clip);
                        canvas.paste = true;
                        canvas.float = true;
                        canvas.snap_hover.get_or_insert((cx + 14, cy + 2));
                    }
                    Err(e) => log.push(e),
                }
            }
        }
        other => log.push(format!("no selection {other:?}: rect, lasso, wand, paint or float")),
    }
    if let Some(name) = canvas.snap_stamp.take() {
        save_stamp(ws, maps, canvas, set, &name, log);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_and_alt_override_the_bar_mode_only_while_selecting() {
        assert_eq!(select::mode_of(true, false, Mode::Replace), Mode::Add);
        assert_eq!(select::mode_of(false, true, Mode::Replace), Mode::Subtract);
        assert_eq!(select::mode_of(false, false, Mode::Add), Mode::Add);
    }

    #[test]
    fn a_shape_combines_with_the_selection_and_an_empty_result_is_none() {
        let mut canvas = Canvas::default();
        let a = Mask::of_rect(6, 1, CellRect { x0: 0, y0: 0, x1: 1, y1: 0 });
        apply_shape(&mut canvas, a.clone(), Mode::Replace);
        assert_eq!(canvas.sel.as_ref().map(Mask::count), Some(2));
        assert_eq!(canvas.sel_edges.len(), 6, "a 2-cell bar has six sides");
        let b = Mask::of_rect(6, 1, CellRect { x0: 1, y0: 0, x1: 4, y1: 0 });
        apply_shape(&mut canvas, b.clone(), Mode::Add);
        assert_eq!(canvas.sel.as_ref().map(Mask::count), Some(5));
        apply_shape(&mut canvas, Mask::all(6, 1), Mode::Subtract);
        assert!(canvas.sel.is_none(), "subtracting everything leaves no selection");
        assert!(canvas.sel_edges.is_empty());
    }

    #[test]
    fn only_in_selection_limits_the_paints_only_when_a_selection_exists() {
        let mut canvas = Canvas::default();
        assert!(within(&canvas).is_none(), "no selection: nothing limits a paint");
        set_selection(&mut canvas, Some(Mask::of_rect(4, 4, CellRect { x0: 0, y0: 0, x1: 0, y1: 0 })));
        assert!(within(&canvas).is_some(), "on by default");
        canvas.only_sel = false;
        assert!(within(&canvas).is_none(), "switched off");
    }

    /// The per-frame cost of the marching ants on the biggest map (200 by 200 cells): the worst case
    /// is a checkerboard, every cell its own outline. Printed with `--nocapture` (release build).
    #[test]
    fn the_ants_frame_cost_on_the_biggest_map() {
        let (w, h) = (200u32, 200u32);
        let cells: Vec<(i64, i64)> = (0..h).flat_map(|y| (0..w).map(move |x| (i64::from(x), i64::from(y)))).filter(|(x, y)| (x + y) % 2 == 0).collect();
        let sel = Mask::of_cells(w, h, &cells);
        let t0 = std::time::Instant::now();
        let edges = sel.outline();
        let outline_time = t0.elapsed();
        let t1 = std::time::Instant::now();
        let mut n = 0usize;
        for step in 0..60 {
            // One frame: the dashed lines of every edge at a new offset.
            let offset = step as f32 * 0.5;
            for e in &edges {
                // A cell is 16 points at 100 % zoom: the dashes of a real view.
                let pts = [egui::pos2(e.a.0 as f32 * 16.0, e.a.1 as f32 * 16.0), egui::pos2(e.b.0 as f32 * 16.0, e.b.1 as f32 * 16.0)];
                n += egui::Shape::dashed_line_with_offset(&pts, Stroke::new(1.0, Color32::WHITE), &[4.0], &[4.0], offset).len();
            }
        }
        let per_frame = t1.elapsed() / 60;
        eprintln!("ants: {} edges (checkerboard 200x200), outline {:?}, dashed shapes {} per frame, {:?} per frame", edges.len(), outline_time, n / 60, per_frame);
        assert!(!edges.is_empty());
    }
}
