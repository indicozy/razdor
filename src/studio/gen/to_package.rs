//! A generated map as a package map: `maps/<id>/terrain.png` (the terrain in the default palette)
//! and `maps/<id>/map.toml` (`docs/reference/rzpack-format.md`, «Maps»). What the format cannot
//! hold is not written: each such thing is listed in [`PackageMap::gaps`] (and in
//! `docs/OPEN_QUESTIONS.md` under «Package format gaps (studio)»). Nothing is faked: a unit
//! without a `dt:` alias, a footprint or a terrain the format lacks is an error, not a stand-in.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::dt::dtm::{Building, Scenario};
use crate::dt::dtm::Surface;
use crate::pack::aliases;
use crate::pack::refs::DtKind;
use crate::pack::terrain::{self, Palette, Terrain};

use super::model::Marks;
use super::Generated;

/// The map sizes the package holds: its picture's side is 50, 100 or 200 cells.
pub const PACKAGE_SIZES: [u32; 3] = [50, 100, 200];

/// The building types by the engine's code (`pack::compile::BUILDING_NAMES`).
const BUILDING_TYPES: [&str; 16] = [
    "palace",
    "town",
    "village",
    "castle",
    "fort",
    "tavern",
    "market",
    "church",
    "smithy",
    "shipyard",
    "altar",
    "dungeon_entrance",
    "ruins",
    "stone_bridge",
    "wooden_bridge",
    "obelisk",
];

/// The object kind of a map object's class (`pack::map` `kind`); other classes are a gap.
fn object_kind(class: u8) -> Option<(&'static str, u8)> {
    match class {
        1 => Some(("hills", 10)),
        5 => Some(("mountains", 10)),
        8 => Some(("stones", 10)),
        9 => Some(("trees", 0)),
        10 => Some(("dead_trees", 108)),
        11 => Some(("thicket", 0)),
        _ => None,
    }
}

/// The army models of the generator (its army's model 4 to 7, `newmap` 4 feudal, 5 bandits,
/// 6 peasants, 7 inactive).
fn army_model(model: u8) -> &'static str {
    match model {
        5 => "bandits",
        6 => "peasants",
        7 => "inactive",
        _ => "feudal",
    }
}

/// What the package map is made of, as text: the TOML of `map.toml`, the picture, the text
/// keys its names use (one English text for every language the package has), and what the
/// format does not hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageMap {
    pub toml: String,
    pub png: Vec<u8>,
    /// The text keys the map uses, with their texts: the title and the buildings' and armies'
    /// names. The caller adds them to every `texts/<lang>.toml`.
    pub texts: Vec<(String, String)>,
    /// What the generator made that the format does not hold (one line each).
    pub gaps: Vec<String>,
    /// The hero's start cell and the reach cell of the victory (both on land, the reach cell
    /// walkable from the start).
    pub hero: (u32, u32),
    pub reach: (u32, u32),
}

/// Why a generated map is not a package map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The map's size is not one the package holds.
    Size(u32),
    /// A terrain code the generator leaves on a cell that the package has no kind for.
    Terrain { code: u8 },
    /// A unit of the generator has no `dt:` alias (it cannot be named in a package).
    Unit { id: u32 },
    /// No land cell is walkable from the hero's start (the map has no place for the victory).
    NoReachCell,
    /// An army has more than six troops (its leader counts as one): the format holds six.
    Troops { army: usize },
}

/// The `dt:` alias of every base unit id, by the package's alias table.
pub fn unit_aliases() -> BTreeMap<u32, String> {
    aliases::names(DtKind::Unit).filter_map(|n| aliases::resolve(DtKind::Unit, n).map(|id| (id, n.to_string()))).collect()
}

/// Converts `g` to a package map with id `id` (`[a-z0-9-]+`) and title `title`. `aliases` maps
/// the unit ids to their `dt:` aliases (see [`unit_aliases`]).
pub fn to_package(g: &Generated, id: &str, title: &str, aliases: &BTreeMap<u32, String>) -> Result<PackageMap, Refusal> {
    let s = &g.scenario;
    let (w, h) = (s.width(), s.height());
    if !PACKAGE_SIZES.contains(&w) || w != h {
        return Err(Refusal::Size(w));
    }
    let mut gaps: Vec<String> = Vec::new();
    let palette = Palette::default();
    let mut codes = Vec::with_capacity(s.terrain.len());
    for &code in &s.terrain {
        match Surface::from_code(code) {
            Some(_) => codes.push(code),
            None => return Err(Refusal::Terrain { code }),
        }
    }
    let t = Terrain { width: w, height: h, cells: codes.clone() };
    let png = terrain::encode(&t, &palette);

    let mut toml = String::new();
    let key = |suffix: &str| format!("gen.{id}.{suffix}");
    let mut texts = vec![(key("title"), title.to_string())];
    let _ = writeln!(toml, "title = \"{}\"", key("title"));
    let _ = writeln!(toml, "start_date = \"1200-01-01\"");
    let _ = writeln!(toml, "relations = [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]]");

    // The hero's start and the reach cell.
    let marks = Marks::build(s);
    let walk = walkable(s, &marks);
    let hero = hero_cell(s, &walk).unwrap_or((w / 2, h / 2));
    let reach = reach_cell(s, &walk, hero).ok_or(Refusal::NoReachCell)?;
    let _ = writeln!(toml, "\n[hero]");
    let _ = writeln!(toml, "cell = [{}, {}]", hero.0, hero.1);
    let _ = writeln!(toml, "classes = [\"knight\", \"archmage\", \"ranger\"]");
    let _ = writeln!(toml, "gold = 500");
    let _ = writeln!(toml, "mana = 0");

    // Objects: the first and second slot of each cell, as the package counts them. An object under a
    // building's footprint is dropped: the original's placement clears the footprint's forest layer
    // (worldgen.md §3.5), so the generator never leaves one there, and the package would draw it over
    // the building (the checker's warning).
    let mut other_objects = 0;
    let mut under_buildings = 0;
    let covered = footprint_grid(s);
    for o in &s.objects {
        if (o.x as u32) < w && (o.y as u32) < h && covered[(o.y as u32 * w + o.x as u32) as usize] {
            under_buildings += 1;
            continue;
        }
        match object_kind(o.class) {
            Some((kind, default)) => {
                let sprite = if o.sprite == default { String::new() } else { format!("sprite = {}\n", o.sprite) };
                let _ = write!(toml, "\n[[object]]\nkind = \"{kind}\"\ncell = [{}, {}]\n{sprite}", o.x, o.y);
            }
            None => other_objects += 1,
        }
    }
    if other_objects > 0 {
        gaps.push(format!("{other_objects} objects of a class the format has no kind for (classes 2, 3, 4, 6 and 12 of the install)"));
    }
    if under_buildings > 0 {
        gaps.push(format!("{under_buildings} objects under a building's footprint were dropped (the original clears a footprint, worldgen.md §3.5)"));
    }

    // Buildings: the anchor is the bottom-right cell of the footprint, as the package's.
    let mut dropped_economy = [false; 3];
    for (k, b) in s.buildings.iter().enumerate() {
        let n = k + 1;
        let bid = format!("b{n}");
        let kind = BUILDING_TYPES.get(b.kind as usize).copied().unwrap_or("ruins");
        let name_key = key(&format!("building.{bid}.name"));
        let type_word = kind.replace('_', " ");
        let mut cap = type_word.clone();
        if let Some(first) = cap.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        texts.push((name_key.clone(), format!("{cap} {n}")));
        let _ = write!(toml, "\n[[building]]\nid = \"{bid}\"\ntype = \"{kind}\"\nname = \"{name_key}\"\ncell = [{}, {}]\nsize = [{}, {}]\nfaction = \"{}\"\n", b.x, b.y, b.size_x, b.size_y, faction_name(b.faction));
        if b.gold_per_day > 0 {
            let _ = writeln!(toml, "income = {}", b.gold_per_day);
        }
        if b.gold_max > 0 {
            let _ = writeln!(toml, "gold_max = {}", b.gold_max);
        }
        let garrison: Vec<String> = b.garrison.iter().filter(|t| t.count > 0 && t.unit != 0).map(|t| troop_text(t.unit as u32, t.level, t.count, aliases)).collect::<Result<_, _>>()?;
        if !garrison.is_empty() {
            let _ = writeln!(toml, "garrison = [{}]", garrison.join(", "));
        }
        let recruit: Vec<String> = b
            .barracks
            .iter()
            .filter(|r| r.unit != 0)
            .map(|r| {
                let unit = aliases.get(&(r.unit as u32)).ok_or(Refusal::Unit { id: r.unit as u32 })?;
                Ok(format!("{{ unit = \"dt:{unit}\", start = {}, max = {} }}", r.start_count, r.max_count))
            })
            .collect::<Result<_, Refusal>>()?;
        if !recruit.is_empty() {
            let _ = writeln!(toml, "recruit = [{}]", recruit.join(", "));
        }
        dropped_economy[0] |= b.price_min != 0 || b.price_max != 0 || b.stale_artifacts != [0; 6] || b.spells_for_sale != [0; 6];
        dropped_economy[1] |= b.mana_per_day != 0 || b.mana_max != 0;
        dropped_economy[2] |= b.random_artifacts_for_sale != 0 || b.event_count != 0;
    }
    if dropped_economy[0] {
        gaps.push("trade and library: prices, goods and spells for sale of the towns, markets and churches (the format has no shop)".into());
    }
    if dropped_economy[1] {
        gaps.push("village mana (mana a day and the stock): the format has no mana income".into());
    }
    if dropped_economy[2] {
        gaps.push("artefacts for sale and the buildings' events: the format has no shop or building events".into());
    }

    // Armies: the generator stands an army at its building's centre, which is inside the
    // footprint; the format keeps armies off every footprint and off water, so each such army
    // moves to the nearest free cell. Its leader is its first troop (the format's named leaders
    // need a name of their own): a leader that does not fit the six slots is dropped and listed.
    let blocked = occupied_cells(s);
    let mut taken: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let mut moved = 0;
    let mut dropped_leader = 0;
    let mut no_troops = 0;
    let mut army_no = 0;
    for a in &s.armies {
        let mut troops: Vec<String> = Vec::new();
        for t in a.troops.iter().filter(|t| t.count > 0 && t.unit != 0) {
            troops.push(troop_text(t.unit as u32, t.level, t.count, aliases)?);
        }
        let mut leader = None;
        if a.leader_unit != 0 {
            let text = troop_text(a.leader_unit as u32, a.leader_level, 1, aliases)?;
            if troops.len() < 6 {
                leader = Some(text);
            } else {
                dropped_leader += 1;
            }
        }
        if let Some(l) = leader {
            troops.insert(0, l);
        }
        if troops.is_empty() {
            no_troops += 1;
            continue;
        }
        army_no += 1;
        let aid = format!("a{army_no}");
        let name_key = key(&format!("army.{aid}.name"));
        texts.push((name_key.clone(), format!("Army {army_no}")));
        let at = placed_cell(s, (a.x as u32, a.y as u32), &blocked, &taken);
        if at != (a.x as u32, a.y as u32) {
            moved += 1;
        }
        taken.insert(at);
        let _ = write!(toml, "\n[[army]]\nid = \"{aid}\"\nname = \"{name_key}\"\ncell = [{}, {}]\nfaction = \"{}\"\nmodel = \"{}\"\n", at.0, at.1, faction_name(a.faction), army_model(a.model));
        if a.gold_income > 0 {
            let _ = writeln!(toml, "gold = {}", a.gold_income);
        }
        let _ = writeln!(toml, "troops = [{}]", troops.join(", "));
    }
    if moved > 0 {
        gaps.push(format!("{moved} armies moved off their building's footprint to the nearest free cell (the format keeps armies off buildings)"));
    }
    if dropped_leader > 0 {
        gaps.push(format!("{dropped_leader} army leaders dropped: the army already has six troops and the format holds six (no named leaders)"));
    }
    if no_troops > 0 {
        gaps.push(format!("{no_troops} armies with no troops left out (the format needs one troop)"));
    }
    if s.armies.iter().any(|a| a.patrols != 0 || a.patrol_radius != 0 || a.aggression != 0 || a.respawn_days != 0) {
        gaps.push("army patrols, patrol radius, aggression and respawn days: the format's army models stand for them".into());
    }

    let _ = write!(toml, "\n[victory]\nkind = \"reach\"\ncell = [{}, {}]\n", reach.0, reach.1);
    gaps.sort();
    gaps.dedup();
    Ok(PackageMap { toml, png, texts, gaps, hero, reach })
}

/// The texts of `pairs` as the package's `texts/<lang>.toml` holds them: one table per key's
/// path (`gen.map.title` is `title` in table `[gen.map]`). The caller appends them to a file.
pub fn texts_toml(pairs: &[(String, String)]) -> String {
    let mut out = String::new();
    for (key, text) in pairs {
        let (table, leaf) = key.rsplit_once('.').unwrap_or(("", key));
        let _ = writeln!(out, "\n[{table}]\n{leaf} = {}", toml::Value::from(text.as_str()));
    }
    out
}

/// The cell an army of the generator stands on, moved off every footprint, off water and off
/// the other armies: the nearest such cell in rings around its own (the first in row order).
fn placed_cell(s: &Scenario, at: (u32, u32), blocked: &std::collections::HashSet<(u32, u32)>, taken: &std::collections::HashSet<(u32, u32)>) -> (u32, u32) {
    let (w, h) = (s.width() as i64, s.height() as i64);
    let ok = |x: i64, y: i64| {
        if !(0..w).contains(&x) || !(0..h).contains(&y) {
            return false;
        }
        let c = (x as u32, y as u32);
        let code = s.terrain_at(c.0, c.1).unwrap_or(0);
        !blocked.contains(&c) && !taken.contains(&c) && !(0..=2).contains(&code) && !matches!(code, 3 | 9 | 15)
    };
    let (ax, ay) = (at.0 as i64, at.1 as i64);
    if ok(ax, ay) {
        return at;
    }
    for r in 1..=w.max(h) {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) == r && ok(ax + dx, ay + dy) {
                    return ((ax + dx) as u32, (ay + dy) as u32);
                }
            }
        }
    }
    at
}

/// A troop as the format writes it: its `dt:` alias, its level as the game shows it (the
/// generator stores it from 0) and its count.
fn troop_text(unit: u32, level: u8, count: u8, aliases: &BTreeMap<u32, String>) -> Result<String, Refusal> {
    let alias = aliases.get(&unit).ok_or(Refusal::Unit { id: unit })?;
    Ok(format!("{{ unit = \"dt:{alias}\", level = {}, count = {count} }}", level as u32 + 1))
}

fn faction_name(code: u8) -> &'static str {
    match code {
        1 => "player",
        2 => "ally",
        4 => "enemy",
        _ => "neighbour",
    }
}

/// The cells a walker can stand on: not water (fords and bridges aside, which a building's
/// footprint paves), not lava, impassable swamp or snowdrifts, not mountains, stones or
/// thickets (the marks of the classes that block).
fn walkable(s: &Scenario, marks: &Marks) -> Vec<bool> {
    let (w, h) = (s.width() as i64, s.height() as i64);
    let mut out = vec![false; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let code = s.terrain_at(x as u32, y as u32).unwrap_or(0);
            let mark = marks.values()[(y * w + x) as usize];
            let blocked = matches!(code, 0..=3 | 9 | 15) || mark == -11 || (-8..=-5).contains(&mark);
            out[(y * w + x) as usize] = !blocked;
        }
    }
    for b in &s.buildings {
        paint_footprint(b, w, h, &mut out);
    }
    out
}

/// The cells a building's footprint covers (row-major, the map's width).
fn footprint_grid(s: &Scenario) -> Vec<bool> {
    let (w, h) = (s.width() as i64, s.height() as i64);
    let mut out = vec![false; (w * h) as usize];
    for b in &s.buildings {
        paint_footprint(b, w, h, &mut out);
    }
    out
}

fn paint_footprint(b: &Building, w: i64, h: i64, out: &mut [bool]) {
    for dx in 0..b.size_x as i64 {
        for dy in 0..b.size_y as i64 {
            let (x, y) = (b.x as i64 - dx, b.y as i64 - dy);
            if (0..w).contains(&x) && (0..h).contains(&y) {
                out[(y * w + x) as usize] = true;
            }
        }
    }
}

/// The hero's start: the nearest walkable cell to the map's centre with no object on it.
fn hero_cell(s: &Scenario, walk: &[bool]) -> Option<(u32, u32)> {
    let (w, h) = (s.width() as i64, s.height() as i64);
    let (cx, cy) = (w / 2, h / 2);
    let occupied = occupied_cells(s);
    let mut best: Option<(i64, (u32, u32))> = None;
    for y in 0..h {
        for x in 0..w {
            if !walk[(y * w + x) as usize] || occupied.contains(&(x as u32, y as u32)) {
                continue;
            }
            let d = (x - cx).abs().max((y - cy).abs());
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, (x as u32, y as u32)));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// The reach cell: the walkable cell (no object on it) farthest from the hero by 4-steps over
/// walkable cells; the first in row order among equals.
fn reach_cell(s: &Scenario, walk: &[bool], hero: (u32, u32)) -> Option<(u32, u32)> {
    let (w, h) = (s.width() as i64, s.height() as i64);
    let mut dist = vec![-1i64; (w * h) as usize];
    let mut queue = std::collections::VecDeque::new();
    let start = (hero.1 as i64 * w + hero.0 as i64) as usize;
    dist[start] = 0;
    queue.push_back(start);
    while let Some(i) = queue.pop_front() {
        let (x, y) = ((i as i64) % w, (i as i64) / w);
        for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if !(0..w).contains(&nx) || !(0..h).contains(&ny) {
                continue;
            }
            let j = (ny * w + nx) as usize;
            if walk[j] && dist[j] < 0 {
                dist[j] = dist[i] + 1;
                queue.push_back(j);
            }
        }
    }
    let occupied = occupied_cells(s);
    let mut best: Option<(i64, (u32, u32))> = None;
    for (i, &d) in dist.iter().enumerate() {
        let cell = ((i as i64 % w) as u32, (i as i64 / w) as u32);
        if d <= 0 || occupied.contains(&cell) {
            continue;
        }
        if best.is_none_or(|(bd, _)| d > bd) {
            best = Some((d, cell));
        }
    }
    best.map(|(_, c)| c)
}

/// The cells an object or a building footprint covers: a hero start or a victory cell is
/// never put on one (the format's placement rules).
fn occupied_cells(s: &Scenario) -> std::collections::HashSet<(u32, u32)> {
    let mut out: std::collections::HashSet<(u32, u32)> = s.objects.iter().map(|o| (o.x as u32, o.y as u32)).collect();
    for b in &s.buildings {
        for dx in 0..b.size_x as u32 {
            for dy in 0..b.size_y as u32 {
                if let (Some(x), Some(y)) = ((b.x as u32).checked_sub(dx), (b.y as u32).checked_sub(dy)) {
                    out.insert((x, y));
                }
            }
        }
    }
    out
}
