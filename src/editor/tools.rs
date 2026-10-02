//! The original editor's tool state and what the mouse does with it
//! (docs/reference/editor/main-window.md §4, §6, §12–§14): five pages, a brush (delete, Info
//! or a size 1–6), the move mode, the held object and each page's palette choice. The window
//! turns mouse events into cells; everything else happens here, and what a press opens (a
//! record's editor, the lantern dialog, the scenario window) is left in
//! [`ToolState::opened`].

use std::sync::Arc;

use crate::i18n::{n_, tr};
use crate::trf;

pub use super::brush::{Held, Page};
use super::brush::{army_at, building_at, pick_up, Placed};
use super::cells::{figure_index, figure_kind};
use super::command::Command;
use super::doc::{EditorDoc, Target};
use super::geometry::brush_centre;
use super::naming::NamePools;
use super::palette::{BuildingPicture, Names, ObjectKey, Palette};

/// The brush of the delete button.
pub const DELETE: i32 = -1;
/// The brush of the Info button.
pub const INFO: i32 = 0;
/// The largest size button.
pub const MAX_SIZE: i32 = 6;

/// How the terrain page paints: the original's square brush, or Razdor's flood fill and
/// rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainShape {
    Brush,
    Fill,
    Rect,
}

/// What a press asks the window to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Open {
    Building(u16),
    Army(u8),
    Point(u16),
    /// A target place's editor (the original's target window; Razdor's point panel).
    Target(u16),
    /// The number dialog of a lantern's radius: `placed` for a new lantern.
    LanternRadius { id: u16, placed: bool },
    /// The scenario window on the tab of hero class `k` (0 knight … 2 ranger).
    HeroSettings(usize),
}

/// The original's cursors (§6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    /// Info or painting.
    Normal,
    /// Over an object in Info mode.
    OverObject,
    /// Over an object in move mode.
    OverObjectMove,
    Delete,
    Holding,
    /// A forbidden spot: a lantern or event point on a figure (the press is ignored), a
    /// building over mountains (a warning only).
    Forbidden,
}

/// A press of a mouse button on the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Press {
    /// The hovered cell.
    pub cell: (i32, i32),
    /// The map cell at the top-left of the view.
    pub view_origin: (i32, i32),
    /// The right button (else the left one).
    pub right: bool,
    /// Shift, Ctrl or Alt held.
    pub modifiers: bool,
}

/// What the tools read besides the document: the palette and the building name lists.
#[derive(Clone, Copy)]
pub struct Kit<'a> {
    pub palette: &'a Palette,
    pub names: Option<&'a Arc<NamePools>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    /// A stroke of the left button (one undo group); the last cell it acted on.
    Stroke((i32, i32)),
    /// A terrain rectangle from this corner (Razdor's).
    Rect((i32, i32)),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolState {
    pub page: Page,
    /// −1 delete, 0 Info, 1–6 a size; while a building is held, its footprint's width.
    pub brush: i32,
    pub move_mode: bool,
    pub held: Option<Held>,
    pub terrain: u8,
    pub shape: TerrainShape,
    pub hill: Option<ObjectKey>,
    pub forest: Option<ObjectKey>,
    pub building: Option<BuildingPicture>,
    /// The items page's kind: 0 none, 1–3 hero start, 4–7 army, 8 lantern, 9 event point.
    pub item: u8,
    /// The palette cell last clicked.
    pub palette_index: usize,
    /// Tree replacement mode (§14.1).
    pub tree_replace: bool,
    /// The building-place check (`FindBuildingPlace`, §14.2).
    pub building_check: bool,
    /// The "ignore mountains" check box, which starts as the opposite of the check.
    pub ignore_mountains: bool,
    /// The record whose panel is open (Razdor's form of the original's editors).
    pub selected: Option<Target>,
    /// What the last press asks the window to open, in order.
    pub opened: Vec<Open>,
    /// The last refused edit, for the status line.
    pub message: Option<String>,
    drag: Option<Drag>,
}

impl Default for ToolState {
    fn default() -> Self {
        ToolState::with_check(false)
    }
}

impl ToolState {
    /// The start-up state: the terrain page (§1.2 step 14) and the building-place check from
    /// the settings, its check box showing the opposite.
    pub fn with_check(building_check: bool) -> ToolState {
        ToolState {
            page: Page::Terrain,
            brush: 1,
            move_mode: false,
            held: None,
            terrain: 0,
            shape: TerrainShape::Brush,
            hill: None,
            forest: None,
            building: None,
            item: 0,
            palette_index: 0,
            tree_replace: false,
            building_check,
            ignore_mountains: !building_check,
            selected: None,
            opened: Vec::new(),
            message: None,
            drag: None,
        }
    }

    /// Choosing a page (0x5aa0f8): move mode off, brush 1 (3 on the hills and forests
    /// pages), then a click on the first palette cell.
    pub fn choose_page(&mut self, page: Page, palette: &Palette) {
        self.move_mode = false;
        self.drag = None;
        self.page = page;
        self.brush = match page {
            Page::Hills | Page::Forests => 3,
            _ => 1,
        };
        self.click_palette(0, palette);
    }

    /// Which size buttons the page enables: only size 1 on the buildings and items pages;
    /// the delete button is off on the terrain page.
    pub fn size_enabled(&self, brush: i32) -> bool {
        match brush {
            DELETE => self.page != Page::Terrain,
            INFO | 1 => true,
            _ => !matches!(self.page, Page::Buildings | Page::Items),
        }
    }

    /// A size, Info or delete button (0x5a4100): move mode off and that brush. The palette
    /// choice stays (on the hills page a hill of another size until the palette is clicked).
    pub fn press_size(&mut self, brush: i32) {
        if !self.size_enabled(brush) {
            return;
        }
        self.move_mode = false;
        self.brush = brush.clamp(DELETE, MAX_SIZE);
    }

    /// Space (0x5b3714): the Info tool and move mode off; a held object stays held.
    pub fn space(&mut self) {
        self.move_mode = false;
        self.brush = INFO;
    }

    /// The move menu item (0x5b1df0): move mode on and the Info brush.
    pub fn start_move(&mut self) {
        self.move_mode = true;
        self.brush = INFO;
    }

    /// The "ignore mountains" check box or menu item (0x5b25e8, 0x5b2654): the box flips and
    /// the check takes its new state as it is, so after the first toggle a ticked box means
    /// the check is on (the original's inversion, kept).
    pub fn toggle_ignore_mountains(&mut self) {
        self.ignore_mountains = !self.ignore_mountains;
        self.building_check = self.ignore_mountains;
    }

    /// The palette cell `index` (0x5aaa7c), as the page lists it ([`palette_cells`]). A click
    /// while Info or delete is pressed switches to size 1 (on the hills page after the hill
    /// lookup, which needs a size, so that click selects nothing).
    pub fn click_palette(&mut self, index: usize, palette: &Palette) {
        self.palette_index = index;
        let mut size = self.brush.max(1);
        match self.page {
            Page::Terrain => {
                if index < 16 {
                    self.terrain = index as u8;
                }
            }
            Page::Hills => {
                if self.brush > 0 {
                    if let Some(o) = palette.hills(self.brush as u32).get(index) {
                        self.hill = Some(*o);
                    }
                }
            }
            Page::Forests => {
                let list = if (2..=6).contains(&self.brush) { palette.forests(2) } else { palette.forests(1) };
                if let Some(o) = list.get(index) {
                    self.forest = Some(*o);
                }
            }
            Page::Buildings => {
                if let Some(b) = palette.buildings.get(index) {
                    self.building = Some(*b);
                    size = b.brush as i32;
                }
            }
            Page::Items => self.item = index as u8 + 1,
        }
        if self.brush <= INFO {
            self.move_mode = false;
        }
        self.brush = size;
    }

    fn inside(doc: &EditorDoc, c: (i32, i32)) -> bool {
        c.0 >= 0 && c.1 >= 0 && (c.0 as u32) < doc.scenario.width() && (c.1 as u32) < doc.scenario.height()
    }

    /// A lantern or event point refused on a figure: the original tests the cell at the
    /// hovered *view* position counted from the map's corner, without the view origin
    /// (0x5ad4da), so once scrolled it tests an unrelated cell (the original's bug, kept).
    fn point_blocked(&self, doc: &EditorDoc, cell: (i32, i32), origin: (i32, i32)) -> bool {
        self.page == Page::Items && matches!(self.item, 8 | 9) && doc.cells.figure((cell.0 - origin.0) as i64, (cell.1 - origin.1) as i64) != 0
    }

    /// A building over mountains with the check on (0x5ad3b4): the footprint plus one column
    /// on the left and one row on top, from the brush centre, holding a mark −5..−7 (rocks,
    /// −8, are not tested). The palette building's footprint is used even for a held one.
    pub fn over_mountains(&self, doc: &EditorDoc, cell: (i32, i32)) -> bool {
        let holding = matches!(self.held, Some(Held::Building(_)));
        if self.page != Page::Buildings || !self.building_check || (self.building.is_none() && !holding) {
            return false;
        }
        let Some(b) = self.building else { return false };
        let (cx, cy) = brush_centre(cell, self.brush);
        let (cx, cy) = (cx as i64, cy as i64);
        let (sx, sy) = (b.size.0 as i64, b.size.1 as i64);
        (cx - sx..=cx).any(|x| (cy - sy..=cy).any(|y| Self::inside(doc, (x as i32, y as i32)) && (-7..=-5).contains(&doc.cells.mark(x, y))))
    }

    /// The cursor over `cell` (§6.2).
    pub fn cursor(&self, doc: &EditorDoc, cell: (i32, i32), origin: (i32, i32)) -> Cursor {
        if self.point_blocked(doc, cell, origin) || self.over_mountains(doc, cell) {
            return Cursor::Forbidden;
        }
        match self.brush {
            DELETE => Cursor::Delete,
            INFO if self.held.is_none() => {
                let (x, y) = (cell.0 as i64, cell.1 as i64);
                let over = doc.cells.figure(x, y) != 0 || building_at(&doc.scenario, x, y, None).is_some();
                match (over, self.move_mode) {
                    (true, true) => Cursor::OverObjectMove,
                    (true, false) => Cursor::OverObject,
                    _ => Cursor::Normal,
                }
            }
            _ if self.held.is_some() => Cursor::Holding,
            _ => Cursor::Normal,
        }
    }

    fn run(&mut self, doc: &mut EditorDoc, cmd: Command) -> Option<crate::editor::Applied> {
        match doc.apply(cmd) {
            Ok(a) => {
                self.message = None;
                Some(a)
            }
            Err(e) => {
                self.message = Some(e.to_string());
                None
            }
        }
    }

    /// A mouse button went down on the map (§6.3).
    pub fn press(&mut self, doc: &mut EditorDoc, kit: Kit, p: Press) {
        self.opened.clear();
        // Only exact button states count; a lantern or event point on a figure is refused.
        if p.modifiers || !Self::inside(doc, p.cell) || self.point_blocked(doc, p.cell, p.view_origin) {
            return;
        }
        let (x, y) = (p.cell.0 as i64, p.cell.1 as i64);
        let pick = self.brush == INFO && self.held.is_none() && (if p.right { !self.move_mode } else { self.move_mode });
        if pick {
            self.held = pick_up(&doc.scenario, &mut doc.cells, x, y);
            self.brush = match self.held {
                Some(Held::Building(id)) => doc.scenario.building(id).map_or(1, |b| b.size_x as i32),
                Some(_) => 1,
                None => self.brush,
            };
            return;
        }
        if p.right {
            return;
        }
        let centre = brush_centre(p.cell, self.brush);
        if let Some(held) = self.held.take() {
            self.run(doc, Command::Drop { held, x: centre.0, y: centre.1 });
            doc.mark_modified();
            self.brush = INFO;
            return;
        }
        if self.brush == INFO {
            self.info_click(doc, p.cell);
            return;
        }
        doc.begin_group(match (self.brush, self.page) {
            (DELETE, _) => n_("Delete"),
            (_, Page::Terrain) => n_("Paint terrain"),
            (_, Page::Hills | Page::Forests) => n_("Place objects"),
            (_, Page::Buildings) => n_("Place building"),
            (_, Page::Items) => n_("Place item"),
        });
        self.act(doc, kit, p.cell);
        doc.mark_modified();
        self.drag = Some(match (self.page, self.shape) {
            (Page::Terrain, TerrainShape::Rect) if self.brush > 0 => Drag::Rect(p.cell),
            _ => Drag::Stroke(p.cell),
        });
    }

    /// The paint or delete of a press, at the hovered cell.
    fn act(&mut self, doc: &mut EditorDoc, kit: Kit, cell: (i32, i32)) {
        let (cx, cy) = brush_centre(cell, self.brush);
        if self.brush == DELETE {
            let (x, y) = (cell.0 as u16, cell.1 as u16);
            self.run(doc, Command::DeleteAt { x, y, page: self.page });
            return;
        }
        let size = self.brush as u32;
        match self.page {
            Page::Terrain => match self.shape {
                TerrainShape::Brush => {
                    let _ = doc.apply(Command::PaintTerrain { x: cx, y: cy, size, code: self.terrain });
                }
                TerrainShape::Fill => {
                    self.run(doc, Command::FillTerrain { x: cell.0 as u32, y: cell.1 as u32, code: self.terrain });
                }
                TerrainShape::Rect => {}
            },
            Page::Hills | Page::Forests => {
                let key = if self.page == Page::Hills { self.hill } else { self.forest };
                let Some(o) = key else { return };
                let facts = kit.palette.forest_facts();
                let cmd = Command::PlaceObject { x: cx, y: cy, size, class: o.class, sprite: o.sprite, facts, replace: self.tree_replace };
                // A square over the edge does nothing, as in the original.
                let _ = doc.apply(cmd);
            }
            Page::Buildings => {
                let Some(b) = self.building else { return };
                let names = kit.names.cloned();
                self.run(doc, Command::PlaceBuilding { x: cx, y: cy, picture_type: b.picture_type, variant: b.variant, size: b.size, brush: size, names });
            }
            Page::Items => {
                if self.item == 0 {
                    return;
                }
                let applied = self.run(doc, Command::PlaceItem { x: cx as u16, y: cy as u16, kind: self.item });
                // A full list leaves the tool as it is; anything else returns to Info.
                let Some(a) = applied else { return };
                match a.placed {
                    Some(Placed::Army(id)) => self.opened.push(Open::Army(id)),
                    Some(Placed::Point(id)) => self.opened.push(match self.item {
                        8 => Open::LanternRadius { id, placed: true },
                        9 => Open::Point(id),
                        _ => Open::Target(id),
                    }),
                    _ => {}
                }
                self.item = 0;
                self.brush = INFO;
                self.move_mode = false;
            }
        }
    }

    /// A left press with the Info tool (§13.2): by the cell's figure word.
    fn info_click(&mut self, doc: &EditorDoc, cell: (i32, i32)) {
        let (x, y) = (cell.0 as i64, cell.1 as i64);
        let word = doc.cells.figure(x, y);
        if word == 0 {
            if let Some(id) = building_at(&doc.scenario, x, y, None) {
                self.opened.push(Open::Building(id));
            }
            return;
        }
        let (kind, index) = (figure_kind(word), figure_index(word));
        if index > 0 {
            if let Some(id) = army_at(&doc.scenario, x, y) {
                self.opened.push(Open::Army(id));
            }
        } else if (1..=3).contains(&kind) {
            self.opened.push(Open::HeroSettings(kind as usize - 1));
        }
        let point = index as u16;
        let exists = point >= 1 && (point as usize) <= doc.scenario.points.len();
        match kind {
            8 if exists => self.opened.push(Open::LanternRadius { id: point, placed: false }),
            9 if exists => self.opened.push(Open::Point(point)),
            10 if exists => self.opened.push(Open::Target(point)),
            _ => {}
        }
    }

    /// The mouse moved to `cell` with the left button held: the terrain and forest pages go on
    /// painting (or deleting); the others act once per press.
    pub fn drag_to(&mut self, doc: &mut EditorDoc, kit: Kit, cell: (i32, i32)) {
        let Some(Drag::Stroke(last)) = self.drag else { return };
        if last == cell || !matches!(self.page, Page::Terrain | Page::Forests) || !Self::inside(doc, cell) {
            return;
        }
        if self.page == Page::Terrain && self.shape != TerrainShape::Brush && self.brush > 0 {
            return;
        }
        self.act(doc, kit, cell);
        self.drag = Some(Drag::Stroke(cell));
    }

    /// The rectangle being dragged on the terrain page (corner, for the preview).
    pub fn rect_start(&self) -> Option<(i32, i32)> {
        match self.drag {
            Some(Drag::Rect(c)) => Some(c),
            _ => None,
        }
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The button came up on `cell`: the stroke is one undo step.
    pub fn release(&mut self, doc: &mut EditorDoc, cell: (i32, i32)) {
        if let Some(Drag::Rect(from)) = self.drag {
            let _ = doc.apply(Command::RectTerrain { from, to: cell, code: self.terrain });
        }
        if self.drag.take().is_some() {
            doc.end_group();
        }
    }

    /// Deletes the selected record (Razdor's Delete key and panels).
    pub fn delete_selected(&mut self, doc: &mut EditorDoc) {
        let Some(t) = self.selected.take() else { return };
        let cmd = match t {
            Target::Building(id) => Command::DeleteBuilding { id },
            Target::Army(id) => Command::DeleteArmy { id },
            Target::Point(id) => Command::DeletePoint { id },
        };
        self.run(doc, cmd);
    }

    /// After undo/redo: drop a selection or a held object whose record is gone.
    pub fn check_selection(&mut self, doc: &EditorDoc) {
        let s = &doc.scenario;
        let exists = |t: Target| match t {
            Target::Building(id) => (id as usize) <= s.buildings.len() && id > 0,
            Target::Army(id) => (id as usize) <= s.armies.len() && id > 0,
            Target::Point(id) => (id as usize) <= s.points.len() && id > 0,
        };
        if self.selected.is_some_and(|t| !exists(t)) {
            self.selected = None;
        }
        let held = self.held.map(|h| match h {
            Held::Building(id) => Target::Building(id),
            Held::Army(id) => Target::Army(id),
            Held::Point(id) => Target::Point(id),
        });
        if held.is_some_and(|t| !exists(t)) {
            self.held = None;
            self.brush = INFO;
        }
    }

    /// The army whose patrol zone the overlay shows: the one under the cursor in Info mode
    /// (§13.1), found by the figure word.
    pub fn hovered_army(&self, doc: &EditorDoc, cell: (i32, i32)) -> Option<u8> {
        if self.brush != INFO || self.held.is_some() {
            return None;
        }
        let (x, y) = (cell.0 as i64, cell.1 as i64);
        (4..=7).contains(&figure_kind(doc.cells.figure(x, y))).then(|| army_at(&doc.scenario, x, y)).flatten()
    }

    /// The Info mode's hint over `cell` (§13.1), outside move mode: a hero start, else the
    /// building covering the cell, else the army, else the point of the figure word.
    pub fn hint(&self, doc: &EditorDoc, names: &Names, cell: (i32, i32)) -> Vec<String> {
        if self.brush != INFO || self.held.is_some() || self.move_mode || !Self::inside(doc, cell) {
            return Vec::new();
        }
        let s = &doc.scenario;
        let (x, y) = (cell.0 as i64, cell.1 as i64);
        let word = doc.cells.figure(x, y);
        let (kind, index) = (figure_kind(word), figure_index(word));
        if word != 0 && index == 0 && (1..=3).contains(&kind) {
            return vec![trf!("Hero start: {class}", class = tr(super::palette::HERO_CLASSES[kind as usize - 1]))];
        }
        if let Some(id) = building_at(s, x, y, None) {
            let b = &s.buildings[id as usize - 1];
            let mut out = vec![trf!("Building {id}: {name} (type {kind}, picture {t}, {v})", id, name = b.name, kind = b.kind, t = b.picture_type, v = b.picture_variant)];
            match b.owner_army {
                o @ 1..=254 => {
                    let a = s.army(o);
                    out.push(trf!("Owner: army {o}, {name} ({leader})", o, name = a.map_or("", |a| a.name.as_str()), leader = a.map_or("", |a| a.leader_name.as_str())));
                }
                _ => out.push(trf!("Owner: {name}", name = b.owner_name)),
            }
            out.push(trf!("Income: {gold} gold, {mana} mana", gold = b.gold_per_day, mana = b.mana_per_day));
            if !b.description.is_empty() {
                out.push(tr("Has a description").into());
            }
            return out;
        }
        if (4..=7).contains(&kind) {
            if let Some(a) = army_at(s, x, y).and_then(|id| s.army(id)) {
                return vec![trf!("Army {id}: {name}", id = a.id, name = a.name), trf!("Tactical cost: {a} / {b}", a = a.tactical_cost_1, b = a.tactical_cost_2)];
            }
        }
        if (8..=9).contains(&kind) {
            if let Some(p) = (index as usize).checked_sub(1).and_then(|i| s.points.get(i)) {
                let mut out = Vec::new();
                if p.event_count != 0 {
                    out.push(trf!("Events: {n}", n = p.event_count));
                    for e in p.event_slots.iter().take((p.event_count as usize).min(5)) {
                        if let Some(ev) = (*e as usize).checked_sub(1).and_then(|i| s.events.get(i)) {
                            out.push(super::events::split_title(&ev.title).name);
                        }
                    }
                }
                if p.radius != 0 {
                    out.push(trf!("Lantern radius: {r}", r = p.radius));
                }
                let _ = names;
                return out;
            }
        }
        Vec::new()
    }
}

/// What a page's palette shows at the current brush (§4.2, 0x5aaa7c), cell by cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteCells {
    Terrain,
    Objects(Vec<ObjectKey>),
    Buildings(Vec<BuildingPicture>),
    /// Kinds 1–9.
    Items,
}

pub fn palette_cells(t: &ToolState, palette: &Palette) -> PaletteCells {
    match t.page {
        Page::Terrain => PaletteCells::Terrain,
        Page::Hills => PaletteCells::Objects(palette.hills(t.brush.max(1) as u32)),
        Page::Forests => PaletteCells::Objects(palette.forests(if (2..=6).contains(&t.brush) { 2 } else { 1 })),
        Page::Buildings => PaletteCells::Buildings(palette.buildings.clone()),
        Page::Items => PaletteCells::Items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::NewMap;

    fn doc() -> EditorDoc {
        EditorDoc::new_map(NewMap { width: 20, height: 20, fill: 6 })
    }

    fn press(cell: (i32, i32)) -> Press {
        Press { cell, view_origin: (0, 0), right: false, modifiers: false }
    }

    fn kit(p: &Palette) -> Kit<'_> {
        Kit { palette: p, names: None }
    }

    #[test]
    fn pages_set_their_brush_and_palette() {
        let p = Palette::fallback();
        let mut t = ToolState::default();
        assert_eq!((t.page, t.brush), (Page::Terrain, 1));
        t.choose_page(Page::Hills, &p);
        assert_eq!((t.brush, t.hill), (3, None), "the fallback has no 3-cell hill");
        t.press_size(1);
        t.click_palette(0, &p);
        assert_eq!(t.hill, Some(ObjectKey { class: 1, sprite: 10 }));
        t.choose_page(Page::Buildings, &p);
        assert_eq!((t.brush, t.building.map(|b| b.picture_type)), (7, Some(1)), "a town's brush is its width");
        assert!(!t.size_enabled(2) && t.size_enabled(1) && t.size_enabled(DELETE));
        t.choose_page(Page::Items, &p);
        assert_eq!((t.item, t.brush), (1, 1));
        t.choose_page(Page::Terrain, &p);
        assert!(!t.size_enabled(DELETE));
        // A palette click from Info or delete switches to size 1; on the hills page that
        // click selects nothing.
        t.choose_page(Page::Hills, &p);
        t.press_size(INFO);
        let before = t.hill;
        t.click_palette(0, &p);
        assert_eq!((t.brush, t.hill), (1, before));
    }

    #[test]
    fn presses_with_modifiers_or_outside_do_nothing() {
        let p = Palette::fallback();
        let (mut d, mut t) = (doc(), ToolState::default());
        t.terrain = 2;
        t.press(&mut d, kit(&p), Press { modifiers: true, ..press((3, 3)) });
        t.press(&mut d, kit(&p), press((-1, 3)));
        assert!(!d.can_undo() && d.scenario.terrain.iter().all(|c| *c == 6));
    }

    #[test]
    fn terrain_and_forests_drag_the_others_act_once() {
        let p = Palette::fallback();
        let (mut d, mut t) = (doc(), ToolState::default());
        t.terrain = 4;
        t.press_size(2);
        t.press(&mut d, kit(&p), press((0, 5)));
        t.drag_to(&mut d, kit(&p), (3, 5));
        t.release(&mut d, (3, 5));
        // Size 2 ending at (1, 6) and at (4, 6): 8 cells, no line between.
        assert_eq!(d.scenario.terrain.iter().filter(|c| **c == 4).count(), 8);
        assert_eq!(d.scenario.terrain[6 * 20 + 1], 4);
        assert!(d.undo() && d.scenario.terrain.iter().all(|c| *c == 6), "a stroke is one undo step");
        // Hills act once per press.
        t.choose_page(Page::Hills, &p);
        t.press_size(1);
        t.click_palette(0, &p);
        t.press(&mut d, kit(&p), press((5, 5)));
        t.drag_to(&mut d, kit(&p), (8, 8));
        t.release(&mut d, (8, 8));
        assert_eq!(d.scenario.objects.len(), 1);
    }

    #[test]
    fn items_return_to_info_and_open_their_editor() {
        let p = Palette::fallback();
        let (mut d, mut t) = (doc(), ToolState::default());
        t.choose_page(Page::Items, &p);
        t.click_palette(4, &p);
        assert_eq!(t.item, 5);
        t.press(&mut d, kit(&p), press((4, 4)));
        assert_eq!((d.scenario.armies.len(), t.brush, t.item, t.opened.clone()), (1, INFO, 0, vec![Open::Army(1)]));
        assert!(d.dirty());
        // A lantern asks for its radius.
        t.click_palette(7, &p);
        t.press(&mut d, kit(&p), press((8, 8)));
        assert_eq!(t.opened, vec![Open::LanternRadius { id: 1, placed: true }]);
        // In Info mode a click opens by the figure word: the army, the lantern dialog.
        t.press(&mut d, kit(&p), press((4, 4)));
        assert_eq!(t.opened, vec![Open::Army(1)]);
        t.press(&mut d, kit(&p), press((8, 8)));
        assert_eq!(t.opened, vec![Open::LanternRadius { id: 1, placed: false }]);
        // A hero start opens the scenario window on its class's tab.
        t.click_palette(2, &p);
        t.press(&mut d, kit(&p), press((12, 3)));
        t.press(&mut d, kit(&p), press((12, 3)));
        assert_eq!(t.opened, vec![Open::HeroSettings(2)]);
    }

    #[test]
    fn a_point_on_a_figure_is_refused_at_the_unscrolled_view_cell() {
        let p = Palette::fallback();
        let (mut d, mut t) = (doc(), ToolState::default());
        t.choose_page(Page::Items, &p);
        t.click_palette(3, &p);
        t.press(&mut d, kit(&p), press((2, 2)));
        // Unscrolled, a lantern on the army's cell is refused.
        t.click_palette(7, &p);
        assert_eq!(t.cursor(&d, (2, 2), (0, 0)), Cursor::Forbidden);
        t.press(&mut d, kit(&p), press((2, 2)));
        assert!(d.scenario.points.is_empty());
        // Scrolled by (5, 5): the view cell (2, 2) is map cell (7, 7), and the test still
        // reads map cell (2, 2): the empty (7, 7) is refused, the army's cell would not be.
        let scrolled = Press { view_origin: (5, 5), ..press((7, 7)) };
        t.press(&mut d, kit(&p), scrolled);
        assert!(d.scenario.points.is_empty());
        t.press(&mut d, kit(&p), Press { view_origin: (5, 5), ..press((6, 6)) });
        assert_eq!(d.scenario.points.len(), 1);
    }

    #[test]
    fn pick_up_by_right_press_drop_without_checks() {
        let p = Palette::fallback();
        let (mut d, mut t) = (doc(), ToolState::default());
        t.choose_page(Page::Items, &p);
        t.click_palette(3, &p);
        t.press(&mut d, kit(&p), press((2, 2)));
        assert_eq!(t.brush, INFO);
        t.press(&mut d, kit(&p), Press { right: true, ..press((2, 2)) });
        assert_eq!((t.held, t.cursor(&d, (5, 5), (0, 0))), (Some(Held::Army(1)), Cursor::Holding));
        t.press(&mut d, kit(&p), press((19, 19)));
        assert_eq!((d.scenario.armies[0].x, d.scenario.armies[0].y, t.held, t.brush), (19, 19, None, INFO));
        // Move mode: a left press picks up and stays in move mode after the drop.
        t.start_move();
        t.press(&mut d, kit(&p), press((19, 19)));
        assert_eq!(t.held, Some(Held::Army(1)));
        t.press(&mut d, kit(&p), press((0, 0)));
        assert!(t.move_mode && t.held.is_none());
        // Space leaves move mode but keeps what is held.
        t.press(&mut d, kit(&p), press((0, 0)));
        t.space();
        assert!(!t.move_mode && t.held.is_some());
    }

    #[test]
    fn the_ignore_mountains_box_inverts() {
        let p = Palette::fallback();
        let mut t = ToolState::with_check(true);
        assert!(t.building_check && !t.ignore_mountains, "the box starts as the opposite");
        t.toggle_ignore_mountains();
        assert!(t.ignore_mountains && t.building_check, "after a toggle the ticked box means checking");
        t.toggle_ignore_mountains();
        assert!(!t.building_check);
        // The warning: mountains (−5) in the footprint plus a column left and a row on top.
        let mut d = doc();
        d.cells.set_mark(5, 5, -5);
        t.toggle_ignore_mountains();
        t.choose_page(Page::Buildings, &p);
        t.click_palette(4, &p);
        let b = t.building.unwrap();
        assert_eq!((b.size, t.brush), ((2, 2), 2));
        // Hovering (6, 6) puts the centre at (7, 7): x 5..7, y 5..7 are tested.
        assert_eq!(t.cursor(&d, (6, 6), (0, 0)), Cursor::Forbidden);
        assert_eq!(t.cursor(&d, (7, 6), (0, 0)), Cursor::Normal);
        d.cells.set_mark(5, 5, -8);
        assert_eq!(t.cursor(&d, (6, 6), (0, 0)), Cursor::Normal, "rocks are not tested");
        // The press is still accepted.
        d.cells.set_mark(5, 5, -5);
        t.press(&mut d, kit(&p), press((6, 6)));
        assert_eq!(d.scenario.buildings.len(), 1);
    }

    #[test]
    fn delete_marks_modified_even_when_nothing_goes() {
        let p = Palette::fallback();
        let (mut d, mut t) = (doc(), ToolState::default());
        d.mark_unmodified();
        t.choose_page(Page::Buildings, &p);
        t.press_size(DELETE);
        t.press(&mut d, kit(&p), press((3, 3)));
        assert!(d.dirty() && d.scenario.buildings.is_empty());
    }
}
