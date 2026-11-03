//! Who names an army, a building or a travel point of a map: the triggers, the dialogs' choices and
//! the map's own victory that refer to it by its id (the reference graph of
//! `docs/reference/rzpack-editor-guide.md` §2). The scan reads the files' text, so each use carries
//! the line it is on. Triggers that name another map (`map = "…"`) are left out; dialogs are read
//! whatever map opens them (a dialog has no map of its own).

use toml_edit::{Document, Item, Table, Value};

use super::load::{line_of, Package};

/// What is looked for: an army, a building or a travel point, by its id in the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target<'a> {
    Army(&'a str),
    Building(&'a str),
    Point(&'a str),
}

/// One reference: the file, its 1-based line and what holds it (`trigger `id``, `dialog `d`, choice `c``, `the map's victory`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Use {
    pub file: String,
    pub line: u32,
    pub what: String,
}

/// The kind of object a key's value names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Named {
    Army,
    Building,
}

/// Facts of one table that decide what a key names (`to` is a building in `teleport_army`, an
/// entity's `of = "building"` makes it one).
#[derive(Default)]
struct Flags {
    teleport: bool,
    of_building: bool,
    travel: bool,
}

impl Flags {
    fn of(pairs: &[(&str, &Value)]) -> Flags {
        Flags {
            teleport: pairs.iter().any(|(k, _)| *k == "teleport_army"),
            of_building: pairs.iter().any(|(k, v)| *k == "of" && v.as_str() == Some("building")),
            travel: pairs.iter().any(|(k, _)| *k == "travel"),
        }
    }
}

/// Where a scan is: the file, its text (for the lines), what holds the tables, and the target.
struct Ctx<'a> {
    file: &'a str,
    text: &'a str,
    what: String,
    target: Target<'a>,
}

/// The references to `target` on map `map` of the package, in file and line order.
pub fn uses(pkg: &Package, map: &str, target: Target) -> Vec<Use> {
    let mut out = Vec::new();
    let map_file = format!("maps/{map}/map.toml");
    if let Some(text) = pkg.files.text(&map_file) {
        if let Ok(doc) = Document::parse(text) {
            if let Some(Item::Table(victory)) = doc.get("victory") {
                let ctx = Ctx { file: &map_file, text, what: "the map's victory".to_string(), target };
                scan_table(victory, &ctx, &mut out);
            }
        }
    }
    let mut paths: Vec<&str> = pkg.files.paths().filter(|p| (p.starts_with("triggers/") || p.starts_with("dialogs/")) && p.ends_with(".toml")).collect();
    paths.sort_unstable();
    for path in paths {
        let Some(text) = pkg.files.text(path) else { continue };
        let Ok(doc) = Document::parse(text) else { continue };
        if path.starts_with("triggers/") {
            for t in tables(doc.get("trigger")) {
                let id = str_of(t, "id");
                if t.get("map").and_then(Item::as_str).is_some_and(|m| m != map) {
                    continue;
                }
                let ctx = Ctx { file: path, text, what: format!("trigger `{id}`"), target };
                scan_table(t, &ctx, &mut out);
            }
        } else {
            for d in tables(doc.get("dialog")) {
                let dialog = str_of(d, "id");
                for c in tables(d.get("choice")) {
                    let choice = str_of(c, "id");
                    let ctx = Ctx { file: path, text, what: format!("dialog `{dialog}`, choice `{choice}`"), target };
                    scan_table(c, &ctx, &mut out);
                }
            }
        }
    }
    out.sort_by(|a, b| (a.file.as_str(), a.line).cmp(&(b.file.as_str(), b.line)));
    out
}

/// The tables of an array of tables (`[[name]]`), or none.
fn tables(item: Option<&Item>) -> Vec<&Table> {
    item.and_then(Item::as_array_of_tables).into_iter().flat_map(|a| a.iter()).collect()
}

fn str_of<'a>(t: &'a Table, key: &str) -> &'a str {
    t.get(key).and_then(Item::as_str).unwrap_or("?")
}

/// A table's keys and values, with its sub-tables read as inline tables are.
fn scan_table(t: &Table, ctx: &Ctx, out: &mut Vec<Use>) {
    let pairs: Vec<(&str, &Value)> = t.iter().filter_map(|(k, i)| i.as_value().map(|v| (k, v))).collect();
    scan_pairs(&pairs, ctx, out);
    for (_, item) in t.iter() {
        if let Item::Table(sub) = item {
            scan_table(sub, ctx, out);
        }
    }
}

fn scan_pairs(pairs: &[(&str, &Value)], ctx: &Ctx, out: &mut Vec<Use>) {
    let flags = Flags::of(pairs);
    for (key, value) in pairs {
        if names(key, value, &flags, ctx.target) {
            let offset = value.span().map_or(0, |s| s.start);
            out.push(Use { file: ctx.file.to_string(), line: line_of(ctx.text, offset), what: ctx.what.clone() });
        }
        match value {
            Value::InlineTable(it) => {
                let inner: Vec<(&str, &Value)> = it.iter().collect();
                scan_pairs(&inner, ctx, out);
            }
            Value::Array(a) => {
                for v in a.iter() {
                    if let Value::InlineTable(it) = v {
                        let inner: Vec<(&str, &Value)> = it.iter().collect();
                        scan_pairs(&inner, ctx, out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Whether `key = value` in a table with `flags` names the target.
fn names(key: &str, value: &Value, flags: &Flags, target: Target) -> bool {
    let Some(name) = value.as_str() else { return false };
    if let Target::Point(id) = target {
        return key == "at" && flags.travel && name == id;
    }
    let (id, want) = match target {
        Target::Army(id) => (id, Named::Army),
        Target::Building(id) => (id, Named::Building),
        Target::Point(_) => return false,
    };
    if name != id {
        return false;
    }
    let kind = match key {
        "army" | "army_at" | "army_alive" | "army_goal" | "army_speed" | "army_figure" | "remove_army" | "teleport_army" | "start_battle" | "follow"
        | "attack" | "in" => Named::Army,
        "to" if flags.teleport => Named::Building,
        "to" => Named::Army,
        "building" | "garrison" | "go_to_building" | "building_owner" | "hero_enters_building" | "building_captured" | "building_lost" => Named::Building,
        "set_faction" | "set_relation" | "add_units" if flags.of_building => Named::Building,
        "set_faction" | "set_relation" | "add_units" => Named::Army,
        _ => return false,
    };
    kind == want
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::files::Files;

    fn package(files: &[(&str, &str)]) -> Package {
        let mut f = Files::default();
        for (p, t) in files {
            f.insert(p, *t);
        }
        Package::parse(f).0
    }

    #[test]
    fn finds_armies_buildings_and_travel_points_with_their_lines() {
        let pkg = package(&[
            ("maps/m/map.toml", "title = \"t\"\n[victory]\nkind = \"defeat_army\"\narmy = \"toll\"\n"),
            (
                "triggers/a.toml",
                "[[trigger]]\nid = \"ambush\"\nwhen = { event = \"army_defeated\", army = \"toll\" }\nthen = [\n  { army_goal = \"toll\", go_to_building = \"gate\" },\n  { set_faction = \"toll\", faction = \"ally\" },\n]\n\n[[trigger]]\nid = \"elsewhere\"\nmap = \"other\"\nwhen = { event = \"hero_enters_building\", building = \"gate\" }\n\n[[trigger]]\nid = \"walls\"\nwhen = { event = \"hero_enters_building\", building = \"gate\" }\nthen = [{ give_item = \"x\", to = \"toll\" }, { teleport_army = \"toll\", to = \"gate\" }]\n",
            ),
            (
                "dialogs/d.toml",
                "[[dialog]]\nid = \"d\"\ntitle = \"t\"\ntext = \"x\"\n[[dialog.choice]]\nid = \"c\"\ntext = \"x\"\nthen = [{ add_units = \"garrison\", garrison = \"gate\", troops = [] }]\n",
            ),
            ("triggers/t.toml", "[[trigger]]\nid = \"walk\"\nwhen = { event = \"hero_enters\", at = [1, 2] }\nthen = [{ travel = \"marka\", at = \"gate\" }]\n"),
        ]);
        let armies = uses(&pkg, "m", Target::Army("toll"));
        let whats: Vec<(&str, u32)> = armies.iter().map(|u| (u.what.as_str(), u.line)).collect();
        assert!(whats.contains(&("the map's victory", 4)), "{whats:?}");
        assert!(whats.contains(&("trigger `ambush`", 3)), "army_defeated: {whats:?}");
        assert!(whats.contains(&("trigger `ambush`", 5)), "army_goal: {whats:?}");
        assert!(whats.contains(&("trigger `ambush`", 6)), "set_faction names an army: {whats:?}");
        assert!(whats.contains(&("trigger `walls`", 17)), "give_item to an army: {whats:?}");

        let gates = uses(&pkg, "m", Target::Building("gate"));
        let whats: Vec<&str> = gates.iter().map(|u| u.what.as_str()).collect();
        assert!(whats.contains(&"trigger `ambush`"), "go_to_building names a building: {whats:?}");
        assert!(whats.contains(&"trigger `walls`"), "{whats:?}");
        assert!(whats.contains(&"dialog `d`, choice `c`"), "garrison names a building: {whats:?}");
        assert!(!whats.contains(&"trigger `elsewhere`"), "a trigger of another map is left out");
        let teleport = gates.iter().filter(|u| u.what == "trigger `walls`").count();
        assert_eq!(teleport, 2, "the event's building and teleport_army's `to`");

        let points = uses(&pkg, "marka", Target::Point("gate"));
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].file, "triggers/t.toml");
    }
}
