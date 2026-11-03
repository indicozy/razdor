//! The scenario forms' edits (P3): the package settings (`package.toml`), the map header (the
//! header keys, the relations, the terrain palette and the victory of `maps/<id>/map.toml`) and the
//! campaign graph (`campaign.toml`: nodes, ordered `next` edges, `[carry]`). Each is a list of
//! [`DocEdit`]s over one file; the screens apply them as one undo step (`Workspace::apply_doc`).
//! The campaign graph's model (what is drawn) is in [`graph`].

use toml_edit::Value;

use super::doc::{entry, entry_id, table, DocEdit, Hop};
use crate::pack::manifest::{valid_version, BASES};
use crate::pack::map::parse_date;

/// The package's manifest file.
pub const PACKAGE_FILE: &str = "package.toml";
/// The licence the manifest accepts.
pub const LICENCE: &str = "MIT";

/// The map's file, package-relative.
pub fn map_file(map: &str) -> String {
    format!("maps/{map}/map.toml")
}

/// A package field: its value set, or removed when it is optional and `None`. The id is never
/// changed here (it names the package in saves).
pub fn package_field(key: &str, value: Option<Value>) -> Result<DocEdit, String> {
    match (key, value) {
        ("id", _) => Err("the id is read only: it names the package in saves".into()),
        ("version", Some(v)) if v.as_str().is_some_and(valid_version) => Ok(DocEdit::Set { path: vec![], key: key.into(), value: v }),
        ("version", _) => Err("the version is x.y.z with numbers only, e.g. 0.2.0".into()),
        ("license", Some(v)) if v.as_str() == Some(LICENCE) => Ok(DocEdit::Set { path: vec![], key: key.into(), value: v }),
        ("license", _) => Err("the licence must be \"MIT\"".into()),
        ("base", Some(v)) if v.as_str().is_some_and(|s| BASES.contains(&s)) => Ok(DocEdit::Set { path: vec![], key: key.into(), value: v }),
        ("base", _) => Err(format!("the base is one of: {}", BASES.join(", "))),
        ("title" | "author", Some(v)) if v.as_str().is_some_and(|s| !s.trim().is_empty()) => {
            Ok(DocEdit::Set { path: vec![], key: key.into(), value: v })
        }
        ("title" | "author", _) => Err(format!("{key} cannot be empty")),
        ("description", Some(v)) => Ok(DocEdit::Set { path: vec![], key: key.into(), value: v }),
        ("description", None) => Ok(DocEdit::Remove { path: vec![], key: key.into() }),
        (other, _) => Err(format!("the package has no `{other}` field the studio edits")),
    }
}

/// `start_date`: validated by the format's own rule (`pack::map::parse_date`).
pub fn start_date(text: &str) -> Result<DocEdit, String> {
    parse_date(text)?;
    Ok(DocEdit::Set { path: vec![], key: "start_date".into(), value: Value::from(text) })
}

/// The map's title or description (a text key; `None` removes the description).
pub fn map_text_key(key: &str, value: Option<&str>) -> Result<DocEdit, String> {
    match (key, value) {
        ("title", Some(v)) if !v.trim().is_empty() => Ok(DocEdit::Set { path: vec![], key: "title".into(), value: Value::from(v) }),
        ("title", _) => Err("the title is a text key".into()),
        ("description", Some(v)) => Ok(DocEdit::Set { path: vec![], key: "description".into(), value: Value::from(v) }),
        ("description", None) => Ok(DocEdit::Remove { path: vec![], key: "description".into() }),
        (other, _) => Err(format!("`{other}` is not a header key the studio edits")),
    }
}

/// The relations grid: rows and columns in the order player, ally, neighbour, enemy; each −3..3.
pub fn relations(grid: [[i8; 4]; 4]) -> Result<DocEdit, String> {
    if grid.iter().flatten().any(|v| !(-3..=3).contains(v)) {
        return Err("a relation runs from −3 to 3".into());
    }
    let rows = grid
        .iter()
        .map(|row| {
            let mut a = toml_edit::Array::new();
            for v in row {
                a.push(i64::from(*v));
            }
            Value::Array(a)
        })
        .collect::<Vec<_>>();
    let mut outer = toml_edit::Array::new();
    for r in rows {
        outer.push(r);
    }
    Ok(DocEdit::Set { path: vec![], key: "relations".into(), value: Value::Array(outer) })
}

/// A terrain palette entry: its colour set (`#rrggbb`), or removed (`None`: back to the default).
pub fn palette(kind: &str, colour: Option<&str>) -> Result<DocEdit, String> {
    if !crate::pack::terrain::DEFAULT_PALETTE.iter().any(|(k, _)| *k == kind) {
        return Err(format!("`{kind}` is not a terrain kind"));
    }
    match colour {
        Some(c) => {
            if crate::pack::terrain::parse_hex(c).is_none() {
                return Err(format!("`{c}` is not a colour: write #rrggbb"));
            }
            Ok(DocEdit::Set { path: vec![table("terrain_palette")], key: kind.into(), value: Value::from(c) })
        }
        None => Ok(DocEdit::Remove { path: vec![table("terrain_palette")], key: kind.into() }),
    }
}

/// The victory's kinds, in the form's order.
pub const VICTORY_KINDS: [&str; 4] = ["defeat_all", "defeat_army", "capture", "reach"];

/// A victory as the form holds it: its kind and the argument its kind needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VictoryDraft {
    pub kind: String,
    /// The army of `defeat_army`.
    pub army: Option<String>,
    /// The building of `capture`.
    pub building: Option<String>,
    /// The cell of `reach`.
    pub cell: Option<[u32; 2]>,
    /// The victory message's text key (optional).
    pub message: Option<String>,
}

/// The victory edits: the kind, the argument it needs, the message; the keys of the old victory
/// that the new one does not have are removed (`existing` are the keys the table has now). The
/// draft is checked first: a kind without its argument is an error.
pub fn victory(draft: &VictoryDraft, existing: &[String]) -> Result<Vec<DocEdit>, String> {
    if !VICTORY_KINDS.contains(&draft.kind.as_str()) {
        return Err(format!("`{}` is not a victory kind", draft.kind));
    }
    let path = vec![table("victory")];
    let mut keep: Vec<(&str, Value)> = vec![("kind", Value::from(draft.kind.as_str()))];
    match draft.kind.as_str() {
        "defeat_army" => {
            let a = draft.army.as_deref().filter(|a| !a.is_empty()).ok_or("a defeat-army victory needs an army")?;
            keep.push(("army", Value::from(a)));
        }
        "capture" => {
            let b = draft.building.as_deref().filter(|b| !b.is_empty()).ok_or("a capture victory needs a building")?;
            keep.push(("building", Value::from(b)));
        }
        "reach" => {
            let [x, y] = draft.cell.ok_or("a reach victory needs a cell")?;
            let mut a = toml_edit::Array::new();
            a.push(i64::from(x));
            a.push(i64::from(y));
            keep.push(("cell", Value::Array(a)));
        }
        _ => {}
    }
    if let Some(m) = draft.message.as_deref().filter(|m| !m.is_empty()) {
        keep.push(("message", Value::from(m)));
    }
    let mut out = Vec::new();
    for key in existing {
        if key != "kind" && !keep.iter().any(|(k, _)| k == key) {
            out.push(DocEdit::Remove { path: path.clone(), key: key.clone() });
        }
    }
    for (key, value) in keep {
        out.push(DocEdit::Set { path: path.clone(), key: key.into(), value });
    }
    Ok(out)
}

// The campaign graph's edits (`campaign.toml`). A map is named by its id, an edge by its position
// in its map's `[[map.next]]` list (file order), a carry flag by its key.

/// Adds a node: a `[[map]]` with the id.
pub fn add_node(id: &str) -> DocEdit {
    DocEdit::Push { path: vec![], key: "map".into(), fields: vec![("id".into(), Value::from(id))] }
}

/// Makes `map` the campaign's start.
pub fn set_start(map: &str) -> DocEdit {
    DocEdit::Set { path: vec![], key: "start".into(), value: Value::from(map) }
}

/// Sets a node's `revisit`.
pub fn set_revisit(map: &str, on: bool) -> DocEdit {
    DocEdit::Set { path: vec![entry_id("map", map)], key: "revisit".into(), value: Value::from(on) }
}

/// Appends an edge `from` → `to` (last in priority order: the new edge is tried after the others).
pub fn add_edge(from: &str, to: &str) -> DocEdit {
    DocEdit::Push { path: vec![entry_id("map", from)], key: "next".into(), fields: vec![("to".into(), Value::from(to))] }
}

/// Moves edge `at` of `from` to position `to` (its priority: 0 is tried first).
pub fn move_edge(from: &str, at: usize, to: usize) -> DocEdit {
    DocEdit::Move { path: vec![entry_id("map", from)], key: "next".into(), from: at, to }
}

/// Deletes edge `at` of `from`.
pub fn delete_edge(from: &str, at: usize) -> DocEdit {
    DocEdit::Drop { path: vec![entry_id("map", from)], key: "next".into(), pick: super::doc::Pick::Index(at) }
}

/// Sets the conditions of an edge: `conds` are inline tables (as TOML text, one each). Empty
/// removes `if` (the edge always holds).
pub fn set_edge_if(from: &str, at: usize, conds: &[Value]) -> DocEdit {
    let path: Vec<Hop> = vec![entry_id("map", from), entry("next", at)];
    if conds.is_empty() {
        return DocEdit::Remove { path, key: "if".into() };
    }
    DocEdit::Set { path, key: "if".into(), value: super::edit::array_value(conds.to_vec(), false) }
}

/// Sets one `[carry]` flag (`hero`, `army`, `pack`, `gold`, `vars`).
pub fn set_carry(key: &str, on: bool) -> DocEdit {
    DocEdit::Set { path: vec![table("carry")], key: key.into(), value: Value::from(on) }
}

/// The text of a new `campaign.toml`: the start map and one `[[map]]` for each map of the package
/// in order, with the start first.
pub fn new_campaign(start: &str, maps: &[String]) -> String {
    let mut out = String::from("# The campaign graph: the first `next` whose `if` holds is taken on a victory.\n\n");
    out.push_str(&format!("start = \"{start}\"\n\n[carry]\nhero = true\narmy = true\npack = true\ngold = true\nvars = true\n"));
    for m in std::iter::once(&start.to_string()).chain(maps.iter().filter(|m| m.as_str() != start)) {
        out.push_str(&format!("\n[[map]]\nid = \"{m}\"\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::doc::apply_all;

    const MAP: &str = "# the header\ntitle = \"t.title\" # shown\nstart_date = \"1300-04-01\"\nrelations = [[3, 3, 0, -3], [3, 3, 0, -3], [0, 0, 3, -3], [-3, -3, -3, 3]]\n\n[terrain_palette]\ngrass_plain = \"#60a040\"\n\n[hero]\ncell = [1, 2]\nclasses = [\"knight\"]\ngold = 0\nmana = 0\n\n# the win\n[victory]\nkind = \"defeat_all\"\nmessage = \"m.won\"\n";

    #[test]
    fn a_header_edit_keeps_the_comments_and_re_parses() {
        let edits = [start_date("1301-02-03").unwrap(), relations([[0, 1, 2, 3], [3, 2, 1, 0], [-1, -2, -3, 0], [0, 0, 0, 0]]).unwrap()];
        let out = apply_all(MAP, &edits).unwrap();
        assert!(out.starts_with("# the header\n"), "{out}");
        assert!(out.contains("start_date = \"1301-02-03\""), "{out}");
        assert!(out.contains("[terrain_palette]"), "the palette stays");
        let parsed: crate::pack::map::MapFile = toml::from_str(&out).expect("the edited map parses");
        assert_eq!(parsed.relations[0], [0, 1, 2, 3]);
        assert_eq!(parsed.start_date, "1301-02-03");
    }

    #[test]
    fn a_bad_date_or_relation_is_refused_before_the_file() {
        assert!(start_date("1300-13-01").is_err());
        assert!(relations([[4, 0, 0, 0], [0; 4], [0; 4], [0; 4]]).is_err());
    }

    #[test]
    fn the_palette_sets_and_removes_one_kind() {
        let out = apply_all(MAP, &[palette("marsh", Some("#5f7a4a")).unwrap()]).unwrap();
        assert!(out.contains("[terrain_palette]\ngrass_plain = \"#60a040\"\nmarsh = \"#5f7a4a\""), "{out}");
        let out = apply_all(MAP, &[palette("grass_plain", None).unwrap()]).unwrap();
        assert!(!out.contains("grass_plain"), "{out}");
        assert!(palette("grass_plain", Some("green")).is_err());
        assert!(palette("nope", Some("#000000")).is_err());
    }

    #[test]
    fn the_victory_kinds_each_re_parse() {
        let existing = vec!["kind".to_string(), "message".to_string()];
        for (kind, army, building, cell) in [
            ("defeat_all", None, None, None),
            ("defeat_army", Some("toll"), None, None),
            ("capture", None, Some("dawnwatch"), None),
            ("reach", None, None, Some([3, 4])),
        ] {
            let draft = VictoryDraft { kind: kind.into(), army: army.map(Into::into), building: building.map(Into::into), cell, message: Some("m.won".into()) };
            let edits = victory(&draft, &existing).unwrap();
            let out = apply_all(MAP, &edits).unwrap();
            let parsed: crate::pack::map::MapFile = toml::from_str(&out).unwrap_or_else(|e| panic!("{kind}: {e}\n{out}"));
            assert_eq!(parsed.victory.get_ref().message(), Some("m.won"), "{kind}");
        }
    }

    #[test]
    fn a_victory_kind_without_its_argument_is_refused() {
        let draft = VictoryDraft { kind: "reach".into(), army: None, building: None, cell: None, message: None };
        assert!(victory(&draft, &[]).is_err());
        let draft = VictoryDraft { kind: "defeat_army".into(), army: None, building: None, cell: None, message: None };
        assert!(victory(&draft, &[]).is_err());
    }

    #[test]
    fn switching_the_kind_drops_the_argument_of_the_old_one() {
        let with_army = "[victory]\nkind = \"defeat_army\"\narmy = \"toll\"\n";
        let draft = VictoryDraft { kind: "defeat_all".into(), army: None, building: None, cell: None, message: None };
        let out = apply_all(with_army, &victory(&draft, &["kind".into(), "army".into()]).unwrap()).unwrap();
        assert!(!out.contains("army"), "{out}");
        assert!(out.contains("kind = \"defeat_all\""), "{out}");
    }

    #[test]
    fn package_fields_are_checked_and_the_id_is_read_only() {
        assert!(package_field("id", Some(Value::from("x"))).is_err());
        assert!(package_field("version", Some(Value::from("1.0"))).is_err());
        assert!(package_field("version", Some(Value::from("1.2.3"))).is_ok());
        assert!(package_field("license", Some(Value::from("GPL"))).is_err());
        assert!(package_field("base", Some(Value::from("elsewhere"))).is_err());
        assert!(package_field("description", None).is_ok());
    }

    const CAMP: &str = "# the graph\nstart = \"brod\"\n\n[carry]\nhero = true\n\n[[map]]\nid = \"brod\"\n[[map.next]]\nto = \"marka\"\n[[map.next]]\nto = \"salt\"\nif = [ { var = \"ally\", value = 2 } ]\n\n[[map]]\nid = \"marka\"\nrevisit = true\n";

    #[test]
    fn an_edge_is_added_last_reordered_and_deleted() {
        let read = |text: &str| crate::pack::campaign::read(text, &mut crate::pack::diag::Report::default()).expect("the campaign reads");
        let targets = |text: &str, from: &str| read(text).map(from).unwrap().next.iter().map(|n| n.to.clone()).collect::<Vec<_>>();
        let out = apply_all(CAMP, &[add_edge("marka", "salt")]).unwrap();
        assert_eq!(targets(&out, "marka"), ["salt"]);
        let out = apply_all(CAMP, &[add_edge("brod", "salt")]).unwrap();
        assert_eq!(targets(&out, "brod"), ["marka", "salt", "salt"], "the new edge is last");
        assert_eq!(targets(&apply_all(CAMP, &[move_edge("brod", 1, 0)]).unwrap(), "brod"), ["salt", "marka"]);
        assert_eq!(targets(&apply_all(CAMP, &[delete_edge("brod", 0)]).unwrap(), "brod"), ["salt"]);
        assert!(apply_all(CAMP, &[delete_edge("brod", 0)]).unwrap().contains("# the graph"), "the comment at the top stays");
    }

    #[test]
    fn an_edge_condition_round_trips_through_the_campaign_reader() {
        let mut t = toml_edit::InlineTable::new();
        t.insert("var", Value::from("ally"));
        t.insert("op", Value::from("=="));
        t.insert("value", Value::from(1_i64));
        let read = |text: &str| crate::pack::campaign::read(text, &mut crate::pack::diag::Report::default()).expect("the campaign reads");
        let set = apply_all(CAMP, &[set_edge_if("brod", 0, &[Value::InlineTable(t)])]).unwrap();
        let parsed = read(&set);
        assert_eq!(parsed.map("brod").unwrap().next[0].cond.len(), 1, "edge 0 has its condition");
        assert_eq!(parsed.map("brod").unwrap().next[1].cond.len(), 1, "edge 1 keeps its own");
        // The edge that has an `if` loses it; the others keep theirs.
        let cleared = read(&apply_all(&set, &[set_edge_if("brod", 1, &[])]).unwrap());
        assert_eq!(cleared.map("brod").unwrap().next[0].cond.len(), 1);
        assert!(cleared.map("brod").unwrap().next[1].cond.is_empty());
    }

    #[test]
    fn the_carry_flags_and_the_start_and_the_revisit_are_set() {
        let out = apply_all(CAMP, &[set_carry("gold", false), set_start("marka"), set_revisit("brod", true)]).unwrap();
        assert!(out.contains("gold = false"), "{out}");
        assert!(out.contains("start = \"marka\""), "{out}");
        let parsed = crate::pack::campaign::read(&out, &mut crate::pack::diag::Report::default()).unwrap();
        assert_eq!(parsed.start, "marka");
        assert!(!parsed.carry.gold);
    }

    #[test]
    fn a_new_campaign_reads_back_with_the_start_first() {
        let text = new_campaign("marka", &["brod".into(), "marka".into(), "salt".into()]);
        let parsed = crate::pack::campaign::read(&text, &mut crate::pack::diag::Report::default()).unwrap();
        assert_eq!(parsed.start, "marka");
        assert_eq!(parsed.maps.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["marka", "brod", "salt"]);
    }

    #[test]
    fn a_node_is_added_once_with_its_id() {
        let out = apply_all(CAMP, &[add_node("salt")]).unwrap();
        assert!(out.ends_with("[[map]]\nid = \"salt\"\n"), "{out}");
    }
}
