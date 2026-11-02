//! Reachability on foot of a package map's story: the cells a map or its story names must be
//! walkable from where they are reached. Those are the entrances of its buildings, its points and
//! armies, the cells its triggers name, the teleports, the travel points, the hero's arrivals and
//! the army goals (a goal is reached from its army's own cell, the rest from the hero's start).
//! Walking is the engine's: [`TileMap`] on foot, where water is never walkable and a bridge is a
//! building paved as road (`docs/reference/rzpack-format.md`, the terrain section).
//!
//! `razdor-pack check` warns for each cell it cannot reach ([`check_map`]), and the campaign test
//! (`pack::sim`) asserts that none is unreached on the compiled map ([`unreached`]).

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use super::compile::OBJECT_KINDS;
use super::diag::Report;
use super::files::Files;
use super::terrain::Terrain;
use crate::rules::map::{Decoration, Grid, TileMap};

/// A cell `(x, y)`.
pub type Cell = (i32, i32);

/// A cell a map or its story names: one cell, or an area `(x, y, w, h)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Named {
    Cell(Cell),
    Area(i32, i32, i32, i32),
}

/// A named cell and how it must be reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    /// What names it, for a person: ``building `tower` ``, ``trigger `m_setup`: `patrol` ``.
    pub what: String,
    /// The file that names it (the map's `map.toml`, or a trigger's file).
    pub file: String,
    pub named: Named,
    /// Strictly: the walk must end on the cell (an entrance, a point, a `hero_enters` cell, a
    /// teleport, a patrol or goal cell). Loosely: the cell or one next to it is enough.
    pub strict: bool,
    /// The cell the walk starts from: the army's own cell for its goals, else `None` (the hero's
    /// start).
    pub from: Option<Cell>,
}

/// The map's story as the reachability rules read it: the hero's start and every named cell.
#[derive(Clone, Debug)]
pub struct Story {
    pub hero: Cell,
    pub sites: Vec<Site>,
}

/// A named cell before its origin is known (an army goal names its army by id).
struct Raw {
    what: String,
    file: String,
    named: Named,
    strict: bool,
    army: Option<String>,
}

/// Reads the story of map `map` from `files`: its `map.toml` (the hero, buildings, armies, points)
/// and every `triggers/*.toml` that applies to it (a trigger with `map` only on that map). The
/// armies that sail (`ship`) are left out, and so are their goals. An error names the file that
/// does not read.
pub fn story(files: &Files, map: &str) -> Result<Story, String> {
    let file = format!("maps/{map}/map.toml");
    let spec = parse(files, &file)?;
    let hero = spec.get("hero").and_then(|h| h.get("cell")).and_then(pair).ok_or_else(|| format!("{file}: the hero has no cell"))?;
    let mut raw = Vec::new();
    let mut spawns: BTreeMap<String, Cell> = BTreeMap::new();
    let mut cells: BTreeMap<String, Cell> = BTreeMap::new();
    let mut ships = BTreeSet::new();
    for a in tables(spec.get("army")) {
        let id = text(a, "id");
        let Some(c) = a.get("cell").and_then(pair) else { continue };
        cells.insert(id.clone(), c);
        if a.get("ship").and_then(|s| s.as_str()).is_some_and(|s| s != "none") {
            ships.insert(id.clone());
            continue;
        }
        raw.push(Raw { what: format!("army `{id}`"), file: file.clone(), named: Named::Cell(c), strict: true, army: None });
    }
    for b in tables(spec.get("building")) {
        if let Some(c) = b.get("cell").and_then(pair) {
            raw.push(Raw { what: format!("building `{}`", text(b, "id")), file: file.clone(), named: Named::Cell(c), strict: true, army: None });
        }
    }
    // A travel point (a point with an `id`: a campaign's travel enters the map there). A lantern
    // without one only lights its cells, and nothing walks to it.
    for p in tables(spec.get("point")) {
        if let (Some(id), Some(c)) = (p.get("id").and_then(|i| i.as_str()), p.get("cell").and_then(pair)) {
            raw.push(Raw { what: format!("travel point `{id}`"), file: file.clone(), named: Named::Cell(c), strict: true, army: None });
        }
    }
    let mut trigger_files: Vec<&str> = files.paths().filter(|p| p.starts_with("triggers/") && p.ends_with(".toml")).collect();
    trigger_files.sort();
    for tf in trigger_files {
        let spec = parse(files, tf)?;
        for t in tables(spec.get("trigger")) {
            if t.get("map").and_then(|m| m.as_str()).is_some_and(|m| m != map) {
                continue;
            }
            let label = format!("trigger `{}`", text(t, "id"));
            walk_table(t, tf, &label, &mut raw, &mut spawns);
        }
    }
    let mut sites = Vec::new();
    for r in raw {
        let from = match &r.army {
            None => None,
            Some(army) if ships.contains(army) => continue,
            Some(army) => cells.get(army).or_else(|| spawns.get(army)).copied(),
        };
        sites.push(Site { what: r.what, file: r.file, named: r.named, strict: r.strict, from });
    }
    Ok(Story { hero, sites })
}

/// The foot map of map `map` as the engine walks it: the terrain (`codes`, row by row, `w` cells
/// wide), its objects, and every building's cell paved as road (`rules::world` lays the footprints).
pub fn foot_map(files: &Files, map: &str, w: i32, h: i32, codes: &[u8]) -> Result<TileMap, String> {
    let file = format!("maps/{map}/map.toml");
    let spec = parse(files, &file)?;
    let mut objects = Vec::new();
    for o in tables(spec.get("object")) {
        let kind = text(o, "kind");
        let Some(&(_, class, default)) = OBJECT_KINDS.iter().find(|k| k.0 == kind) else { continue };
        let Some(tile) = o.get("cell").and_then(pair) else { continue };
        let sprite = o.get("sprite").and_then(|s| s.as_integer()).map_or(default, |s| s as u8);
        objects.push(Decoration { tile, class, sprite });
    }
    let mut foot = TileMap::from_codes(Grid::Square8, w, h, codes, objects);
    for b in tables(spec.get("building")) {
        if let Some(c) = b.get("cell").and_then(pair) {
            foot.pave(c);
        }
    }
    Ok(foot)
}

/// The sites of `story` that the walk cannot reach on `foot` (the engine's map of the same map, or
/// [`foot_map`]), in story order. A site is reached from its origin: the army's cell for a goal,
/// else the hero's start.
pub fn unreached<'a>(foot: &TileMap, hero: Cell, story: &'a Story) -> Vec<&'a Site> {
    let mut floods: HashMap<Cell, Vec<bool>> = HashMap::new();
    let mut out = Vec::new();
    for site in story.sites.iter().filter(|s| on_map(foot, s)) {
        let origin = site.from.unwrap_or(hero);
        let seen = floods.entry(origin).or_insert_with(|| flood(foot, origin));
        if !met(foot, seen, site) {
            out.push(site);
        }
    }
    out
}

/// Warns (in `r`) for each named cell of map `map` that the hero's start cannot reach on foot, on
/// the map's terrain. A map whose story does not read is left to its own errors.
pub fn check_map(files: &Files, map: &str, terrain: &Terrain, r: &mut Report) {
    let Ok(story) = story(files, map) else { return };
    let Ok(foot) = foot_map(files, map, terrain.width as i32, terrain.height as i32, &terrain.cells) else { return };
    for site in unreached(&foot, story.hero, &story) {
        let from = if site.from.is_some() { "from its army's cell" } else { "from the hero's start" };
        r.warning_hint(
            &site.file,
            None,
            format!("{} cannot be reached on foot {from}", site.what),
            "walkers never cross water: a river is crossed on a bridge (a stone or wooden bridge on each water cell); move the cell, or add the bridge",
        );
    }
}

/// Whether the walk from the origin reaches the site: its cell (strictly), a cell next to it, or
/// any cell of its area.
fn met(foot: &TileMap, seen: &[bool], site: &Site) -> bool {
    let reached = |c: Cell| foot.in_bounds(c) && seen[(c.1 * foot.w + c.0) as usize];
    match site.named {
        Named::Cell(c) if site.strict => reached(c),
        Named::Cell(c) => (-1..=1).any(|dy| (-1..=1).any(|dx| reached((c.0 + dx, c.1 + dy)))),
        Named::Area(x, y, w, h) => {
            let (x0, x1) = span(x, w, foot.w);
            let (y0, y1) = span(y, h, foot.h);
            (y0..y1).any(|yy| (x0..x1).any(|xx| reached((xx, yy))))
        }
    }
}

/// The cells `start..start + len` of an area along an axis of `size` cells, clipped to the map. An
/// area is package input: its size may be far beyond the map (or overflow), so it is never walked
/// past the map's edge. An empty range (`lo >= hi`) means no cell of the area is on the map.
fn span(start: i32, len: i32, size: i32) -> (i32, i32) {
    (start.max(0), start.saturating_add(len).min(size))
}

/// Whether the site names at least one cell of the map. A site wholly off the map is an error
/// that the map's own checks report, so the walk leaves it out.
fn on_map(foot: &TileMap, site: &Site) -> bool {
    match site.named {
        Named::Cell(c) => foot.in_bounds(c),
        Named::Area(x, y, w, h) => {
            let (x0, x1) = span(x, w, foot.w);
            let (y0, y1) = span(y, h, foot.h);
            x0 < x1 && y0 < y1
        }
    }
}

/// The cells walkable from `from` (its own cell included, walkable or not), row by row.
fn flood(foot: &TileMap, from: Cell) -> Vec<bool> {
    let mut seen = vec![false; (foot.w * foot.h).max(0) as usize];
    if !foot.in_bounds(from) {
        return seen;
    }
    let idx = |(x, y): Cell| (y * foot.w + x) as usize;
    seen[idx(from)] = true;
    let mut queue = VecDeque::from([from]);
    while let Some(t) = queue.pop_front() {
        for n in foot.grid.neighbours(t) {
            if foot.in_bounds(n) && !seen[idx(n)] && foot.passable(n) {
                seen[idx(n)] = true;
                queue.push_back(n);
            }
        }
    }
    seen
}

/// Collects the named cells of a trigger table (or of any table under it). `at` of a `hero_enters`
/// is strict, as are `hero_at`, `go_to`, `teleport_hero`, a patrol's cells and a spawn's `at`; an
/// area is strict for `at` and `hero_at`. The other cells are loose. A `spawn_army` with an `at`
/// records where that army stands, for its goals.
fn walk_table(t: &toml::Table, file: &str, label: &str, out: &mut Vec<Raw>, spawns: &mut BTreeMap<String, Cell>) {
    let enters = t.get("event").and_then(|e| e.as_str()) == Some("hero_enters");
    let army = t.get("army_goal").and_then(|a| a.as_str()).map(str::to_string);
    let spawn = t.get("spawn_army").and_then(|a| a.as_str()).map(str::to_string);
    if let (Some(name), Some(c)) = (&spawn, t.get("at").and_then(pair)) {
        spawns.insert(name.clone(), c);
    }
    for (k, val) in t {
        let what = format!("{label}: `{k}`");
        if let Some(c) = pair(val) {
            let strict = match k.as_str() {
                "hero_at" | "go_to" | "teleport_hero" => true,
                "at" => enters || spawn.is_some(),
                _ => false,
            };
            let by = if k == "go_to" { army.clone() } else { None };
            out.push(Raw { what, file: file.to_string(), named: Named::Cell(c), strict, army: by });
        } else if let Some(a) = quad(val) {
            if k == "at" || k == "hero_at" {
                out.push(Raw { what: format!("{label}: area `{k}`"), file: file.to_string(), named: Named::Area(a.0, a.1, a.2, a.3), strict: true, army: None });
            }
        } else if let Some(list) = pairs(val) {
            for c in list {
                let by = if k == "patrol" { army.clone() } else { None };
                out.push(Raw { what: what.clone(), file: file.to_string(), named: Named::Cell(c), strict: k == "patrol", army: by });
            }
        } else {
            walk_value(val, file, label, out, spawns);
        }
    }
}

fn walk_value(v: &toml::Value, file: &str, label: &str, out: &mut Vec<Raw>, spawns: &mut BTreeMap<String, Cell>) {
    match v {
        toml::Value::Table(t) => walk_table(t, file, label, out, spawns),
        toml::Value::Array(a) => {
            for x in a {
                walk_value(x, file, label, out, spawns);
            }
        }
        _ => {}
    }
}

fn parse(files: &Files, file: &str) -> Result<toml::Value, String> {
    let text = files.text(file).ok_or_else(|| format!("{file} is missing"))?;
    toml::from_str(text).map_err(|e| format!("{file}: {e}"))
}

fn tables(v: Option<&toml::Value>) -> Vec<&toml::Table> {
    v.and_then(|v| v.as_array()).into_iter().flatten().filter_map(|x| x.as_table()).collect()
}

fn text(t: &toml::Table, key: &str) -> String {
    t.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn pair(v: &toml::Value) -> Option<Cell> {
    match v.as_array()?.as_slice() {
        [x, y] => Some((x.as_integer()? as i32, y.as_integer()? as i32)),
        _ => None,
    }
}

fn quad(v: &toml::Value) -> Option<(i32, i32, i32, i32)> {
    match v.as_array()?.as_slice() {
        [x, y, w, h] => Some((x.as_integer()? as i32, y.as_integer()? as i32, w.as_integer()? as i32, h.as_integer()? as i32)),
        _ => None,
    }
}

/// A non-empty list of cells, every item a pair (a patrol).
fn pairs(v: &toml::Value) -> Option<Vec<Cell>> {
    let list = v.as_array().filter(|l| !l.is_empty())?;
    list.iter().map(pair).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::Surface;

    /// A 6×3 map of grass with a river (coastal water) down column 2, a hero on the west bank and a
    /// building on the east bank: the building is out of reach until a bridge stands on the river.
    fn river(bridge: bool) -> (Files, Vec<u8>) {
        let mut codes = vec![Surface::GrassPlain as u8; 18];
        for y in 0..3 {
            codes[y * 6 + 2] = Surface::CoastalWater as u8;
        }
        let mut map = String::from("[hero]\ncell = [0, 1]\n\n[[building]]\nid = \"far\"\ntype = \"village\"\nname = \"x\"\nfaction = \"neighbour\"\ncell = [4, 1]\n");
        if bridge {
            map += "\n[[building]]\nid = \"bridge\"\ntype = \"wooden_bridge\"\nname = \"x\"\nfaction = \"neighbour\"\ncell = [2, 1]\n";
        }
        let mut files = Files::default();
        files.insert("maps/m/map.toml", map);
        (files, codes)
    }

    #[test]
    fn water_is_never_walked_and_a_bridge_is_a_walkable_building() {
        let (files, codes) = river(false);
        let story = super::story(&files, "m").unwrap();
        let foot = foot_map(&files, "m", 6, 3, &codes).unwrap();
        let missing: Vec<&str> = unreached(&foot, story.hero, &story).iter().map(|s| s.what.as_str()).collect();
        assert_eq!(missing, ["building `far`"], "the river cuts the east bank off");

        let (files, codes) = river(true);
        let story = super::story(&files, "m").unwrap();
        let foot = foot_map(&files, "m", 6, 3, &codes).unwrap();
        assert!(unreached(&foot, story.hero, &story).is_empty(), "a bridge on the river is walked");
    }

    #[test]
    fn a_huge_area_is_clipped_to_the_map_and_a_site_off_the_map_is_skipped() {
        let (files, codes) = river(true);
        let mut story = super::story(&files, "m").unwrap();
        let foot = foot_map(&files, "m", 6, 3, &codes).unwrap();
        let area = |x: i32, y: i32, w: i32, h: i32| Site { what: "huge".into(), file: "f".into(), named: Named::Area(x, y, w, h), strict: true, from: None };
        story.sites = vec![area(0, 0, 2_000_000_000, 2_000_000_000), area(2_000_000_000, 2_000_000_000, 2_000_000_000, 2_000_000_000), area(-5, -5, i32::MAX, 1)];
        let started = std::time::Instant::now();
        assert!(unreached(&foot, story.hero, &story).is_empty(), "the first area holds reached cells; the others are off the map and skipped");
        assert!(started.elapsed() < std::time::Duration::from_secs(1), "an area is clipped, not walked");
    }

    #[test]
    fn a_ship_army_and_its_goals_are_left_out() {
        let (mut files, codes) = river(false);
        files.insert("maps/m/map.toml", format!("{}\n[[army]]\nid = \"boat\"\ncell = [2, 0]\nship = \"pirate\"\n", files.text("maps/m/map.toml").unwrap()));
        let story = super::story(&files, "m").unwrap();
        assert!(story.sites.iter().all(|s| !s.what.contains("boat")), "a ship sails the water");
        assert!(foot_map(&files, "m", 6, 3, &codes).is_ok());
    }
}
