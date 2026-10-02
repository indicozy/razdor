//! What the original editor's map tools do to the map (docs/reference/editor/main-window.md
//! §10–§12, §15): the brush of each page (0x595390), the delete brush (0x597588), picking up
//! and dropping (0x5ad0c4) and "burn everything" (0x5b29d4). Each works on the scenario and
//! the [`CellLayer`]; [`super::EditorDoc`] runs them as commands.
//!
//! Every tool works on the square of side `size` that ends at the brush centre (its
//! bottom-right cell, [`CellRect::brush`]). Objects follow the original's cell grid: one hill,
//! mountain or rocks and one plant per cell ([`ObjectGrid`]).

use crate::dt::dtm::{Building, MapObject, Scenario};
use crate::rules::rng::Rng;

use super::cells::{army_word, figure_index, figure_kind, point_word, CellLayer, Reveal, HERO_REVEAL};
use super::defaults::{new_army, new_point, LANTERN};
use super::geometry::CellRect;
use super::grid::{object_slot, ObjectGrid, MARK_BUILDING};
use super::naming::{building_names, NamePools};
use super::palette::ForestFacts;
use super::refs;

/// The original places a building only while fewer than this many exist.
pub const MAX_BUILDINGS: usize = 254;
/// Armies the original places at most.
pub const MAX_ARMIES: usize = 255;
/// Points the original places at most.
pub const MAX_POINTS: usize = 256;

/// The page whose delete brush runs (the tool pages of §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Terrain,
    Hills,
    Forests,
    Buildings,
    Items,
}

/// An object being moved (§12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    Building(u16),
    Army(u8),
    Point(u16),
}

/// Why a placement did nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The record list is full.
    Full,
    /// A building is already anchored on the cell.
    Occupied,
    /// The brush square reaches outside the map.
    Outside,
}

fn inside(s: &Scenario, x: i64, y: i64) -> bool {
    x >= 0 && y >= 0 && x < s.width() as i64 && y < s.height() as i64
}

/// The brush square, if all of it is inside the map (the hills, forests and buildings pages
/// act only then).
fn square_inside(s: &Scenario, cx: i64, cy: i64, size: u32) -> Option<CellRect> {
    let r = CellRect::brush(cx as i32, cy as i32, size);
    r.inside(s.width(), s.height()).then_some(r)
}

/// The objects as a grid while a tool works; written back only when a slot changed.
struct Objects {
    grid: ObjectGrid,
    changed: bool,
}

impl Objects {
    fn of(s: &Scenario) -> Objects {
        Objects { grid: ObjectGrid::from_objects(s.width(), s.height(), &s.objects), changed: false }
    }

    fn at(&self, x: i64, y: i64, slot: usize) -> Option<MapObject> {
        self.grid.at(x, y)[slot]
    }

    fn set(&mut self, x: i64, y: i64, slot: usize, o: Option<MapObject>) {
        if self.grid.at(x, y)[slot] != o {
            self.grid.set(x, y, slot, o);
            self.changed = true;
        }
    }

    fn store(self, s: &mut Scenario) {
        if self.changed {
            s.objects = self.grid.objects();
        }
    }
}

fn object(x: i64, y: i64, class: u8, sprite: u8) -> MapObject {
    MapObject { x: x as u16, y: y as u16, class, sprite }
}

/// One cell of the terrain brush: the code; water and lava (codes below 4) take the plant
/// away, deep sea (2) the hill too; the mark becomes the code unless it holds an object or
/// building mark.
fn paint_cell(s: &mut Scenario, objects: &mut Objects, cells: &mut CellLayer, x: i64, y: i64, code: u8) {
    let w = s.width() as usize;
    s.terrain[y as usize * w + x as usize] = code;
    if code < 4 {
        objects.set(x, y, 1, None);
    }
    if code == 2 {
        objects.set(x, y, 0, None);
    }
    if cells.mark(x, y) >= 0 {
        cells.set_mark(x, y, code as i8);
    }
}

/// The terrain brush (page 0) on every map cell of the square ending at `(cx, cy)`.
pub fn paint_terrain(s: &mut Scenario, cells: &mut CellLayer, cx: i64, cy: i64, size: u32, code: u8) {
    let r = CellRect::brush(cx as i32, cy as i32, size);
    paint_cells(s, cells, r.cells().map(|(x, y)| (x as i64, y as i64)), code);
}

/// The terrain brush's cell rule on any cells (Razdor's fill and rectangle use it too).
pub fn paint_cells(s: &mut Scenario, cells: &mut CellLayer, at: impl Iterator<Item = (i64, i64)>, code: u8) {
    let code = code.min(15);
    let mut objects = Objects::of(s);
    for (x, y) in at {
        if inside(s, x, y) {
            paint_cell(s, &mut objects, cells, x, y, code);
        }
    }
    objects.store(s);
}

/// Whether tree replacement may re-roll a plant of class `old` with one of class `new`: live
/// and dead trees count as one family, thickets and bushes each only with themselves.
fn same_family(old: u8, new: u8) -> bool {
    match old {
        9 | 10 => matches!(new, 9 | 10),
        11 | 12 => new == old,
        _ => false,
    }
}

/// The hills and forests brush (pages 1 and 2) with object `(class, sprite)`; nothing unless
/// the whole square is inside the map.
///
/// - Classes 1–8: one object, on the centre cell.
/// - Plants at size 1: the object on the centre cell (tree replacement does not apply).
/// - Plants at larger sizes, cell by cell from the bottom-right, right to left, then the row
///   above: a member of the sprite's family of twelve, `Random(family size)`, and when that
///   sprite has a "+120" alternate a further `Random(5)` = 0 takes the alternate. With tree
///   replacement on, only cells already holding a plant of the same family are re-rolled.
///
/// Every cell of the square then gets the class's mark, also the cells tree replacement left
/// alone. Returns whether it acted.
#[allow(clippy::too_many_arguments)]
pub fn place_object(s: &mut Scenario, cells: &mut CellLayer, rng: &mut Rng, cx: i64, cy: i64, size: u32, class: u8, sprite: u8, facts: &ForestFacts, replace: bool) -> bool {
    let Some(square) = square_inside(s, cx, cy, size) else { return false };
    let mut objects = Objects::of(s);
    if (9..=12).contains(&class) {
        if size < 2 {
            objects.set(cx, cy, 1, Some(object(cx, cy, class, sprite)));
        } else {
            let family = sprite / 12;
            if family > 9 {
                // The original stops with a range error (a family above 9): nothing changes.
                return false;
            }
            for j in 0..size as i64 {
                for i in 0..size as i64 {
                    let (x, y) = (cx - i, cy - j);
                    let ok = !replace || objects.at(x, y, 1).is_some_and(|o| same_family(o.class, class));
                    if !ok {
                        continue;
                    }
                    let mut pick = rng.random(facts.count(class, family) as i32) as u8 + family * 12;
                    if facts.has_alternate(class, pick) && rng.random(5) == 0 {
                        pick += 120;
                    }
                    objects.set(x, y, 1, Some(object(x, y, class, pick)));
                }
            }
        }
    } else {
        objects.set(cx, cy, object_slot(class), Some(object(cx, cy, class, sprite)));
    }
    objects.store(s);
    for (x, y) in square.cells() {
        cells.set_mark(x as i64, y as i64, -(class as i8));
    }
    true
}

/// The record type of a new building of picture `(picture_type, variant)`: the picture type,
/// except houses 2–4, which are obelisks (15), and 5–6, ruins (12).
pub fn building_kind(picture_type: u8, variant: u8) -> u8 {
    match (picture_type, variant) {
        (8, 2..=4) => 15,
        (8, 5..=6) => 12,
        (t, _) => t,
    }
}

/// The garrison defence a new building of picture type `t` starts with: town 20, village 2,
/// castle 15, fort 10, ruins 5 (the picture type, so houses made ruins get none).
pub fn building_defence(t: u8) -> u8 {
    match t {
        1 => 20,
        2 => 2,
        3 => 15,
        4 => 10,
        12 => 5,
        _ => 0,
    }
}

/// The buildings brush (page 3): a new building anchored at `(cx, cy)` (its bottom-right
/// cell) with picture `(picture_type, variant)`, footprint `size` and brush `brush` (the
/// picture's width in cells). It needs fewer than 254 buildings, no building anchored on the
/// cell and the brush square inside the map; nothing else (overlaps, water, figures) is
/// checked. The record is zeroed, then: the type ([`building_kind`]), names
/// ([`building_names`], which reseeds `rng`), faction 3 with the scenario's attitude row of
/// faction 3, the defence ([`building_defence`]), owner 0. The footprint's cells lose their
/// plants and get the building mark, none of them when the footprint reaches past the map's
/// left or top edge. Returns the new id.
#[allow(clippy::too_many_arguments)]
pub fn place_building(
    s: &mut Scenario,
    cells: &mut CellLayer,
    rng: &mut Rng,
    cx: i64,
    cy: i64,
    picture: (u8, u8),
    size: (u8, u8),
    brush: u32,
    pools: Option<&NamePools>,
) -> Result<u16, Refused> {
    let mut objects = Objects::of(s);
    let placed = place_building_in(s, &mut objects.grid, cells, rng, BuildingAt { x: cx, y: cy, picture, size, brush }, pools, false);
    if let Ok((_, cleared)) = placed {
        objects.changed = cleared;
        objects.store(s);
    }
    placed.map(|p| p.0)
}

/// Where and what [`place_building_in`] places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingAt {
    pub x: i64,
    pub y: i64,
    pub picture: (u8, u8),
    pub size: (u8, u8),
    pub brush: u32,
}

/// [`place_building`] on an object grid the caller keeps (the world generator places
/// hundreds), with the original's "generating" flag (0x5bee90): while it is on, the footprint
/// cells of a building of type below 13 take the temporary terrain 16 and keep their plants;
/// while it is off they lose their plants. Returns the id and whether a plant was cleared.
pub fn place_building_in(
    s: &mut Scenario,
    objects: &mut ObjectGrid,
    cells: &mut CellLayer,
    rng: &mut Rng,
    at: BuildingAt,
    pools: Option<&NamePools>,
    generating: bool,
) -> Result<(u16, bool), Refused> {
    let BuildingAt { x: cx, y: cy, picture, size, brush } = at;
    if s.buildings.len() >= MAX_BUILDINGS {
        return Err(Refused::Full);
    }
    if s.buildings.iter().any(|b| (b.x as i64, b.y as i64) == (cx, cy)) {
        return Err(Refused::Occupied);
    }
    if square_inside(s, cx, cy, brush).is_none() {
        return Err(Refused::Outside);
    }
    let (picture_type, variant) = picture;
    let (x, y) = (cx as u16, cy as u16);
    // Without the install's lists the draws still run (so the generator goes on as the
    // original's would with empty lists) and the names stay empty.
    let none = NamePools::default();
    let (mut name, mut owner_name) = building_names(pools.unwrap_or(&none), rng, x, y, picture_type, variant);
    if pools.is_none() {
        (name, owner_name) = (String::new(), String::new());
    }
    let kind = building_kind(picture_type, variant);
    let b = Building {
        x,
        y,
        picture_type,
        picture_variant: variant,
        kind,
        size_x: size.0,
        size_y: size.1,
        owner_army: 0,
        faction: 3,
        relations: s.header.relations[2],
        garrison_extra_defence: building_defence(picture_type),
        name,
        owner_name,
        ..Building::default()
    };
    s.buildings.push(b);
    let mut cleared = false;
    let w = s.width() as usize;
    let (x0, y0) = (cx - size.0 as i64 + 1, cy - size.1 as i64 + 1);
    // A footprint reaching left of or above the map stops the original with a range error
    // after the record is made, at its first (top-left) cell: no footprint cell is written.
    if x0 < 0 || y0 < 0 {
        return Ok((s.buildings.len() as u16, false));
    }
    for fx in x0..=cx {
        for fy in y0..=cy {
            if !inside(s, fx, fy) {
                continue;
            }
            if !generating {
                cleared |= objects.at(fx, fy)[1].is_some();
                objects.set(fx, fy, 1, None);
            } else if picture_type < 13 {
                // The picture type, not the record's (0x596c10): a house made an obelisk
                // (type 15) still stands on terrain 16.
                s.terrain[fy as usize * w + fx as usize] = TEMPORARY_TERRAIN;
            }
            cells.set_mark(fx, fy, MARK_BUILDING);
        }
    }
    Ok((s.buildings.len() as u16, cleared))
}

/// The terrain the generator's buildings stand on until its step ends (worldgen.md §3.5).
pub const TEMPORARY_TERRAIN: u8 = 16;

/// What an item placement made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placed {
    /// Hero preset `k` (0 knight, 1 archmage, 2 ranger) starts here now.
    HeroStart(usize),
    Army(u8),
    /// A point of model 8 (lantern), 9 (event point) or 10 (target place).
    Point(u16),
}

/// The items brush (page 4) with kind `kind` at `(cx, cy)`:
/// - 1–3, a hero start: the preset's old cell loses its figure and its radius-5 reveal
///   (even a preset never placed, at 0,0, so a figure there is wiped and the fog around 0,0
///   wraps), the preset moves here, reveals radius 5 and writes its figure;
/// - 4–7, an army (at most 255): [`new_army`], its figure written;
/// - 8–10, a point (at most 256): [`new_point`], its figure written; the figure word is
///   `kind·256 + number`, so the 256th point's word carries into the kind byte.
///
/// No cell is checked: a figure overwrites the one there.
pub fn place_item(s: &mut Scenario, cells: &mut CellLayer, cx: i64, cy: i64, kind: u8) -> Result<Placed, Refused> {
    let (x, y) = (cx as u16, cy as u16);
    match kind {
        1..=3 => {
            let k = kind as usize - 1;
            let h = &mut s.header.heroes[k];
            let (ox, oy) = (h.x as i64, h.y as i64);
            cells.set_figure(ox, oy, 0);
            cells.reveal(ox, oy, HERO_REVEAL, Reveal::Remove);
            (h.x, h.y) = (x, y);
            cells.reveal(cx, cy, HERO_REVEAL, Reveal::Add);
            cells.set_figure(cx, cy, (kind as u16) << 8);
            Ok(Placed::HeroStart(k))
        }
        4..=7 => {
            if s.armies.len() >= MAX_ARMIES {
                return Err(Refused::Full);
            }
            let n = s.armies.len() as u8 + 1;
            let a = new_army(n, x, y, kind);
            cells.set_figure(cx, cy, army_word(&a));
            s.armies.push(a);
            Ok(Placed::Army(n))
        }
        8..=10 => {
            if s.points.len() >= MAX_POINTS {
                return Err(Refused::Full);
            }
            let n = s.points.len() as u16 + 1;
            let p = new_point(n, x, y, kind);
            cells.set_figure(cx, cy, point_word(&p));
            s.points.push(p);
            Ok(Placed::Point(n))
        }
        _ => Err(Refused::Outside),
    }
}

/// The building anchored on a cell: the last record anchored there (the original keeps one
/// anchor word per cell and finds its record by searching all of them).
fn anchored_at(s: &Scenario, x: i64, y: i64, skip: Option<u16>) -> Option<usize> {
    s.buildings.iter().enumerate().rposition(|(k, b)| skip != Some(k as u16 + 1) && (b.x as i64, b.y as i64) == (x, y))
}

/// Whether a building anchored at `(ax, ay)` of footprint `size` covers `(x, y)`, the anchor
/// being right of and below it.
fn covers(ax: i64, ay: i64, size: (u8, u8), x: i64, y: i64) -> bool {
    ay - (size.1 as i64) < y && ax - (size.0 as i64) < x
}

/// The cells searched for anchors of what covers `(x, y)`: up to 8 right and down, row by
/// row.
fn window(s: &Scenario, x: i64, y: i64) -> impl Iterator<Item = (i64, i64)> {
    let (x1, y1) = ((x + 8).min(s.width() as i64 - 1), (y + 8).min(s.height() as i64 - 1));
    (y..=y1).flat_map(move |cy| (x..=x1).map(move |cx| (cx, cy)))
}

/// The building covering a cell as the original finds it (0x54af70): the first anchor of
/// the search [`window`] whose footprint covers the cell. `skip` is a building being moved.
pub fn building_at(s: &Scenario, x: i64, y: i64, skip: Option<u16>) -> Option<u16> {
    window(s, x, y).find_map(|(ax, ay)| {
        let k = anchored_at(s, ax, ay, skip)?;
        let b = &s.buildings[k];
        covers(ax, ay, (b.size_x, b.size_y), x, y).then_some(k as u16 + 1)
    })
}

/// The army standing on a cell: the last one there (0x5acf94).
pub fn army_at(s: &Scenario, x: i64, y: i64) -> Option<u8> {
    s.armies.iter().rposition(|a| (a.x as i64, a.y as i64) == (x, y)).map(|k| k as u8 + 1)
}

/// What a delete changed, for the buildings and armies lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Deleted {
    pub buildings: bool,
    pub armies: bool,
}

/// The delete brush at `(x, y)` on page `page` (0x597588):
/// - hills: every hill whose footprint (sprite div 10 cells, ending at it) covers the cell,
///   searched up to 8 cells right and down; the marks are then built again. A hill of
///   sprite below 10 covers nothing and cannot be deleted (the original's test, kept);
/// - forests: the cell's plant; its mark becomes the terrain;
/// - buildings: every building of the search window whose footprint covers the cell, with
///   [`refs::remove_building`]; their footprints' marks become the terrain;
/// - items, by the cell's figure word: a lantern or event point is removed
///   ([`refs::remove_point`] by the word's index; the later points' words are written again
///   at their cells); a hero start's preset goes to 0,0; anything else is taken for an army
///   of the word's index ([`refs::remove_army`]), so deleting a target place deletes an army
///   (the original's bug, kept). The figure word is cleared; no reveal is undone.
pub fn delete_at(s: &mut Scenario, cells: &mut CellLayer, x: i64, y: i64, page: Page) -> Deleted {
    let mut out = Deleted::default();
    if !inside(s, x, y) {
        return out;
    }
    match page {
        Page::Terrain => {}
        Page::Hills => {
            let mut objects = Objects::of(s);
            for (ox, oy) in window(s, x, y).collect::<Vec<_>>() {
                if let Some(o) = objects.at(ox, oy, 0) {
                    let n = (o.sprite / 10) as i64;
                    if oy - n < y && ox - n < x {
                        objects.set(ox, oy, 0, None);
                    }
                }
            }
            let changed = objects.changed;
            objects.store(s);
            if changed {
                cells.rebuild_marks(s);
            }
        }
        Page::Forests => {
            let mut objects = Objects::of(s);
            if objects.at(x, y, 1).is_some() {
                objects.set(x, y, 1, None);
                let code = super::grid::terrain_or_zero(s, x, y);
                cells.set_mark(x, y, code as i8);
            }
            objects.store(s);
        }
        Page::Buildings => {
            for (ax, ay) in window(s, x, y).collect::<Vec<_>>() {
                let Some(k) = anchored_at(s, ax, ay, None) else { continue };
                let size = (s.buildings[k].size_x, s.buildings[k].size_y);
                if !covers(ax, ay, size, x, y) {
                    continue;
                }
                refs::remove_building(s, k as u16 + 1);
                out.buildings = true;
                for fy in (ay - size.1 as i64 + 1)..=ay {
                    for fx in (ax - size.0 as i64 + 1)..=ax {
                        if inside(s, fx, fy) {
                            let code = super::grid::terrain_or_zero(s, fx, fy);
                            cells.set_mark(fx, fy, code as i8);
                        }
                    }
                }
            }
        }
        Page::Items => {
            let word = cells.figure(x, y);
            if word == 0 {
                return out;
            }
            let (kind, index) = (figure_kind(word), figure_index(word));
            if matches!(kind, 8 | 9) {
                if refs::remove_point(s, index as u16) {
                    for p in s.points.iter().skip((index as usize).saturating_sub(1)) {
                        cells.set_figure(p.x as i64, p.y as i64, point_word(p));
                    }
                }
            } else if index == 0 {
                // A hero start. A word of another kind with index 0 (the 256th target place)
                // stops the original with a range error; nothing changes then.
                if let Some(h) = (kind as usize).checked_sub(1).and_then(|k| s.header.heroes.get_mut(k)) {
                    (h.x, h.y) = (0, 0);
                }
            } else if refs::remove_army(s, index) {
                out.armies = true;
                for a in s.armies.iter().skip(index as usize - 1) {
                    cells.set_figure(a.x as i64, a.y as i64, army_word(a));
                }
            }
            cells.set_figure(x, y, 0);
        }
    }
    out
}

/// Picks up what stands on a cell (§12): the army there, else the building covering it, else
/// the lantern or event point of the cell's figure word; hero starts cannot be picked up. An
/// army's or point's figure word is cleared (a building's anchor leaves the map while it is
/// held); nothing else changes until the drop.
pub fn pick_up(s: &Scenario, cells: &mut CellLayer, x: i64, y: i64) -> Option<Held> {
    if let Some(id) = army_at(s, x, y) {
        cells.set_figure(x, y, 0);
        return Some(Held::Army(id));
    }
    if let Some(id) = building_at(s, x, y, None) {
        return Some(Held::Building(id));
    }
    let word = cells.figure(x, y);
    let index = figure_index(word) as u16;
    // The 256th point's word has index 0, which the original's pick-up reads as a record
    // before the first and stops with a range error: it is not picked up here.
    if matches!(figure_kind(word), 8 | 9) && index >= 1 && (index as usize) <= s.points.len() {
        cells.set_figure(x, y, 0);
        return Some(Held::Point(index));
    }
    None
}

/// Drops a held object with its anchor at `(cx, cy)` (§12): no check at all (any terrain,
/// other buildings and figures, a footprint over the map's edge). A building's marks are
/// built again; an army or point writes its figure word; a lantern takes its reveal along
/// (removed at the old cell, added at the new one, lit or not).
pub fn drop(s: &mut Scenario, cells: &mut CellLayer, held: Held, cx: i64, cy: i64) {
    let (x, y) = (cx.clamp(0, u16::MAX as i64) as u16, cy.clamp(0, u16::MAX as i64) as u16);
    match held {
        Held::Building(id) => {
            if let Some(b) = (id as usize).checked_sub(1).and_then(|k| s.buildings.get_mut(k)) {
                (b.x, b.y) = (x, y);
                cells.rebuild_marks(s);
            }
        }
        Held::Army(id) => {
            if let Some(a) = (id as usize).checked_sub(1).and_then(|k| s.armies.get_mut(k)) {
                (a.x, a.y) = (x, y);
                cells.set_figure(cx, cy, army_word(a));
            }
        }
        Held::Point(id) => {
            if let Some(p) = (id as usize).checked_sub(1).and_then(|k| s.points.get_mut(k)) {
                let lantern = p.model == LANTERN;
                if lantern {
                    cells.reveal(p.x as i64, p.y as i64, p.radius as u16, Reveal::Remove);
                }
                (p.x, p.y) = (x, y);
                cells.set_figure(cx, cy, point_word(p));
                if lantern {
                    cells.reveal(cx, cy, p.radius as u16, Reveal::Add);
                }
            }
        }
    }
}

/// A lantern's radius from the number dialog: a new lantern reveals its area with it; an
/// existing one's old area is removed first.
pub fn lantern_radius(s: &mut Scenario, cells: &mut CellLayer, id: u16, radius: u8, placed: bool) -> bool {
    let Some(p) = (id as usize).checked_sub(1).and_then(|k| s.points.get_mut(k)) else { return false };
    if !placed {
        cells.reveal(p.x as i64, p.y as i64, p.radius as u16, Reveal::Remove);
    }
    p.radius = radius;
    cells.reveal(p.x as i64, p.y as i64, radius as u16, Reveal::Add);
    true
}

/// The ruin picture "burn everything" gives a building picture: town variant 3 and every
/// village one ruin each, castles 0–5 six ruins, forts 1–5 one of two at random, taverns and
/// house 0 one, house 1 another, churches 0–1 one and 4 another; the rest keep theirs.
fn burnt_picture(rng: &mut Rng, picture_type: u8, variant: u8) -> (u8, u8) {
    match (picture_type, variant) {
        (1, 3) => (12, 13),
        (2, _) => (12, 14),
        (3, v @ 0..=5) => (12, v),
        (4, 1..=5) => (12, 7 + rng.random(2) as u8),
        (5, _) => (12, 12),
        (7, 0..=1) => (12, 9),
        (7, 4) => (12, 8),
        (8, 0) => (12, 12),
        (8, 1) => (12, 11),
        other => other,
    }
}

/// "Burn everything" (0x5b29d4):
/// - every building gets its ruin picture ([`burnt_picture`]) but keeps its type, and loses
///   its description;
/// - then, row by row, every cell: terrain above 3 becomes scorched land (13); green, steppe
///   and sand hills (classes 1, 2, 4) become scorched hills (3) and green mountains (5)
///   scorched ones (6), each keeping its sprite; live trees become a random dead tree of the
///   last family (sprite 108 + `Random(size)`), thickets are re-rolled the same way, either
///   then taking its "+120" alternate on `Random(5)` = 0 when there is one.
///
/// The original also replaces nine nouns in the building names by their ruined forms; those
/// words are the original's own text and are not reproduced. It asks nothing, does not mark
/// the map modified and leaves the marks as they were.
pub fn burn(s: &mut Scenario, rng: &mut Rng, facts: &ForestFacts) {
    for b in &mut s.buildings {
        (b.picture_type, b.picture_variant) = burnt_picture(rng, b.picture_type, b.picture_variant);
        b.description.clear();
    }
    let (w, h) = (s.width() as i64, s.height() as i64);
    let mut objects = Objects::of(s);
    for y in 0..h {
        for x in 0..w {
            let t = &mut s.terrain[(y * w + x) as usize];
            if *t > 3 {
                *t = 13;
            }
            if let Some(o) = objects.at(x, y, 0) {
                let class = match o.class {
                    1 | 2 | 4 => 3,
                    5 => 6,
                    c => c,
                };
                objects.set(x, y, 0, Some(MapObject { class, ..o }));
            }
            if let Some(o) = objects.at(x, y, 1) {
                let class = match o.class {
                    9 => 10,
                    11 => 11,
                    _ => continue,
                };
                let mut sprite = 108 + rng.random(facts.count(class, 9) as i32) as u8;
                if facts.has_alternate(class, sprite) && rng.random(5) == 0 {
                    sprite += 120;
                }
                objects.set(x, y, 1, Some(MapObject { class, sprite, ..o }));
            }
        }
    }
    objects.store(s);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::HeroPreset;
    use crate::editor::grid::Marks;
    use crate::editor::palette::{ObjectKey, Palette};
    use crate::editor::{EditorDoc, NewMap};

    fn scenario(w: u32) -> Scenario {
        EditorDoc::new_map(NewMap { width: w, height: w, fill: 6 }).scenario
    }

    fn facts() -> ForestFacts {
        let mut objects: Vec<ObjectKey> = (0..120).map(|s| ObjectKey { class: 9, sprite: s }).collect();
        objects.extend([121, 125].map(|s| ObjectKey { class: 9, sprite: s }));
        objects.extend((108..112).map(|s| ObjectKey { class: 10, sprite: s }));
        objects.push(ObjectKey { class: 10, sprite: 229 });
        objects.extend((108..110).map(|s| ObjectKey { class: 11, sprite: s }));
        objects.extend((0..3).map(|s| ObjectKey { class: 11, sprite: s }));
        objects.sort();
        Palette { objects, buildings: vec![], from_install: true }.forest_facts()
    }

    fn objects_at(s: &Scenario, x: u16, y: u16) -> Vec<(u8, u8)> {
        s.objects.iter().filter(|o| (o.x, o.y) == (x, y)).map(|o| (o.class, o.sprite)).collect()
    }

    #[test]
    fn terrain_clears_plants_and_hills() {
        let mut s = scenario(10);
        s.objects = vec![object(4, 4, 1, 20), object(4, 4, 9, 3), object(5, 4, 9, 3), object(6, 4, 1, 10)];
        let mut c = CellLayer::load(&s);
        // Coast (1) takes plants, deep sea (2) hills too, road (4) nothing.
        paint_terrain(&mut s, &mut c, 5, 4, 1, 1);
        paint_terrain(&mut s, &mut c, 4, 4, 1, 4);
        assert_eq!((objects_at(&s, 4, 4), objects_at(&s, 5, 4)), (vec![(1, 20), (9, 3)], vec![]));
        paint_terrain(&mut s, &mut c, 4, 4, 1, 2);
        paint_terrain(&mut s, &mut c, 6, 4, 1, 3);
        assert_eq!((objects_at(&s, 4, 4), objects_at(&s, 6, 4)), (vec![], vec![(1, 10)]));
        // Object marks stay where they were written (the hill's −1, the cleared plant's −9),
        // a free cell takes the code.
        paint_terrain(&mut s, &mut c, 8, 8, 1, 4);
        assert_eq!((c.mark(3, 3), c.mark(5, 4), c.mark(8, 8), s.terrain[4 * 10 + 4]), (-1, -9, 4, 2));
        // A size 2 brush at centre (9, 9) paints only the cells inside the map.
        paint_terrain(&mut s, &mut c, 10, 10, 2, 0);
        assert_eq!(s.terrain[99], 0);
        assert_eq!(s.terrain.iter().filter(|t| **t == 0).count(), 1);
    }

    #[test]
    fn hills_are_one_object_and_need_the_whole_square() {
        let mut s = scenario(10);
        let mut c = CellLayer::load(&s);
        let mut rng = Rng::new(1);
        let f = ForestFacts::default();
        assert!(!place_object(&mut s, &mut c, &mut rng, 1, 5, 3, 1, 30, &f, false), "the square would reach x = -1");
        assert!(place_object(&mut s, &mut c, &mut rng, 4, 5, 3, 1, 30, &f, false));
        assert_eq!(s.objects, vec![object(4, 5, 1, 30)]);
        assert_eq!((c.mark(2, 3), c.mark(4, 5), c.mark(5, 5)), (-1, -1, 6));
        // A second hill on the same cell replaces the first.
        place_object(&mut s, &mut c, &mut rng, 4, 5, 2, 5, 20, &f, false);
        assert_eq!(s.objects, vec![object(4, 5, 5, 20)]);
        assert_eq!(rng.state(), 1, "hills draw nothing");
    }

    #[test]
    fn forests_draw_row_by_row_from_the_bottom_right() {
        let f = facts();
        let mut s = scenario(10);
        let mut c = CellLayer::load(&s);
        let mut rng = Rng::new(42);
        // Family 0 of class 9: 12 sprites; sprite 1 and 5 have alternates.
        assert!(place_object(&mut s, &mut c, &mut rng, 5, 5, 2, 9, 0, &f, false));
        let mut e = Rng::new(42);
        let mut want = Vec::new();
        for (x, y) in [(5, 5), (4, 5), (5, 4), (4, 4)] {
            let mut sprite = e.random(12) as u8;
            if matches!(sprite, 1 | 5) && e.random(5) == 0 {
                sprite += 120;
            }
            want.push(object(x, y, 9, sprite));
        }
        want.sort_by_key(|o| (o.y, o.x));
        assert_eq!(s.objects, want);
        assert_eq!(rng.state(), e.state());
        assert!((4..=5).all(|x| (4..=5).all(|y| c.mark(x, y) == -9)));
        // Size 1 puts the chosen sprite and draws nothing.
        let before = rng.state();
        place_object(&mut s, &mut c, &mut rng, 1, 1, 1, 9, 7, &f, true);
        assert_eq!((objects_at(&s, 1, 1), rng.state()), (vec![(9, 7)], before));
    }

    #[test]
    fn tree_replacement_rerolls_only_the_same_family() {
        let f = facts();
        let mut s = scenario(10);
        // Over a 3×3 square ending at (5, 5): a dead tree, a thicket, a bush, empty cells.
        s.objects = vec![object(3, 3, 10, 108), object(4, 3, 11, 0), object(5, 3, 12, 0)];
        let mut c = CellLayer::load(&s);
        let mut rng = Rng::new(9);
        place_object(&mut s, &mut c, &mut rng, 5, 5, 3, 9, 12, &f, true);
        // Only the dead tree was re-rolled (into family 1 of live trees: sprites 12–23).
        let tree = objects_at(&s, 3, 3);
        assert!(tree.len() == 1 && tree[0].0 == 9 && (12..24).contains(&tree[0].1));
        assert_eq!((objects_at(&s, 4, 3), objects_at(&s, 5, 3), objects_at(&s, 5, 5)), (vec![(11, 0)], vec![(12, 0)], vec![]));
        // Yet every cell of the square has the trees' mark.
        assert!((3..=5).all(|x| (3..=5).all(|y| c.mark(x, y) == -9)));
        assert_ne!(Marks::build(&s).at(5, 5), -9, "a full rebuild finds no tree there");
    }

    #[test]
    fn buildings_get_the_original_defaults() {
        let pools = crate::editor::naming::tests::pools();
        let mut s = scenario(20);
        s.header.relations[2] = [1, 2, 3, -1];
        s.objects = vec![object(9, 9, 9, 1), object(5, 5, 9, 1)];
        let mut c = CellLayer::load(&s);
        let mut rng = Rng::new(0);
        let id = place_building(&mut s, &mut c, &mut rng, 10, 10, (3, 2), (4, 3), 4, Some(&pools)).unwrap();
        let b = &s.buildings[id as usize - 1];
        assert_eq!((b.kind, b.faction, b.relations, b.garrison_extra_defence, b.owner_army, b.size_x, b.size_y), (3, 3, [1, 2, 3, -1], 15, 0, 4, 3));
        assert_eq!((b.name.clone(), b.owner_name.clone()), building_names(&pools, &mut Rng::new(0), 10, 10, 3, 2));
        assert!(b.name.starts_with("C1 ") && b.description.is_empty() && b.event_count == 0 && b.has_barracks == 0);
        // The footprint loses its plant and takes the building mark.
        assert_eq!((objects_at(&s, 9, 9), objects_at(&s, 5, 5), c.mark(7, 8), c.mark(6, 8), c.mark(7, 7)), (vec![], vec![(9, 1)], MARK_BUILDING, 6, 6));
        // Only the anchor cell and the brush square are checked: an overlapping building is
        // fine, one on the same anchor or reaching out of the map is not.
        assert!(place_building(&mut s, &mut c, &mut rng, 9, 9, (1, 0), (7, 7), 7, None).is_ok());
        assert_eq!(place_building(&mut s, &mut c, &mut rng, 10, 10, (2, 0), (3, 3), 3, None), Err(Refused::Occupied));
        assert_eq!(place_building(&mut s, &mut c, &mut rng, 1, 15, (2, 0), (3, 3), 3, None), Err(Refused::Outside));
        // House pictures 2–4 are obelisks, 5–6 ruins without the ruins' defence.
        let id = place_building(&mut s, &mut c, &mut rng, 15, 15, (8, 5), (2, 2), 2, Some(&pools)).unwrap();
        let b = &s.buildings[id as usize - 1];
        assert_eq!((b.kind, b.garrison_extra_defence, building_kind(8, 3), building_kind(8, 1)), (12, 0, 15, 8));
        assert!(b.name.starts_with("n12.") && b.owner_name.is_empty());
        assert_eq!([1, 2, 3, 4, 5, 12].map(building_defence), [20, 2, 15, 10, 0, 5]);
        s.buildings.resize(MAX_BUILDINGS, Building::default());
        assert_eq!(place_building(&mut s, &mut c, &mut rng, 18, 18, (2, 0), (1, 1), 1, None), Err(Refused::Full));
    }

    #[test]
    fn while_generating_footprints_take_terrain_16_and_keep_their_plants() {
        let mut s = scenario(20);
        s.objects = vec![object(9, 9, 9, 1), object(4, 4, 9, 1)];
        let mut c = CellLayer::load(&s);
        let mut grid = ObjectGrid::from_objects(20, 20, &s.objects);
        let mut rng = Rng::new(0);
        let at = BuildingAt { x: 10, y: 10, picture: (3, 0), size: (2, 2), brush: 2 };
        assert_eq!(place_building_in(&mut s, &mut grid, &mut c, &mut rng, at, None, true), Ok((1, false)));
        assert_eq!((s.terrain[9 * 20 + 9], s.terrain[8 * 20 + 9], grid.at(9, 9)[1].is_some(), c.mark(9, 9)), (TEMPORARY_TERRAIN, 6, true, MARK_BUILDING));
        // Bridges (type 13 and up) keep their terrain; with the flag off plants go.
        let bridge = BuildingAt { x: 5, y: 5, picture: (13, 1), size: (1, 1), brush: 1 };
        place_building_in(&mut s, &mut grid, &mut c, &mut rng, bridge, None, true).unwrap();
        assert_eq!(s.terrain[5 * 20 + 5], 6);
        let fort = BuildingAt { x: 4, y: 4, picture: (4, 0), size: (1, 1), brush: 1 };
        assert_eq!(place_building_in(&mut s, &mut grid, &mut c, &mut rng, fort, None, false), Ok((3, true)));
        assert!(grid.at(4, 4)[1].is_none() && s.terrain[4 * 20 + 4] == 6);
        // The picture type decides, not the record's: a house made an obelisk (type 15)
        // stands on terrain 16 too.
        let obelisk = BuildingAt { x: 15, y: 15, picture: (8, 3), size: (1, 1), brush: 1 };
        place_building_in(&mut s, &mut grid, &mut c, &mut rng, obelisk, None, true).unwrap();
        assert_eq!((s.buildings[3].kind, s.terrain[15 * 20 + 15]), (15, TEMPORARY_TERRAIN));
    }

    #[test]
    fn a_footprint_past_the_left_or_top_edge_writes_none_of_its_cells() {
        // The original's range error comes at the footprint's first, top-left cell, after
        // the record and its names.
        let mut s = scenario(20);
        s.objects = vec![object(0, 5, 9, 1), object(1, 5, 9, 1)];
        let mut c = CellLayer::load(&s);
        let mut rng = Rng::new(0);
        let id = place_building(&mut s, &mut c, &mut rng, 1, 5, (3, 0), (3, 3), 1, None).unwrap();
        let mut e = Rng::new(0);
        building_names(&NamePools::default(), &mut e, 1, 5, 3, 0);
        assert_eq!((id, rng.state()), (1, e.state()));
        assert_eq!((objects_at(&s, 0, 5), objects_at(&s, 1, 5), c.mark(1, 5), c.mark(0, 4)), (vec![(9, 1)], vec![(9, 1)], -9, 6));
        let mut grid = ObjectGrid::from_objects(20, 20, &s.objects);
        let top = BuildingAt { x: 8, y: 0, picture: (3, 0), size: (2, 2), brush: 1 };
        place_building_in(&mut s, &mut grid, &mut c, &mut rng, top, None, true).unwrap();
        assert_eq!((s.terrain[8], c.mark(8, 0), s.buildings.len()), (6, 6, 2));
    }

    #[test]
    fn items_write_figures_and_reveal() {
        let mut s = scenario(20);
        s.header.heroes = [HeroPreset::default(), HeroPreset::default(), HeroPreset::default()];
        let mut c = CellLayer::load(&s);
        assert_eq!(place_item(&mut s, &mut c, 10, 10, 2), Ok(Placed::HeroStart(1)));
        assert_eq!(((s.header.heroes[1].x, s.header.heroes[1].y), c.figure(10, 10), c.revealed(10, 15), c.revealed(10, 16)), ((10, 10), 0x0200, 1, 0));
        // The unset preset's old cell 0,0 lost a reveal it never had: it wraps.
        assert_eq!(c.revealed(0, 0), 255);
        place_item(&mut s, &mut c, 12, 10, 2).unwrap();
        assert_eq!((c.figure(10, 10), c.figure(12, 10), c.revealed(5, 10), c.revealed(7, 10)), (0, 0x0200, 0, 1));
        // An army and a lantern overwrite the figure there.
        assert_eq!(place_item(&mut s, &mut c, 12, 10, 6), Ok(Placed::Army(1)));
        assert_eq!(c.figure(12, 10), 0x0601);
        let a = &s.armies[0];
        assert_eq!((a.model, a.behaviour, a.exp_correction, a.name.as_str()), (6, 2, 100, "Army 1"));
        assert_eq!(place_item(&mut s, &mut c, 3, 16, 8), Ok(Placed::Point(1)));
        assert_eq!((c.figure(3, 16), s.points[0].radius, s.points[0].active, c.revealed(3, 16)), (0x0801, 10, 1, 0), "the dialog reveals it");
        assert!(lantern_radius(&mut s, &mut c, 1, 4, true));
        assert_eq!((c.revealed(3, 12), c.revealed(3, 11)), (1, 0));
        assert!(lantern_radius(&mut s, &mut c, 1, 2, false));
        assert_eq!((c.revealed(3, 14), c.revealed(3, 13)), (1, 0));
        // The 256th point's word carries into the kind byte: an event point becomes 10.
        s.points.resize(255, s.points[0].clone());
        assert_eq!(place_item(&mut s, &mut c, 5, 5, 9), Ok(Placed::Point(256)));
        assert_eq!((c.figure(5, 5), s.points[255].model, s.points[255].id), (0x0A00, 10, 0));
        assert_eq!(place_item(&mut s, &mut c, 5, 6, 9), Err(Refused::Full));
    }

    #[test]
    fn pick_up_takes_army_then_building_then_point() {
        let mut s = scenario(20);
        let mut c = CellLayer::load(&s);
        let mut rng = Rng::new(0);
        place_building(&mut s, &mut c, &mut rng, 10, 10, (3, 0), (3, 3), 3, None).unwrap();
        place_item(&mut s, &mut c, 9, 9, 9).unwrap();
        place_item(&mut s, &mut c, 9, 9, 4).unwrap();
        // The army on top of the point inside the building's footprint comes first; a drop
        // checks nothing.
        assert_eq!(pick_up(&s, &mut c, 9, 9), Some(Held::Army(1)));
        assert_eq!(c.figure(9, 9), 0);
        drop(&mut s, &mut c, Held::Army(1), 0, 0);
        assert_eq!((s.armies[0].x, c.figure(0, 0)), (0, 0x0401));
        // The building is next.
        assert_eq!(pick_up(&s, &mut c, 9, 9), Some(Held::Building(1)));
        drop(&mut s, &mut c, Held::Building(1), 15, 15);
        assert_eq!((s.buildings[0].x, s.buildings[0].y, c.mark(14, 14), c.mark(10, 10)), (15, 15, MARK_BUILDING, 6));
        // The point's word was overwritten by the army's: nothing left to pick up.
        assert_eq!(pick_up(&s, &mut c, 9, 9), None);
        place_item(&mut s, &mut c, 4, 4, 8).unwrap();
        lantern_radius(&mut s, &mut c, 2, 3, true);
        assert_eq!(pick_up(&s, &mut c, 4, 4), Some(Held::Point(2)));
        assert_eq!(c.revealed(4, 4), 1, "a lantern keeps its area while held");
        // A drop checks nothing and moves the lantern's area.
        drop(&mut s, &mut c, Held::Point(2), 19, 19);
        assert_eq!((c.revealed(4, 4), c.revealed(19, 17), c.figure(19, 19)), (0, 1, 0x0802));
        // Hero starts cannot be picked up.
        place_item(&mut s, &mut c, 2, 2, 1).unwrap();
        assert_eq!(pick_up(&s, &mut c, 2, 2), None);
    }

    #[test]
    fn delete_follows_the_page_and_the_figure() {
        let mut s = scenario(30);
        let mut c = CellLayer::load(&s);
        let mut rng = Rng::new(0);
        // Hills: a 2-cell hill ending at (6, 6) covers (5, 5); a sprite-5 hill covers nothing.
        s.objects = vec![object(6, 6, 1, 25), object(5, 5, 1, 5), object(5, 5, 9, 1)];
        c.rebuild_marks(&s);
        delete_at(&mut s, &mut c, 5, 5, Page::Hills);
        assert_eq!(s.objects, vec![object(5, 5, 1, 5), object(5, 5, 9, 1)]);
        delete_at(&mut s, &mut c, 5, 5, Page::Forests);
        assert_eq!((s.objects.len(), c.mark(5, 5)), (1, 6));
        // Buildings: two overlapping ones covering (12, 12) both go; events renumber.
        place_building(&mut s, &mut c, &mut rng, 20, 20, (1, 0), (2, 2), 2, None).unwrap();
        place_building(&mut s, &mut c, &mut rng, 13, 13, (3, 0), (3, 3), 3, None).unwrap();
        place_building(&mut s, &mut c, &mut rng, 14, 14, (4, 0), (3, 3), 3, None).unwrap();
        let mut e = crate::dt::dtm::Event::default();
        e.conditions.buildings = [1, 2, 3];
        s.events.push(e);
        let d = delete_at(&mut s, &mut c, 12, 12, Page::Buildings);
        assert!(d.buildings && s.buildings.len() == 1);
        assert_eq!(s.events[0].conditions.buildings, [1, 0, 0]);
        assert_eq!((c.mark(12, 12), c.mark(14, 14)), (6, 6));
        // Items: an army, a hero start, a target place taken for an army.
        place_item(&mut s, &mut c, 3, 3, 4).unwrap();
        place_item(&mut s, &mut c, 4, 3, 5).unwrap();
        place_item(&mut s, &mut c, 6, 3, 10).unwrap();
        place_item(&mut s, &mut c, 8, 8, 3).unwrap();
        c.reveal(8, 8, 1, Reveal::Add);
        let revealed = c.revealed(8, 8);
        delete_at(&mut s, &mut c, 8, 8, Page::Items);
        assert_eq!(((s.header.heroes[2].x, s.header.heroes[2].y), c.figure(8, 8), c.revealed(8, 8)), ((0, 0), 0, revealed));
        // The target place (index 1) deletes army 1; the point itself stays.
        let d = delete_at(&mut s, &mut c, 6, 3, Page::Items);
        assert!(d.armies);
        assert_eq!((s.armies.len(), s.armies[0].id, s.armies[0].model, s.points.len()), (1, 1, 5, 1));
        assert_eq!((c.figure(4, 3), c.figure(6, 3)), (0x0501, 0), "the later army's word is written again");
        // A lantern before an event point: the point's word follows its new number.
        place_item(&mut s, &mut c, 10, 3, 9).unwrap();
        delete_at(&mut s, &mut c, 6, 6, Page::Items);
        assert_eq!(s.points.len(), 2, "an empty cell deletes nothing");
        s.points.insert(0, crate::editor::defaults::new_point(1, 1, 1, 8));
        s.points[1].id = 2;
        s.points[2].id = 3;
        c.set_figure(1, 1, 0x0801);
        c.set_figure(10, 3, 0x0903);
        delete_at(&mut s, &mut c, 1, 1, Page::Items);
        assert_eq!((s.points.len(), c.figure(10, 3), s.points[1].id), (2, 0x0902, 2));
        // The terrain page deletes nothing.
        delete_at(&mut s, &mut c, 3, 3, Page::Terrain);
        assert_eq!(s.armies.len(), 1);
    }

    #[test]
    fn burn_ruins_scorches_and_rerolls() {
        let f = facts();
        let mut s = scenario(4);
        s.terrain = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let pics = [(1, 3), (1, 0), (2, 5), (3, 4), (4, 2), (5, 0), (7, 1), (7, 4), (8, 0), (8, 1), (6, 0)];
        for (t, v) in pics {
            s.buildings.push(Building { picture_type: t, picture_variant: v, kind: t, description: "x".into(), ..Building::default() });
        }
        s.objects = vec![object(0, 0, 1, 20), object(1, 0, 5, 31), object(2, 0, 8, 10), object(0, 1, 9, 3), object(1, 1, 11, 0), object(2, 1, 12, 5)];
        let mut rng = Rng::new(77);
        burn(&mut s, &mut rng, &f);
        let mut e = Rng::new(77);
        let fort = 7 + e.random(2) as u8;
        let got: Vec<(u8, u8)> = s.buildings.iter().map(|b| (b.picture_type, b.picture_variant)).collect();
        assert_eq!(got, [(12, 13), (1, 0), (12, 14), (12, 4), (12, fort), (12, 12), (12, 9), (12, 8), (12, 12), (12, 11), (6, 0)]);
        assert!(s.buildings.iter().all(|b| b.description.is_empty()) && s.buildings[0].kind == 1, "the type stays");
        assert_eq!(s.terrain, [0, 1, 2, 3, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13]);
        // Class 10 family 9 has 4 sprites, 229 being 109's alternate; class 11 has 2 there.
        let mut tree = 108 + e.random(4) as u8;
        if tree == 109 && e.random(5) == 0 {
            tree += 120;
        }
        let thicket = 108 + e.random(2) as u8;
        assert_eq!(s.objects, vec![object(0, 0, 3, 20), object(1, 0, 6, 31), object(2, 0, 8, 10), object(0, 1, 10, tree), object(1, 1, 11, thicket), object(2, 1, 12, 5)]);
        assert_eq!(rng.state(), e.state());
    }
}
