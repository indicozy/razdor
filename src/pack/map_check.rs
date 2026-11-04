//! The rules of a package's maps: `maps/<id>/map.toml` and `maps/<id>/terrain.png`. Called by
//! [`super::check::check`]. The checks need the terrain, so a map whose picture does not decode is
//! checked for its file only.
//!
//! The rules follow the engine: a cell is water (ships only, and a bridge or a ship may be on it),
//! land a unit walks, or blocked (lava, impassable swamp and snowdrifts, the deep sea for a walker).
//! A hero, an army without a ship, a town and a reach cell stand on land; an object does not stand
//! on water.

use std::collections::{BTreeMap, BTreeSet};

use super::compile::{self, ARMY_MODELS, BUILDING_NAMES, CLASSES, NAMED_LIMIT, OBJECT_KINDS, TROOP_SLOTS};
use super::diag::Report;
use super::load::{key_line, line_of, Package};
use super::map::{parse_date, ArmySpec, BuildingSpec, MapEntry, ObjectSpec, TroopSpec, Victory};
use super::terrain::{self, Palette, Terrain, MAP_SIZES};
use crate::dt::dtm::Surface;
use crate::rules::map::{is_water, surface_minutes};
use crate::rules::world::footprint_cells;

/// Files a map folder holds.
const MAP_FILES: [&str; 2] = ["map.toml", "terrain.png"];
/// Cells a lantern's visibility may reach.
const MAX_RADIUS: u8 = 24;
/// Enemy armies `defeat_all` can count: one event checks two armies (`beaten_armies`).
pub const DEFEAT_ALL_LIMIT: usize = 2;

/// What a cell is for the rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ground {
    /// Water: ships sail it, walkers do not.
    Water,
    /// Land a walker stands on.
    Land,
    /// Blocked: no one stands on it.
    Blocked,
}

fn ground(s: Surface) -> Ground {
    if is_water(s) {
        Ground::Water
    } else if surface_minutes(s).is_none() {
        Ground::Blocked
    } else {
        Ground::Land
    }
}

/// The line of a `[table]` header (`[terrain_palette]`), or of the first `key =` line.
fn table_line(text: &str, table: &str) -> Option<u32> {
    text.lines().position(|l| l.trim() == format!("[{table}]")).map(|i| i as u32 + 1)
}

/// The rules of every map of the package, and the folders' files.
pub(super) fn check_maps(pkg: &Package, r: &mut Report) {
    let mut folders: BTreeSet<&str> = BTreeSet::new();
    for path in pkg.files.paths() {
        let Some(rest) = path.strip_prefix("maps/") else { continue };
        let Some((id, file)) = rest.split_once('/') else { continue };
        folders.insert(id);
        if file.contains('/') {
            r.warning_hint(path, None, "not read: a map folder holds map.toml and terrain.png only", "move the file or remove it");
        } else if !MAP_FILES.contains(&file) && !super::load::is_game_file(path) {
            r.warning_hint(path, None, format!("not read: a map folder holds {} only", MAP_FILES.join(" and ")), "move the file or remove it");
        }
    }
    for id in &folders {
        if !pkg.maps.contains_key(*id) {
            r.error_hint(&format!("maps/{id}"), None, "the map has no map.toml", format!("add maps/{id}/map.toml and maps/{id}/terrain.png"));
        }
    }
    for entry in pkg.maps.values() {
        check_map(pkg, entry, r);
    }
}

fn check_map(pkg: &Package, entry: &MapEntry, r: &mut Report) {
    let Some(m) = &entry.spec else { return };
    let file = entry.file.as_str();
    let text = pkg.files.text(file).unwrap_or("");
    let line = |key: &str| key_line(text, key);
    let at = |span: usize| Some(line_of(text, span));
    let units = compile::unit_ids(pkg);
    let png_path = format!("maps/{}/terrain.png", entry.id);

    // The terrain and its palette.
    let palette = match Palette::with_overrides(&m.terrain_palette) {
        Ok(p) => Some(p),
        Err(e) => {
            r.error_hint(file, table_line(text, "terrain_palette"), e, "the kinds are the palette's names, the colours #rrggbb");
            None
        }
    };
    let terrain: Option<Terrain> = match (pkg.files.get(&png_path), &palette) {
        (None, _) => {
            r.error_hint(&png_path, None, "terrain.png is missing", "paint the map's terrain: one colour per terrain kind (docs/reference/rzpack-format.md)");
            None
        }
        (Some(bytes), Some(p)) => match terrain::decode(bytes, p) {
            Ok(t) => Some(t),
            Err(problems) => {
                for p in problems {
                    r.error_hint(&png_path, None, p.message, p.hint);
                }
                None
            }
        },
        (Some(_), None) => None,
    };
    let (w, h) = terrain.as_ref().map_or((MAP_SIZES[0], MAP_SIZES[0]), |t| (t.width, t.height));
    let inside = |c: &[u32; 2]| c[0] < w && c[1] < h;
    let at_cell = |c: &[u32; 2]| terrain.as_ref().map(|t| Surface::from_code(t.cells[(c[1] * t.width + c[0]) as usize]).unwrap_or(Surface::DeepSea));
    let bad_cell = |r: &mut Report, what: &str, c: &[u32; 2], span: usize| {
        if !inside(c) {
            r.error_hint(file, at(span), format!("{what} at ({}, {}) is outside the map ({w}×{h})", c[0], c[1]), "cells count from 0 at the top left");
            return true;
        }
        false
    };

    // The dates and the relations.
    if let Err(e) = parse_date(&m.start_date) {
        r.error_hint(file, line("start_date"), e, "write the start as YYYY-MM-DD");
    }
    if m.relations.iter().flatten().any(|v| !(-3..=3).contains(v)) {
        r.error_hint(file, line("relations"), "a relation is outside −3..3", "use whole numbers from -3 to 3");
    }

    // The texts are checked with the others (`check::check_text_keys`, `text_keys`).

    // The hero.
    let hero_line = table_line(text, "hero");
    if !bad_cell(r, "the hero's start", &m.hero.cell, text.find("[hero]").unwrap_or(0)) {
        if let Some(s) = at_cell(&m.hero.cell) {
            if ground(s) != Ground::Land {
                r.error_hint(file, hero_line, format!("the hero starts on {} (no walking there)", terrain::surface_name(s)), "start him on land");
            }
        }
    }
    if m.hero.classes.is_empty() {
        r.error_hint(file, table_line(text, "hero"), "the hero is offered no class", "list one or more of: knight, archmage, ranger");
    }
    let mut classes = BTreeSet::new();
    for c in &m.hero.classes {
        if !CLASSES.contains(&c.as_str()) {
            r.error_hint(file, table_line(text, "hero"), format!("hero class `{c}` is unknown"), "use knight, archmage or ranger");
        } else if !classes.insert(c) {
            r.error(file, table_line(text, "hero"), format!("hero class `{c}` is listed twice"));
        }
    }
    check_troops(&m.hero.troops, &units, file, table_line(text, "hero"), "the hero", r);

    // Objects.
    for o in &m.object {
        let span = o.span().start;
        let o: &ObjectSpec = o.get_ref();
        if !OBJECT_KINDS.iter().any(|k| k.0 == o.kind) {
            r.error_hint(file, at(span), format!("object kind `{}` is unknown", o.kind), format!("use one of: {}", OBJECT_KINDS.map(|k| k.0).join(", ")));
            continue;
        }
        if bad_cell(r, "an object", &o.cell, span) {
            continue;
        }
        if at_cell(&o.cell).is_some_and(|s| ground(s) == Ground::Water) {
            r.error_hint(file, at(span), format!("an object on water at ({}, {})", o.cell[0], o.cell[1]), "trees and hills stand on land: move it");
        }
    }

    // Buildings.
    let mut building_ids: BTreeMap<&str, usize> = BTreeMap::new();
    // Every footprint cell and the building that covers it (`rules::world`: a building covers
    // `size` cells up and left of its cell, one more row above when wider than tall).
    let mut covered: BTreeMap<(i32, i32), &str> = BTreeMap::new();
    for (i, b) in m.building.iter().enumerate() {
        let span = b.span().start;
        let b: &BuildingSpec = b.get_ref();
        if building_ids.insert(&b.id, i).is_some() {
            r.error_hint(file, at(span), format!("building id `{}` is listed twice", b.id), "give each building its own id");
        }
        if !BUILDING_NAMES.contains(&b.kind.as_str()) {
            r.error_hint(file, at(span), format!("building type `{}` is unknown", b.kind), format!("use one of: {}", BUILDING_NAMES.join(", ")));
        }
        if compile::faction_code(&b.faction).is_none() {
            r.error_hint(file, at(span), format!("building `{}`: faction `{}` is unknown", b.id, b.faction), "use player, ally, neighbour or enemy");
        }
        if bad_cell(r, &format!("building `{}`", b.id), &b.cell, span) {
            continue;
        }
        let (fw, fh) = compile::footprint_of(b);
        if !(1..=MAX_FOOTPRINT).contains(&fw) || !(1..=MAX_FOOTPRINT).contains(&fh) {
            r.error_hint(file, at(span), format!("building `{}`: size {fw}×{fh} is outside 1 to {MAX_FOOTPRINT}", b.id), format!("a footprint is 1 to {MAX_FOOTPRINT} cells a side: size = [width, height]"));
        } else {
            let anchor = (b.cell[0] as i32, b.cell[1] as i32);
            for t in footprint_cells(anchor, (fw as i32, fh as i32)) {
                if t.0 < 0 || t.1 < 0 {
                    r.error_hint(file, at(span), format!("building `{}`: its footprint goes past the top or left edge", b.id), "move the building's cell down or right, or make it smaller");
                    break;
                }
                // The engine paves every footprint cell (`rules::world`): a cell of water or of
                // blocked ground under a footprint would become a road.
                // (The cells are at or up and left of the anchor, which is on the map.)
                let on_bad_ground = at_cell(&[t.0 as u32, t.1 as u32]).map(ground).is_some_and(|g| matches!(g, Ground::Water | Ground::Blocked));
                if on_bad_ground && !matches!(b.kind.as_str(), "stone_bridge" | "wooden_bridge") {
                    r.warning_hint(file, at(span), format!("building `{}`: its footprint covers water or blocked ground at ({}, {})", b.id, t.0, t.1), "the cell would be paved into a road: make the footprint smaller or move the building");
                }
                if let Some(other) = covered.insert(t, &b.id) {
                    if other != b.id {
                        r.error_hint(file, at(span), format!("building `{}` overlaps building `{other}` at ({}, {})", b.id, t.0, t.1), "one building per cell: move one of them or make it smaller");
                    }
                }
            }
        }
        let bridge = matches!(b.kind.as_str(), "stone_bridge" | "wooden_bridge");
        match at_cell(&b.cell).map(ground) {
            Some(Ground::Water) if !bridge => r.error_hint(
                file,
                at(span),
                format!("building `{}` stands on water", b.id),
                "only bridges stand on water: move the building onto land",
            ),
            Some(Ground::Blocked) => r.error_hint(file, at(span), format!("building `{}` stands on blocked ground", b.id), "a building needs ground a walker can stand on"),
            _ => {}
        }
        check_troops(&b.garrison, &units, file, at(span), &format!("building `{}`", b.id), r);
        if b.recruit.len() > TROOP_SLOTS {
            r.error_hint(file, at(span), format!("building `{}` recruits {} kinds: six at most", b.id, b.recruit.len()), "keep six recruit slots");
        }
        for rec in &b.recruit {
            if let Err(e) = compile::unit_of(&rec.unit, &units) {
                r.error_hint(file, at(span), format!("building `{}`: recruit: {e}", b.id), "name a dt: unit or a unit of this package");
            }
            if rec.start > rec.max {
                r.error_hint(file, at(span), format!("building `{}`: recruit `{}` starts with {} and holds {} at most", b.id, rec.unit, rec.start, rec.max), "the stock at the start is at most the maximum");
            }
        }
    }

    // The footprints: the hero's start and the objects stay off them (an object inside a footprint
    // is drawn over the building; the armies and the lanterns are checked with their own).
    let covering = |c: &[u32; 2]| covered.get(&(c[0] as i32, c[1] as i32)).copied();
    if let Some(b) = covering(&m.hero.cell) {
        r.error_hint(file, table_line(text, "hero"), format!("the hero starts inside building `{b}`'s footprint"), "start him on a cell no building covers");
    }
    for o in &m.object {
        let span = o.span().start;
        let o: &ObjectSpec = o.get_ref();
        if let Some(b) = covering(&o.cell) {
            r.warning_hint(file, at(span), format!("an object at ({}, {}) is inside building `{b}`'s footprint", o.cell[0], o.cell[1]), "it is drawn over the building: move it off the footprint");
        }
    }

    // Armies.
    let mut army_ids: BTreeMap<&str, usize> = BTreeMap::new();
    // Each army with its line and whether it stands on or next to a building (the greeters' lint).
    let mut sites: Vec<(&str, Option<u32>, bool)> = Vec::new();
    let mut named = 0usize;
    if m.army.len() > 255 {
        r.error_hint(file, None, format!("{} armies: a map holds 255 at most", m.army.len()), "an army is one byte in the file");
    }
    for (i, a) in m.army.iter().enumerate() {
        let span = a.span().start;
        let a: &ArmySpec = a.get_ref();
        if army_ids.insert(&a.id, i).is_some() {
            r.error_hint(file, at(span), format!("army id `{}` is listed twice", a.id), "give each army its own id");
        }
        let faction = compile::faction_code(&a.faction);
        if faction.is_none() {
            r.error_hint(file, at(span), format!("army `{}`: faction `{}` is unknown", a.id, a.faction), "use player, ally, neighbour or enemy");
        }
        if faction == Some(1) {
            r.error_hint(file, at(span), format!("army `{}` is the player's", a.id), "the hero is the player's army: a player army is not a map army");
        }
        if !ARMY_MODELS.iter().any(|m| m.0 == a.model) {
            r.error_hint(file, at(span), format!("army `{}`: model `{}` is unknown", a.id, a.model), format!("use one of: {}", ARMY_MODELS.map(|m| m.0).join(", ")));
        }
        let ship = a.ship.as_deref().unwrap_or("none");
        if compile::ship_code(ship).is_none() {
            r.error_hint(file, at(span), format!("army `{}`: ship `{ship}` is unknown", a.id), "use none, hero, pirate or merchant");
        }
        if a.troops.is_empty() {
            r.error_hint(file, at(span), format!("army `{}` has no troops", a.id), "give it one to six troop slots");
        }
        check_troops(&a.troops, &units, file, at(span), &format!("army `{}`", a.id), r);
        if let Some(l) = &a.leader {
            named += 1;
            if let Err(e) = compile::unit_of(&l.unit, &units) {
                r.error_hint(file, at(span), format!("army `{}`: leader: {e}", a.id), "name a dt: unit or a unit of this package");
            }
            if l.level == 0 {
                r.error_hint(file, at(span), format!("army `{}`: leader level 0", a.id), "levels start at 1");
            }
        }
        if bad_cell(r, &format!("army `{}`", a.id), &a.cell, span) {
            continue;
        }
        if let Some(b) = covering(&a.cell) {
            r.error_hint(file, at(span), format!("army `{}` stands inside building `{b}`'s footprint", a.id), "move it onto a cell no building covers");
        }
        let (ax, ay) = (a.cell[0] as i32, a.cell[1] as i32);
        let near_building = (-1..=1).any(|dx| (-1..=1).any(|dy| covered.contains_key(&(ax + dx, ay + dy))));
        sites.push((a.id.as_str(), at(span), near_building));
        let ships = ship != "none";
        match at_cell(&a.cell).map(ground) {
            Some(Ground::Water) if !ships => r.error_hint(
                file,
                at(span),
                format!("army `{}` stands on water without a ship", a.id),
                "set ship = \"pirate\" (or hero, merchant), or move it onto land",
            ),
            Some(g) if ships && g != Ground::Water => r.error_hint(file, at(span), format!("army `{}` has a ship but stands on land", a.id), "a ship sails water: move it or drop the ship"),
            Some(Ground::Blocked) => r.error_hint(file, at(span), format!("army `{}` stands on blocked ground", a.id), "move it onto ground a walker can stand on"),
            _ => {}
        }
    }
    if named > NAMED_LIMIT {
        r.error_hint(file, None, format!("{named} named leaders: a scenario holds {NAMED_LIMIT} at most"), "keep up to 32 named characters");
    }

    // Points.
    for p in &m.point {
        let span = p.span().start;
        let p = p.get_ref();
        if p.radius > MAX_RADIUS {
            r.error_hint(file, at(span), format!("a lantern's radius is {} cells: 24 at most", p.radius), "use a radius from 0 to 24");
        }
        if bad_cell(r, "a lantern", &p.cell, span) {
            continue;
        }
        if let Some(b) = covering(&p.cell) {
            r.error_hint(file, at(span), format!("a lantern stands inside building `{b}`'s footprint"), "move it onto a cell no building covers");
        }
    }

    // The victory.
    let vspan = m.victory.span().start;
    let v: &Victory = m.victory.get_ref();
    let vline = at(vspan);
    match v {
        Victory::DefeatAll { .. } => {
            let enemies = m.army.iter().filter(|a| a.get_ref().faction == "enemy").count();
            if enemies == 0 {
                r.error_hint(file, vline, "defeat_all: the map has no enemy army to beat", "add an army with faction = \"enemy\", or use another victory");
            } else if enemies > DEFEAT_ALL_LIMIT {
                r.error_hint(
                    file,
                    vline,
                    format!("defeat_all counts {enemies} enemy armies: at most {DEFEAT_ALL_LIMIT} in this slice (one event checks two armies)"),
                    "use defeat_army for the armies that matter, or keep two enemy armies",
                );
            }
        }
        Victory::DefeatArmy { army, .. } => {
            if !army_ids.contains_key(army.as_str()) {
                r.error_hint(file, vline, format!("victory: no army `{army}`"), format!("the armies are: {}", army_ids.keys().copied().collect::<Vec<_>>().join(", ")));
            }
        }
        Victory::Capture { building, .. } => match building_ids.get(building.as_str()) {
            None => r.error_hint(file, vline, format!("victory: no building `{building}`"), "name a building's id from [[building]]"),
            Some(&i) => {
                if m.building[i].get_ref().faction == "player" {
                    r.error_hint(file, vline, format!("capture: `{building}` is the player's from the start"), "a building the player holds at the start is already won: choose another or change its faction");
                }
            }
        },
        Victory::Reach { cell, .. } => {
            if !bad_cell(r, "the reach cell", cell, vspan) && at_cell(cell).is_some_and(|s| ground(s) != Ground::Land) {
                r.error_hint(file, vline, "reach: the cell is not land a walker can stand on", "choose a cell on land");
            }
        }
    }

    super::triggers::check_greeters(pkg, &entry.id, &sites, file, r);
    // The package's triggers and dialogs that apply to this map name its armies and buildings, and
    // their cells lie inside it: the map's size is known only now (`triggers::check_map_refs`).
    if let Some(t) = &terrain {
        let armies: BTreeSet<String> = army_ids.keys().map(|k| k.to_string()).collect();
        let buildings: BTreeSet<String> = building_ids.keys().map(|k| k.to_string()).collect();
        super::triggers::check_map_refs(pkg, &entry.id, (t.width, t.height), &armies, &buildings, r);
        // Walking: every named cell the hero's start reaches on foot (`reach`).
        super::reach::check_map(&pkg.files, &entry.id, t, r);
    }
}

/// The widest or tallest footprint a building may have: the original towns' 9×9 (the most common
/// town size in the install's maps) is the largest default.
pub const MAX_FOOTPRINT: u32 = 9;

/// Troop slots: at most six, each with a unit reference and a count; a hero class cannot be a
/// troop (the engine skips unit ids 1–3 in troop slots).
fn check_troops(list: &[TroopSpec], units: &BTreeMap<&str, u32>, file: &str, line: Option<u32>, who: &str, r: &mut Report) {
    if list.len() > TROOP_SLOTS {
        r.error_hint(file, line, format!("{who} has {} troop slots: six at most", list.len()), "merge the kinds into six slots");
    }
    for t in list {
        match compile::unit_of(&t.unit, units) {
            Err(e) => r.error_hint(file, line, format!("{who}: troop `{}`: {e}", t.unit), "name a dt: unit or a unit of this package"),
            Ok(id) if (1..=3).contains(&id) => {
                r.error_hint(file, line, format!("{who}: troop `{}` is a hero class", t.unit), "hero classes are not troops: use a unit such as dt:swordsman")
            }
            Ok(_) => {}
        }
        if t.level == 0 {
            r.error_hint(file, line, format!("{who}: troop `{}` has level 0", t.unit), "levels start at 1 (the first level the game shows)");
        }
        if t.count == 0 {
            r.error_hint(file, line, format!("{who}: troop `{}` has count 0", t.unit), "a slot with no units is left out");
        }
    }
}

/// The text keys of the maps: the map's title and description, the buildings' and armies' names
/// and descriptions, the named leaders' names and the victory messages. As `(key, file, line)`.
pub(super) fn text_keys(pkg: &Package) -> Vec<(String, String, Option<u32>)> {
    let mut out = Vec::new();
    for entry in pkg.maps.values() {
        let Some(m) = &entry.spec else { continue };
        let file = entry.file.clone();
        let text = pkg.files.text(&file).unwrap_or("");
        let mut push = |key: &str, line: Option<u32>| out.push((key.to_string(), file.clone(), line));
        push(&m.title, key_line(text, "title"));
        if let Some(d) = &m.description {
            push(d, key_line(text, "description"));
        }
        for b in &m.building {
            let l = Some(line_of(text, b.span().start));
            push(&b.get_ref().name, l);
            if let Some(d) = &b.get_ref().description {
                push(d, l);
            }
        }
        for a in &m.army {
            let l = Some(line_of(text, a.span().start));
            push(&a.get_ref().name, l);
            if let Some(leader) = &a.get_ref().leader {
                push(&leader.name, l);
            }
        }
        if let Some(message) = m.victory.get_ref().message() {
            push(message, Some(line_of(text, m.victory.span().start)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::files::Files;
    use crate::pack::terrain::{encode_rgb, DEFAULT_PALETTE};
    use crate::pack::Severity;

    /// Texts for every key the test map uses.
    const TEXTS: &str = "[map]\ntitle = \"Map\"\n[town]\nname = \"Town\"\n[army]\nname = \"Army\"\n[leader]\nname = \"Leader\"\n[win]\nmessage = \"Won\"\n";

    fn map_toml(extra: &str) -> String {
        format!(
            "title = \"map.title\"\nstart_date = \"1300-04-01\"\nrelations = [[3,3,0,-3],[3,3,0,-3],[0,0,3,-2],[-3,-3,-2,3]]\n\n[hero]\ncell = [5, 5]\nclasses = [\"knight\"]\ngold = 100\n\n{extra}"
        )
    }

    fn grass_png(w: u32, h: u32) -> Vec<u8> {
        encode_rgb(w, h, |_, _| DEFAULT_PALETTE[6].1)
    }

    fn pkg(map: &str, png: Vec<u8>) -> Package {
        let mut f = Files::default();
        f.insert("package.toml", "id = \"demo\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"map.title\"\nauthor = \"A\"\nlicense = \"MIT\"\n");
        f.insert("texts/en.toml", TEXTS);
        f.insert("maps/m1/map.toml", map);
        f.insert("maps/m1/terrain.png", png);
        Package::parse(f).0
    }

    fn errors(r: &Report) -> Vec<String> {
        r.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.message.clone()).collect()
    }

    #[test]
    fn defeat_all_needs_an_enemy() {
        let p = pkg(&map_toml("[victory]\nkind = \"defeat_all\"\n"), grass_png(50, 50));
        let mut r = Report::default();
        check_maps(&p, &mut r);
        assert!(errors(&r).iter().any(|m| m.contains("no enemy army")), "{}", r.to_text());
    }

    #[test]
    fn a_map_with_a_reach_victory_and_a_plain_hero_is_clean() {
        let p = pkg(&map_toml("[victory]\nkind = \"reach\"\ncell = [20, 20]\n"), grass_png(50, 50));
        let mut r = Report::default();
        check_maps(&p, &mut r);
        assert!(r.diagnostics.is_empty(), "{}", r.to_text());
    }

    #[test]
    fn a_cell_outside_the_map_is_an_error() {
        let extra = "[victory]\nkind = \"reach\"\ncell = [50, 1]\n";
        let p = pkg(&map_toml(extra), grass_png(50, 50));
        let mut r = Report::default();
        check_maps(&p, &mut r);
        assert!(errors(&r).iter().any(|m| m.contains("outside the map")), "{}", r.to_text());
    }

    #[test]
    fn a_footprint_is_one_to_nine_cells_and_stays_apart() {
        let extra = "[[building]]\nid = \"a\"\ntype = \"village\"\nname = \"town.name\"\ncell = [20, 20]\nfaction = \"neighbour\"\nsize = [10, 2]\n\n\
            [[building]]\nid = \"b\"\ntype = \"fort\"\nname = \"town.name\"\ncell = [3, 3]\nfaction = \"neighbour\"\n\n\
            [[building]]\nid = \"c\"\ntype = \"tavern\"\nname = \"town.name\"\ncell = [4, 4]\nfaction = \"neighbour\"\n\n\
            [[building]]\nid = \"d\"\ntype = \"village\"\nname = \"town.name\"\ncell = [1, 1]\nfaction = \"neighbour\"\n\n\
            [victory]\nkind = \"reach\"\ncell = [45, 45]\n";
        let p = pkg(&map_toml(extra), grass_png(50, 50));
        let mut r = Report::default();
        check_maps(&p, &mut r);
        let errs = errors(&r);
        assert!(errs.iter().any(|m| m.contains("size 10×2 is outside 1 to 9")), "{}", r.to_text());
        assert!(errs.iter().any(|m| m.contains("building `c` overlaps building `b` at (3, 3)")), "{}", r.to_text());
        assert!(errs.iter().any(|m| m.contains("building `d`: its footprint goes past the top or left edge")), "{}", r.to_text());
    }

    #[test]
    fn the_hero_and_the_armies_stay_off_the_footprints() {
        let extra = "[[building]]\nid = \"v\"\ntype = \"village\"\nname = \"town.name\"\ncell = [9, 9]\nfaction = \"neighbour\"\n\n\
            [[army]]\nid = \"e\"\nname = \"army.name\"\ncell = [7, 7]\nfaction = \"enemy\"\nmodel = \"bandits\"\ntroops = [{ unit = \"dt:bandit\", count = 2 }]\n\n\
            [[point]]\ncell = [8, 8]\nradius = 1\n\n\
            [[object]]\nkind = \"trees\"\ncell = [6, 6]\n\n[victory]\nkind = \"defeat_all\"\n";
        let p = pkg(&map_toml(extra).replace("cell = [5, 5]", "cell = [7, 8]"), grass_png(50, 50));
        let mut r = Report::default();
        check_maps(&p, &mut r);
        let errs = errors(&r);
        assert!(errs.iter().any(|m| m.contains("the hero starts inside building `v`")), "{}", r.to_text());
        assert!(errs.iter().any(|m| m.contains("army `e` stands inside building `v`")), "{}", r.to_text());
        assert!(errs.iter().any(|m| m.contains("a lantern stands inside building `v`")), "{}", r.to_text());
        assert!(r.diagnostics.iter().any(|d| d.message.contains("object at (6, 6)") && d.message.contains("footprint")), "{}", r.to_text());
    }

    #[test]
    fn an_object_on_water_is_an_error_with_its_line() {
        let blue = DEFAULT_PALETTE[1].1;
        let png = encode_rgb(50, 50, |x, _| if x == 3 { blue } else { DEFAULT_PALETTE[6].1 });
        let extra = "[[object]]\nkind = \"trees\"\ncell = [3, 4]\n\n[victory]\nkind = \"defeat_all\"\n";
        let p = pkg(&map_toml(extra), png);
        let mut r = Report::default();
        check_maps(&p, &mut r);
        let d = r.diagnostics.iter().find(|d| d.message.contains("on water")).expect("reported");
        assert_eq!(d.line, Some(10), "the object's table line");
    }

    #[test]
    fn an_army_on_water_needs_a_ship() {
        let blue = DEFAULT_PALETTE[1].1;
        let png = encode_rgb(50, 50, |x, _| if x == 9 { blue } else { DEFAULT_PALETTE[6].1 });
        let army = "[[army]]\nid = \"a\"\nname = \"army.name\"\ncell = [9, 1]\nfaction = \"enemy\"\nmodel = \"feudal\"\ntroops = [{ unit = \"dt:swordsman\", count = 2 }]\n";
        let p = pkg(&map_toml(&format!("{army}\n[victory]\nkind = \"defeat_all\"\n")), png.clone());
        let mut r = Report::default();
        check_maps(&p, &mut r);
        assert!(errors(&r).iter().any(|m| m.contains("without a ship")), "{}", r.to_text());
        let ship = army.replace("model = \"feudal\"", "model = \"feudal\"\nship = \"pirate\"");
        let p = pkg(&map_toml(&format!("{ship}\n[victory]\nkind = \"defeat_all\"\n")), png);
        let mut r = Report::default();
        check_maps(&p, &mut r);
        assert!(!errors(&r).iter().any(|m| m.contains("ship")), "{}", r.to_text());
    }

    #[test]
    fn a_unit_ref_and_a_hero_troop_are_checked() {
        let victory = "[victory]\nkind = \"reach\"\ncell = [20, 20]\n";
        let with_troops = |unit: &str| map_toml(victory).replace("gold = 100\n", &format!("gold = 100\ntroops = [{{ unit = \"{unit}\", count = 2 }}]\n"));
        let p = pkg(&with_troops("dt:swordsmen"), grass_png(50, 50));
        let mut r = Report::default();
        check_maps(&p, &mut r);
        assert!(errors(&r).iter().any(|m| m.contains("unknown unit `dt:swordsmen`")), "{}", r.to_text());
        let p = pkg(&with_troops("dt:knight"), grass_png(50, 50));
        let mut r = Report::default();
        check_maps(&p, &mut r);
        assert!(errors(&r).iter().any(|m| m.contains("is a hero class")), "{}", r.to_text());
        let p = pkg(&with_troops("dt:swordsman"), grass_png(50, 50));
        let mut r = Report::default();
        check_maps(&p, &mut r);
        assert!(r.diagnostics.is_empty(), "{}", r.to_text());
    }

    #[test]
    fn defeat_all_counts_at_most_two_enemies() {
        let mut extra = String::new();
        for i in 0..3 {
            extra.push_str(&format!("[[army]]\nid = \"e{i}\"\nname = \"army.name\"\ncell = [{}, 9]\nfaction = \"enemy\"\nmodel = \"bandits\"\ntroops = [{{ unit = \"dt:bandit\", count = 2 }}]\n\n", 10 + i));
        }
        extra.push_str("[victory]\nkind = \"defeat_all\"\n");
        let p = pkg(&map_toml(&extra), grass_png(50, 50));
        let mut r = Report::default();
        check_maps(&p, &mut r);
        assert!(errors(&r).iter().any(|m| m.contains("at most 2 in this slice")), "{}", r.to_text());
    }

    #[test]
    fn a_bad_size_and_an_unknown_colour_are_reported_by_check() {
        let p = pkg(&map_toml("[victory]\nkind = \"defeat_all\"\n"), grass_png(51, 50));
        let mut r = Report::default();
        check_maps(&p, &mut r);
        assert!(errors(&r).iter().any(|m| m.contains("51×50 cells")), "{}", r.to_text());
    }

    #[test]
    fn the_text_keys_of_a_map_are_listed() {
        let extra = "[[building]]\nid = \"t\"\ntype = \"town\"\nname = \"town.name\"\ncell = [7, 7]\nfaction = \"player\"\n\n[victory]\nkind = \"capture\"\nbuilding = \"t\"\nmessage = \"win.message\"\n";
        let p = pkg(&map_toml(extra), grass_png(50, 50));
        let keys: Vec<String> = text_keys(&p).into_iter().map(|k| k.0).collect();
        assert!(keys.contains(&"map.title".to_string()) && keys.contains(&"town.name".to_string()) && keys.contains(&"win.message".to_string()), "{keys:?}");
    }
}
