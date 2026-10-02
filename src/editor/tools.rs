//! The editing tools and what a press, drag and release on the map do with them. The window
//! only turns mouse events into cells; everything else happens here.

use crate::i18n::{n_, tr};

use super::command::{Command, ObjectFilter};
use super::doc::{EditorDoc, Target};
use super::palette::Palette;

/// How the terrain tool paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainShape {
    /// A square brush of this size (1, 3, 5 or 9).
    Brush(u32),
    Fill,
    Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Pick a record; drag to move it.
    Select,
    Terrain { code: u8, shape: TerrainShape },
    Objects { class: u8, sprite: u8, size: u32 },
    Erase { size: u32, filter: ObjectFilter },
    Building { kind: u8, picture_type: u8, variant: u8 },
    /// Place an army of map model 4–7 (feudal, rogue, peasant, inactive).
    Army { model: u8 },
    /// Place a point of model 8 (lantern), 9 (event point) or 10 (AI target).
    Point { model: u8 },
    /// Click the start cell of a hero preset (0 knight, 1 archmage, 2 ranger).
    HeroStart(usize),
}

impl Tool {
    pub fn label(&self) -> &'static str {
        tr(match self {
            Tool::Select => n_("Select and move"),
            Tool::Terrain { .. } => n_("Terrain"),
            Tool::Objects { .. } => n_("Objects"),
            Tool::Erase { .. } => n_("Erase objects"),
            Tool::Building { .. } => n_("Buildings"),
            Tool::Army { .. } => n_("Armies"),
            Tool::Point { .. } => n_("Points and lanterns"),
            Tool::HeroStart(_) => n_("Hero start"),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    /// A brush stroke (one undo group); the last cell painted.
    Stroke((i32, i32)),
    /// A terrain rectangle from this corner.
    Rect((i32, i32)),
    /// Moving a record, grabbed this many cells from its anchor; one undo group.
    Move(Target, (i32, i32)),
}

/// The current tool, the selection and a drag in progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolState {
    pub tool: Tool,
    pub selected: Option<Target>,
    drag: Option<Drag>,
    /// The last refused edit, for the status line.
    pub message: Option<String>,
}

impl Default for ToolState {
    fn default() -> Self {
        ToolState { tool: Tool::Select, selected: None, drag: None, message: None }
    }
}

/// Cells of the line from `a` to `b`, `a` excluded (Bresenham), so a fast drag leaves no gaps.
pub fn line(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let (dx, dy) = ((b.0 - a.0).abs(), -(b.1 - a.1).abs());
    let (sx, sy) = ((b.0 - a.0).signum(), (b.1 - a.1).signum());
    let (mut x, mut y, mut err) = (a.0, a.1, dx + dy);
    let mut out = Vec::new();
    while (x, y) != b {
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
        out.push((x, y));
    }
    out
}

impl ToolState {
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.drag = None;
    }

    /// The rectangle being dragged by the terrain tool (corner, for the preview).
    pub fn rect_start(&self) -> Option<(i32, i32)> {
        match self.drag {
            Some(Drag::Rect(c)) => Some(c),
            _ => None,
        }
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    fn run(&mut self, doc: &mut EditorDoc, cmd: Command) -> Option<u32> {
        match doc.apply(cmd) {
            Ok(a) => {
                self.message = None;
                a.new_id
            }
            Err(e) => {
                self.message = Some(e.to_string());
                None
            }
        }
    }

    fn stroke(&mut self, doc: &mut EditorDoc, cell: (i32, i32)) {
        let (x, y) = cell;
        let cmd = match self.tool {
            Tool::Terrain { code, shape: TerrainShape::Brush(size) } => Command::PaintTerrain { x, y, size, code },
            Tool::Objects { class, sprite, size } => Command::PlaceObjects { x, y, size, class, sprite },
            Tool::Erase { size, filter } => Command::EraseObjects { x, y, size, filter },
            _ => return,
        };
        // Brushes may hang over the edge; only a cell outside is refused.
        let _ = doc.apply(cmd);
    }

    /// The mouse went down on `cell`.
    pub fn press(&mut self, doc: &mut EditorDoc, palette: &Palette, cell: (i32, i32)) {
        let inside = cell.0 >= 0 && cell.1 >= 0 && (cell.0 as u32) < doc.scenario.width() && (cell.1 as u32) < doc.scenario.height();
        if !inside {
            return;
        }
        let (x, y) = (cell.0 as u16, cell.1 as u16);
        match self.tool {
            Tool::Select => {
                self.selected = doc.hit(cell.0, cell.1);
                if let Some(t) = self.selected {
                    let s = &doc.scenario;
                    let anchor = match t {
                        Target::Building(id) => s.building(id).map(|b| (b.x, b.y)),
                        Target::Army(id) => s.army(id).map(|a| (a.x, a.y)),
                        Target::Point(id) => s.points.get(id as usize - 1).map(|p| (p.x, p.y)),
                    };
                    let (ax, ay) = anchor.unwrap_or((x, y));
                    let grab = (ax as i32 - cell.0, ay as i32 - cell.1);
                    doc.begin_group(match t {
                        Target::Building(_) => n_("Move building"),
                        Target::Army(_) => n_("Move army"),
                        Target::Point(_) => n_("Move point"),
                    });
                    self.drag = Some(Drag::Move(t, grab));
                }
            }
            Tool::Terrain { code, shape } => match shape {
                TerrainShape::Brush(_) => {
                    doc.begin_group(n_("Paint terrain"));
                    self.stroke(doc, cell);
                    self.drag = Some(Drag::Stroke(cell));
                }
                TerrainShape::Fill => {
                    self.run(doc, Command::FillTerrain { x: x as u32, y: y as u32, code });
                }
                TerrainShape::Rect => self.drag = Some(Drag::Rect(cell)),
            },
            Tool::Objects { .. } | Tool::Erase { .. } => {
                doc.begin_group(if matches!(self.tool, Tool::Objects { .. }) { n_("Place objects") } else { n_("Erase objects") });
                self.stroke(doc, cell);
                self.drag = Some(Drag::Stroke(cell));
            }
            Tool::Building { kind, picture_type, variant } => {
                let size = palette.footprint(picture_type, variant);
                if let Some(id) = self.run(doc, Command::PlaceBuilding { x, y, kind, picture_type, variant, size }) {
                    self.selected = Some(Target::Building(id as u16));
                }
            }
            Tool::Army { model } => {
                if let Some(id) = self.run(doc, Command::PlaceArmy { x, y, model }) {
                    self.selected = Some(Target::Army(id as u8));
                }
            }
            Tool::Point { model } => {
                if let Some(id) = self.run(doc, Command::PlacePoint { x, y, model }) {
                    self.selected = Some(Target::Point(id as u16));
                }
            }
            Tool::HeroStart(k) => {
                let mut st = doc.settings();
                if let Some(h) = st.header.heroes.get_mut(k) {
                    (h.x, h.y) = (x, y);
                    self.run(doc, Command::SetSettings(Box::new(st)));
                }
                self.set_tool(Tool::Select);
            }
        }
    }

    /// The mouse moved to `cell` with the button held.
    pub fn drag_to(&mut self, doc: &mut EditorDoc, cell: (i32, i32)) {
        match self.drag {
            Some(Drag::Stroke(last)) if last != cell => {
                for c in line(last, cell) {
                    self.stroke(doc, c);
                }
                self.drag = Some(Drag::Stroke(cell));
            }
            Some(Drag::Move(t, grab)) => {
                let cell = (cell.0 + grab.0, cell.1 + grab.1);
                let (w, h) = (doc.scenario.width() as i32, doc.scenario.height() as i32);
                if cell.0 < 0 || cell.1 < 0 || cell.0 >= w || cell.1 >= h {
                    return;
                }
                let (x, y) = (cell.0 as u16, cell.1 as u16);
                let cmd = match t {
                    Target::Building(id) => Command::MoveBuilding { id, x, y },
                    Target::Army(id) => Command::MoveArmy { id, x, y },
                    Target::Point(id) => Command::MovePoint { id, x, y },
                };
                // A footprint that would leave the map just stays where it was.
                let _ = doc.apply(cmd);
            }
            _ => {}
        }
    }

    /// The mouse button came up on `cell`.
    pub fn release(&mut self, doc: &mut EditorDoc, cell: (i32, i32)) {
        match self.drag.take() {
            Some(Drag::Rect(from)) => {
                if let Tool::Terrain { code, .. } = self.tool {
                    self.run(doc, Command::RectTerrain { from, to: cell, code });
                }
            }
            Some(Drag::Stroke(_) | Drag::Move(..)) => doc.end_group(),
            None => {}
        }
    }

    /// Deletes the selected record.
    pub fn delete_selected(&mut self, doc: &mut EditorDoc) {
        let Some(t) = self.selected.take() else { return };
        let cmd = match t {
            Target::Building(id) => Command::DeleteBuilding { id },
            Target::Army(id) => Command::DeleteArmy { id },
            Target::Point(id) => Command::DeletePoint { id },
        };
        self.run(doc, cmd);
    }

    /// After undo/redo: drop a selection whose record is gone.
    pub fn check_selection(&mut self, doc: &EditorDoc) {
        let s = &doc.scenario;
        let ok = match self.selected {
            Some(Target::Building(id)) => (id as usize) <= s.buildings.len() && id > 0,
            Some(Target::Army(id)) => (id as usize) <= s.armies.len() && id > 0,
            Some(Target::Point(id)) => (id as usize) <= s.points.len() && id > 0,
            None => true,
        };
        if !ok {
            self.selected = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::NewMap;

    fn doc() -> EditorDoc {
        EditorDoc::new_map(NewMap { width: 20, height: 20, fill: 6 })
    }

    #[test]
    fn lines_have_no_gaps() {
        assert_eq!(line((0, 0), (3, 0)), [(1, 0), (2, 0), (3, 0)]);
        assert_eq!(line((0, 0), (2, 2)), [(1, 1), (2, 2)]);
        assert_eq!(line((2, 2), (2, 2)), Vec::<(i32, i32)>::new());
        let l = line((0, 0), (5, 2));
        assert_eq!(l.last(), Some(&(5, 2)));
        assert!(l.windows(2).all(|w| (w[0].0 - w[1].0).abs() <= 1 && (w[0].1 - w[1].1).abs() <= 1));
    }

    #[test]
    fn a_brush_stroke_is_one_undo_step() {
        let (mut d, p, mut t) = (doc(), Palette::fallback(), ToolState::default());
        t.set_tool(Tool::Terrain { code: 4, shape: TerrainShape::Brush(1) });
        t.press(&mut d, &p, (0, 5));
        t.drag_to(&mut d, (6, 5));
        t.release(&mut d, (6, 5));
        assert_eq!(d.scenario.terrain.iter().filter(|c| **c == 4).count(), 7);
        assert!(d.undo());
        assert!(!d.can_undo());
    }

    #[test]
    fn rectangle_and_fill() {
        let (mut d, p, mut t) = (doc(), Palette::fallback(), ToolState::default());
        t.set_tool(Tool::Terrain { code: 1, shape: TerrainShape::Rect });
        t.press(&mut d, &p, (2, 2));
        assert_eq!(t.rect_start(), Some((2, 2)));
        t.drag_to(&mut d, (4, 3));
        t.release(&mut d, (4, 3));
        assert_eq!(d.scenario.terrain.iter().filter(|c| **c == 1).count(), 6);
        t.set_tool(Tool::Terrain { code: 2, shape: TerrainShape::Fill });
        t.press(&mut d, &p, (0, 0));
        t.release(&mut d, (0, 0));
        assert_eq!(d.scenario.terrain.iter().filter(|c| **c == 2).count(), 394);
    }

    #[test]
    fn placing_selects_and_select_drags() {
        let (mut d, p, mut t) = (doc(), Palette::fallback(), ToolState::default());
        t.set_tool(Tool::Building { kind: 3, picture_type: 3, variant: 0 });
        t.press(&mut d, &p, (10, 10));
        assert_eq!(t.selected, Some(Target::Building(1)));
        assert_eq!((d.scenario.buildings[0].size_x, d.scenario.buildings[0].size_y), crate::editor::palette::fallback_size(3));
        // Too close to the edge: refused with a message.
        t.press(&mut d, &p, (0, 0));
        assert_eq!(d.scenario.buildings.len(), 1);
        assert!(t.message.as_deref().is_some_and(|m| m.contains("outside")));
        t.set_tool(Tool::Select);
        // Grabbed one cell up-left of its anchor: the anchor keeps that offset.
        t.press(&mut d, &p, (9, 9));
        t.drag_to(&mut d, (12, 11));
        t.drag_to(&mut d, (13, 14));
        t.release(&mut d, (13, 14));
        assert_eq!((d.scenario.buildings[0].x, d.scenario.buildings[0].y), (14, 15));
        d.undo();
        assert_eq!((d.scenario.buildings[0].x, d.scenario.buildings[0].y), (10, 10));
        // Clicking empty ground deselects.
        t.press(&mut d, &p, (1, 1));
        assert_eq!(t.selected, None);
    }

    #[test]
    fn armies_points_hero_start_and_delete() {
        let (mut d, p, mut t) = (doc(), Palette::fallback(), ToolState::default());
        t.set_tool(Tool::Army { model: 4 });
        t.press(&mut d, &p, (3, 3));
        t.set_tool(Tool::Point { model: 8 });
        t.press(&mut d, &p, (4, 4));
        assert_eq!(t.selected, Some(Target::Point(1)));
        t.set_tool(Tool::HeroStart(1));
        t.press(&mut d, &p, (7, 8));
        assert_eq!((d.scenario.header.heroes[1].x, d.scenario.header.heroes[1].y), (7, 8));
        assert_eq!(t.tool, Tool::Select);
        t.press(&mut d, &p, (3, 3));
        t.release(&mut d, (3, 3));
        assert_eq!(t.selected, Some(Target::Army(1)));
        t.delete_selected(&mut d);
        assert!(d.scenario.armies.is_empty() && t.selected.is_none());
        t.selected = Some(Target::Point(1));
        d.undo();
        d.undo();
        d.undo();
        t.check_selection(&d);
        assert_eq!(t.selected, None, "the point is gone after undoing its placement");
    }

    #[test]
    fn objects_and_erase_tools() {
        let (mut d, p, mut t) = (doc(), Palette::fallback(), ToolState::default());
        t.set_tool(Tool::Objects { class: 9, sprite: 1, size: 3 });
        t.press(&mut d, &p, (5, 5));
        t.drag_to(&mut d, (6, 5));
        t.release(&mut d, (6, 5));
        assert_eq!(d.scenario.objects.len(), 12);
        t.set_tool(Tool::Erase { size: 1, filter: ObjectFilter::All });
        t.press(&mut d, &p, (5, 5));
        t.release(&mut d, (5, 5));
        assert_eq!(d.scenario.objects.len(), 11);
        d.undo();
        d.undo();
        assert!(d.scenario.objects.is_empty());
    }

    #[test]
    fn presses_outside_the_map_do_nothing() {
        let (mut d, p, mut t) = (doc(), Palette::fallback(), ToolState::default());
        t.set_tool(Tool::Army { model: 4 });
        t.press(&mut d, &p, (-1, 3));
        t.press(&mut d, &p, (3, 20));
        assert!(d.scenario.armies.is_empty() && !d.can_undo());
    }
}
