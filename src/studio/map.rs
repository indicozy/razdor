//! The Map tab's model: which map of the package is open, its last good compile, the ids of its
//! armies and buildings in the map file and in the scenario, the hit test of a cell, the cells
//! a selection outlines, and the TOML table of a selection. No window and no UI toolkit here.
//!
//! The ids. A map file names its armies and buildings by `id`; the compiled scenario numbers them
//! 1, 2, … in file order. `Compiled.names` holds that pairing (`pack::compile::MapNames`), and
//! every lookup here goes through it.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::dt::dtm::Scenario;
use crate::dt::install::DtInstall;
use crate::pack::compile::{self, Compiled, MapNames};
use crate::pack::diag::{Diagnostic, Report, Severity};
use crate::pack::load::{line_of, Package};
use crate::pack::map::MapFile;
use crate::rules::world::footprint_cells;
use crate::studio::geometry::CellRect;
use crate::studio::edit::{Kind, Locator};

/// The ids of the package's maps (`maps/<id>/`), sorted.
pub fn map_ids(pkg: &Package) -> Vec<String> {
    pkg.maps.keys().cloned().collect()
}

/// The map a package path belongs to: `maps/<id>/…` gives `<id>`.
pub fn map_of_file(path: &str) -> Option<&str> {
    let (id, tail) = path.strip_prefix("maps/")?.split_once('/')?;
    (!id.is_empty() && !tail.is_empty()).then_some(id)
}

/// The map the Map tab shows: the map of the file last opened (when it is under `maps/<id>/` of
/// this package), else the campaign's start map, else the first map.
pub fn choose_map(pkg: &Package, opened: Option<&str>) -> Option<String> {
    if let Some(id) = opened.and_then(map_of_file).filter(|id| pkg.maps.contains_key(*id)) {
        return Some(id.to_string());
    }
    if let Some(start) = pkg.campaign.as_ref().map(|c| c.start.as_str()).filter(|s| pkg.maps.contains_key(*s)) {
        return Some(start.to_string());
    }
    pkg.maps.keys().next().cloned()
}

/// What the Map tab and the Scene and Inspector docks have selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Hero,
    /// A building or an army, by its `id` in the map file.
    Building(String),
    Army(String),
    /// A point, by its index in the map file's `[[point]]` list.
    Point(usize),
    /// A terrain object, by its index in the map file's `[[object]]` list (the Scene's rows; the
    /// canvas does not pick objects).
    Object(usize),
}

/// A compile that worked: the map it is of and the compiled map (the canvas draws its scenario).
/// `serial` changes with every compile and with every repaint.
pub struct Good {
    pub map: String,
    pub compiled: Compiled,
    pub serial: u64,
}

/// The Map tab's model: the map shown and its last good compile.
#[derive(Default)]
pub struct MapView {
    pub map: Option<String>,
    pub good: Option<Good>,
    /// Set when the map must be compiled again (opened, switched, saved, checked).
    pub stale: bool,
    serial: u64,
}

impl MapView {
    /// Shows map `id`; the next compile is for it. Nothing happens when it is already shown.
    pub fn show(&mut self, id: &str) {
        if self.map.as_deref() != Some(id) {
            self.map = Some(id.to_string());
            self.stale = true;
        }
    }

    /// Compiles the shown map on `install` (`lang` picks the texts). Returns the diagnostics of a
    /// failed compile, empty when it worked. A failed compile keeps the last good one.
    pub fn compile(&mut self, pkg: &Package, install: &DtInstall, lang: &str) -> Vec<Diagnostic> {
        self.stale = false;
        let Some(map) = self.map.clone() else { return Vec::new() };
        match compile::compile(pkg, install, lang, &map) {
            Ok(compiled) => {
                self.serial += 1;
                self.good = Some(Good { map, compiled, serial: self.serial });
                Vec::new()
            }
            Err(report) => report.diagnostics,
        }
    }

    /// Takes a compile made elsewhere (the build thread's) of map `map`. Only the map shown is taken;
    /// a failed compile keeps the last good one and returns its diagnostics.
    pub fn install(&mut self, map: &str, compiled: Result<Compiled, Report>) -> Vec<Diagnostic> {
        self.stale = false;
        if self.map.as_deref() != Some(map) {
            return Vec::new();
        }
        match compiled {
            Ok(compiled) => {
                self.serial += 1;
                self.good = Some(Good { map: map.to_string(), compiled, serial: self.serial });
                Vec::new()
            }
            Err(report) => report.diagnostics,
        }
    }

    /// Shows the codes of painted cells on the last good compile at once (before the rebuild). `cells`
    /// are `(x, y, code)`; the canvas redraws from the changed scenario (a new serial).
    pub fn repaint(&mut self, cells: &[(u32, u32, u8)]) {
        let Some(good) = self.good.as_mut() else { return };
        let w = good.compiled.scenario.width();
        for &(x, y, code) in cells {
            if let Some(c) = good.compiled.scenario.terrain.get_mut((y * w + x) as usize) {
                *c = code;
            }
        }
        self.serial += 1;
        good.serial = self.serial;
    }

    /// The last good compile, when it is of the map shown.
    pub fn current(&self) -> Option<&Good> {
        self.good.as_ref().filter(|g| self.map.as_deref() == Some(g.map.as_str()))
    }
}

fn name_of<T: Copy + PartialEq>(names: &BTreeMap<String, T>, value: T) -> Option<&str> {
    names.iter().find(|(_, v)| **v == value).map(|(k, _)| k.as_str())
}

/// The selection under a cell: an army on it, else a building whose footprint covers it, else the
/// hero's start. `None` on nothing.
pub fn hit(compiled: &Compiled, spec: &MapFile, (x, y): (i32, i32)) -> Option<Selection> {
    let s = &compiled.scenario;
    if let Some(i) = s.armies.iter().position(|a| a.x as i32 == x && a.y as i32 == y) {
        return name_of(&compiled.names.armies, (i + 1) as u8).map(|id| Selection::Army(id.to_string()));
    }
    if let Some(i) = s.buildings.iter().position(|b| footprint_cells((b.x as i32, b.y as i32), (b.size_x.max(1) as i32, b.size_y.max(1) as i32)).any(|c| c == (x, y))) {
        return name_of(&compiled.names.buildings, (i + 1) as u16).map(|id| Selection::Building(id.to_string()));
    }
    (spec.hero.cell[0] as i32 == x && spec.hero.cell[1] as i32 == y).then_some(Selection::Hero)
}

/// The cells a selection outlines: a building's footprint (its extra row too), an army's or a
/// point's cell, the hero's start. Empty when the selection is not on the compiled map.
pub fn outline_of(compiled: &Compiled, spec: &MapFile, sel: &Selection) -> Vec<CellRect> {
    let names: &MapNames = &compiled.names;
    let s: &Scenario = &compiled.scenario;
    match sel {
        Selection::Hero => {
            let [x, y] = spec.hero.cell;
            vec![CellRect::brush(x as i32, y as i32, 1)]
        }
        Selection::Building(id) => names
            .buildings
            .get(id)
            .and_then(|n| s.buildings.get(*n as usize - 1))
            .map(|b| footprint_rects((b.x as i32, b.y as i32), (b.size_x.max(1) as i32, b.size_y.max(1) as i32)))
            .unwrap_or_default(),
        Selection::Army(id) => names
            .armies
            .get(id)
            .and_then(|n| s.armies.get(*n as usize - 1))
            .map(|a| vec![CellRect::brush(a.x as i32, a.y as i32, 1)])
            .unwrap_or_default(),
        Selection::Point(i) => spec.point.get(*i).map(|p| p.get_ref().cell).map(|c| vec![CellRect::brush(c[0] as i32, c[1] as i32, 1)]).unwrap_or_default(),
        Selection::Object(i) => spec.object.get(*i).map(|o| o.get_ref().cell).map(|c| vec![CellRect::brush(c[0] as i32, c[1] as i32, 1)]).unwrap_or_default(),
    }
}

/// A building's footprint as one rectangle per row of cells (the engine's footprint, `rules::world`).
pub fn footprint_rects(anchor: (i32, i32), size: (i32, i32)) -> Vec<CellRect> {
    let mut rows: BTreeMap<i32, (i32, i32)> = BTreeMap::new();
    for (x, y) in footprint_cells(anchor, size) {
        let e = rows.entry(y).or_insert((x, x));
        e.0 = e.0.min(x);
        e.1 = e.1.max(x);
    }
    rows.into_iter().map(|(y, (x0, x1))| CellRect { x0, y0: y, x1, y1: y }).collect()
}

/// The footprint cells of every building of the compiled map (for the Footprints overlay).
pub fn footprint_cells_of(compiled: &Compiled) -> Vec<(i32, i32)> {
    compiled
        .scenario
        .buildings
        .iter()
        .flat_map(|b| footprint_cells((b.x as i32, b.y as i32), (b.size_x.max(1) as i32, b.size_y.max(1) as i32)).collect::<Vec<_>>())
        .collect()
}

/// A canvas mark: a table of the shown map that a diagnostic points at, with the diagnostic's
/// severity and message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub sel: Selection,
    pub severity: Severity,
    pub message: String,
}

/// The tables a diagnostic is about: the table whose lines hold the diagnostic's line (in `file`, the
/// map's `map.toml`, whose text is `text`); for an overlap, the building it overlaps as well; with no
/// line, the building or army its message names. Empty when nothing maps (the Problems list keeps it).
pub fn targets(d: &Diagnostic, file: &str, text: &str, spans: &[(Locator, Range<usize>)]) -> Vec<Selection> {
    let mut out = Vec::new();
    if d.file == file {
        if let Some(line) = d.line {
            for (loc, r) in spans {
                let first = line_of(text, r.start);
                let last = line_of(text, r.end.saturating_sub(1).max(r.start));
                if (first..=last).contains(&line) {
                    if let Some(sel) = selection_of(loc) {
                        out.push(sel);
                    }
                }
            }
        }
    }
    if let Some(other) = quoted_after(&d.message, "overlaps building ") {
        out.push(Selection::Building(other.to_string()));
    }
    if d.line.is_none() && d.file == file {
        if let Some(id) = quoted_after(&d.message, "building ") {
            out.push(Selection::Building(id.to_string()));
        } else if let Some(id) = quoted_after(&d.message, "army ") {
            out.push(Selection::Army(id.to_string()));
        }
    }
    out
}

/// The text between the back-quotes that follow `key` in `message` (`building `a`` → `a`).
fn quoted_after<'m>(message: &'m str, key: &str) -> Option<&'m str> {
    let at = message.find(key)? + key.len();
    let rest = message[at..].strip_prefix('`')?;
    rest.split('`').next().filter(|s| !s.is_empty())
}

/// The marks of a map's file: each diagnostic of the file, with the selections it is about.
pub fn marks(file: &str, text: &str, spans: &[(Locator, Range<usize>)], diags: &[Diagnostic]) -> Vec<Mark> {
    let mut out = Vec::new();
    for d in diags {
        for sel in targets(d, file, text, spans) {
            out.push(Mark { sel, severity: d.severity, message: d.message.clone() });
        }
    }
    out
}

/// The cell a selection's table gives in the map file (a building's is its footprint's
/// bottom-right cell), from the file's text model.
pub fn cell_of(spec: &MapFile, sel: &Selection) -> Option<[u32; 2]> {
    Some(match sel {
        Selection::Hero => spec.hero.cell,
        Selection::Building(id) => spec.building.iter().find(|b| b.get_ref().id == *id)?.get_ref().cell,
        Selection::Army(id) => spec.army.iter().find(|a| a.get_ref().id == *id)?.get_ref().cell,
        Selection::Point(i) => spec.point.get(*i)?.get_ref().cell,
        Selection::Object(i) => spec.object.get(*i)?.get_ref().cell,
    })
}

/// The map file's table of a selection, as the studio's edits and its linked selection name it.
/// `file` is the map's file (`maps/<id>/map.toml`).
pub fn locator_of(file: &str, spec: &MapFile, sel: &Selection) -> Option<Locator> {
    let at = |kind, id: Option<String>, index| Some(Locator { file: file.to_string(), kind, id, index });
    match sel {
        Selection::Hero => at(Kind::Hero, None, 0),
        Selection::Building(id) => at(Kind::Building, Some(id.clone()), 0),
        Selection::Army(id) => at(Kind::Army, Some(id.clone()), 0),
        Selection::Point(i) => at(Kind::Point, spec.point.get(*i)?.get_ref().id.clone(), *i),
        Selection::Object(i) => at(Kind::Object, None, *i),
    }
}

/// The selection of a map table, the inverse of [`locator_of`]: a `[[building]]` or `[[army]]`
/// by its id, the others by their index.
pub fn selection_of(loc: &Locator) -> Option<Selection> {
    Some(match loc.kind {
        Kind::Hero => Selection::Hero,
        Kind::Building => Selection::Building(loc.id.clone()?),
        Kind::Army => Selection::Army(loc.id.clone()?),
        Kind::Point => Selection::Point(loc.index),
        Kind::Object => Selection::Object(loc.index),
    })
}

/// The point a selection sits on, in cell units (the middle of its cell), from the map file.
pub fn centre_of(spec: &MapFile, sel: &Selection) -> Option<(f32, f32)> {
    let [x, y] = cell_of(spec, sel)?;
    Some((x as f32 + 0.5, y as f32 + 0.5))
}

/// The TOML table of a selection, from the map file's text.
pub struct Inspect {
    /// The 1-based line of the table's header (`[hero]`, `[[building]]` …).
    pub line: u32,
    /// The table's fields in file order: the key and its value in TOML syntax on one line.
    pub fields: Vec<(String, String)>,
}

/// Finds the table of `sel` in the map file `text`: the `[hero]` table, the `[[building]]` or
/// `[[army]]` entry with that `id`, or the `[[point]]` at that index.
pub fn inspect(text: &str, sel: &Selection) -> Result<Inspect, String> {
    let table: toml::Table = text.parse::<toml::Table>().map_err(|e| e.to_string())?;
    let (header, nth, t) = match sel {
        Selection::Hero => {
            let t = table.get("hero").and_then(toml::Value::as_table).ok_or("the map has no [hero] table")?;
            ("[hero]", 0, t)
        }
        Selection::Building(id) => {
            let (n, t) = element(&table, "building", Some(id), 0)?;
            ("[[building]]", n, t)
        }
        Selection::Army(id) => {
            let (n, t) = element(&table, "army", Some(id), 0)?;
            ("[[army]]", n, t)
        }
        Selection::Point(i) => {
            let (n, t) = element(&table, "point", None, *i)?;
            ("[[point]]", n, t)
        }
        Selection::Object(i) => {
            let (n, t) = element(&table, "object", None, *i)?;
            ("[[object]]", n, t)
        }
    };
    let line = header_line(text, header, nth).ok_or_else(|| format!("{header} not found in the file"))?;
    Ok(Inspect { line, fields: ordered_fields(text, line, t) })
}

/// The `nth` (0-based) array entry of `key`: the one with `id`, or the one at `index`.
fn element<'t>(table: &'t toml::Table, key: &str, id: Option<&str>, index: usize) -> Result<(usize, &'t toml::Table), String> {
    let arr = table.get(key).and_then(toml::Value::as_array).ok_or_else(|| format!("the map has no [[{key}]] entries"))?;
    let at = match id {
        Some(id) => arr.iter().position(|v| v.get("id").and_then(toml::Value::as_str) == Some(id)),
        None => (index < arr.len()).then_some(index),
    };
    let at = at.ok_or_else(|| format!("no [[{key}]] entry for this selection"))?;
    let t = arr[at].as_table().ok_or_else(|| format!("[[{key}]] entry {at} is not a table"))?;
    Ok((at, t))
}

/// The 1-based line of the `nth` (0-based) line whose trimmed text is `header`.
fn header_line(text: &str, header: &str, nth: usize) -> Option<u32> {
    text.lines().enumerate().filter(|(_, l)| l.trim() == header).nth(nth).map(|(i, _)| i as u32 + 1)
}

/// The fields of a table in the order the file has them: each key is looked for in the lines of
/// its section (from the header to the next header). Keys the section does not show go last.
fn ordered_fields(text: &str, header_line: u32, t: &toml::Table) -> Vec<(String, String)> {
    let section: Vec<&str> = text
        .lines()
        .skip(header_line as usize)
        .take_while(|l| !l.trim_start().starts_with('['))
        .collect();
    let at = |key: &str| {
        section
            .iter()
            .position(|l| l.trim_start().strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('=')))
            .unwrap_or(usize::MAX)
    };
    let mut keyed: Vec<(usize, String, String)> = t.iter().map(|(k, v)| (at(k), k.clone(), v.to_string().replace('\n', " "))).collect();
    keyed.sort_by_key(|(pos, _, _)| *pos);
    keyed.into_iter().map(|(_, k, v)| (k, v)).collect()
}
