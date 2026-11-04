//! The campaign graph as the Campaign screen draws it: the nodes (the campaign's maps, with
//! `start` and `revisit`), the ordered `next` edges (each with its `if` as text), and the hidden
//! edges that the triggers and dialogs set (`travel`, `victory … next`, `ending`), which are read
//! only. The model is read from `campaign.toml` as the text says now, and the hidden edges from the
//! package's triggers and dialogs. Node positions are kept in `.editor/layout.json` (not a package
//! file, not an undo step).
//!
//! The format is `docs/reference/rzpack-format.md` («Campaigns»); the edits are in `scenario`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value as Json};
use toml_edit::{DocumentMut, Item};

use crate::pack::load::{line_of, Package};
use crate::pack::triggers::Action;

/// The file that holds the graph.
pub const FILE: &str = "campaign.toml";
/// The file of the node positions (under `.editor/`).
pub const LAYOUT: &str = "layout.json";

/// One map of the campaign.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    /// The line of its `[[map]]` in `campaign.toml`.
    pub line: u32,
    pub revisit: bool,
    /// The map is one of the package's (`maps/<id>/`). A node that is not is drawn red.
    pub known: bool,
    /// The map's name, when its title text is in the language shown.
    pub title: Option<String>,
}

/// An ordered `next` edge: the position in its map's list (0 is tried first).
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub index: usize,
    /// The line of its `[[map.next]]`.
    pub line: u32,
    /// Its `if` conditions as they are written (one inline table each), in order.
    pub conds: Vec<String>,
}

/// Which trigger or dialog sets a hidden edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HiddenKind {
    /// `{ travel = "<map>", at = "<point>" }`: leaves mid-map.
    Travel { at: Option<String> },
    /// `{ victory = true, next = "<map>" }`: overrides the graph for that victory.
    Victory,
    /// `{ ending = "<dialog>" }`: the campaign ends with the dialog (drawn to the end sink).
    Ending,
}

/// A hidden edge: from a map (`None` when the trigger applies to every map) to a map (empty for
/// an ending), set by the trigger or dialog named in `by`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hidden {
    pub from: Option<String>,
    pub to: String,
    pub kind: HiddenKind,
    /// `trigger <id>` or `dialog <id>, choice <id>`.
    pub by: String,
    pub file: String,
    pub line: u32,
}

/// The `[carry]` flags, in the file's order, with the defaults the format gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carry {
    pub flags: [(&'static str, bool); 5],
}

/// The graph of the campaign file.
#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    /// `campaign.toml` exists in the package (the screen offers to create it otherwise).
    pub exists: bool,
    pub start: Option<String>,
    pub title: Option<String>,
    pub carry: Carry,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub hidden: Vec<Hidden>,
}

/// The `[carry]` keys, in the order the form shows them.
pub const CARRY_KEYS: [&str; 5] = ["hero", "army", "pack", "gold", "vars"];

impl Graph {
    /// The edges of map `from`, in priority order.
    pub fn edges_from<'a>(&'a self, from: &'a str) -> impl Iterator<Item = &'a Edge> + 'a {
        self.edges.iter().filter(move |e| e.from == from)
    }

    /// The node with `id`.
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

/// Reads the campaign file's text (as the package has it now) with the package's triggers and
/// dialogs. `text` is `None` when there is no `campaign.toml`: an empty graph with the maps listed.
pub fn build(pkg: &Package, text: Option<&str>, lang: &str) -> Graph {
    let title_of = |id: &str| {
        pkg.maps
            .get(id)
            .and_then(|m| m.spec.as_ref())
            .and_then(|s| pkg.texts.get(lang).or_else(|| pkg.texts.values().next()).and_then(|t| t.get(&s.title)).cloned())
    };
    let mut graph = Graph {
        exists: text.is_some(),
        start: None,
        title: None,
        carry: Carry { flags: CARRY_KEYS.map(|k| (k, true)) },
        nodes: Vec::new(),
        edges: Vec::new(),
        hidden: hidden_edges(pkg),
    };
    let Some(text) = text else { return graph };
    let Ok(doc) = text.parse::<DocumentMut>() else { return graph };
    graph.start = doc.get("start").and_then(Item::as_str).map(str::to_string);
    graph.title = doc.get("title").and_then(Item::as_str).map(str::to_string);
    if let Some(carry) = doc.get("carry").and_then(Item::as_table) {
        for (i, key) in CARRY_KEYS.iter().enumerate() {
            if let Some(on) = carry.get(key).and_then(Item::as_bool) {
                graph.carry.flags[i].1 = on;
            }
        }
    }
    let Some(Item::ArrayOfTables(maps)) = doc.get("map") else { return graph };
    for m in maps.iter() {
        let Some(id) = m.get("id").and_then(Item::as_str).map(str::to_string) else { continue };
        let line = m.span().map_or(1, |s| line_of(text, s.start));
        let known = pkg.maps.contains_key(&id);
        graph.nodes.push(Node { title: title_of(&id), line, revisit: m.get("revisit").and_then(Item::as_bool).unwrap_or(false), known, id: id.clone() });
        if let Some(Item::ArrayOfTables(next)) = m.get("next") {
            for (index, n) in next.iter().enumerate() {
                let Some(to) = n.get("to").and_then(Item::as_str).map(str::to_string) else { continue };
                let conds = match n.get("if") {
                    Some(item) => match item.as_array() {
                        Some(a) => a.iter().map(|v| v.to_string().trim().to_string()).collect(),
                        None => Vec::new(),
                    },
                    None => Vec::new(),
                };
                let line = n.span().map_or(1, |s| line_of(text, s.start));
                graph.edges.push(Edge { from: id.clone(), to, index, line, conds });
            }
        }
    }
    graph
}

/// The hidden edges of the package's triggers and dialogs. A dialog's actions are drawn from the
/// maps that open it (a trigger with a map), once per map.
fn hidden_edges(pkg: &Package) -> Vec<Hidden> {
    // Which triggers open which dialog, and on which map (`None`: every map).
    let mut openers: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
    for t in &pkg.triggers {
        for a in &t.value.then {
            if let Action::Dialog(d) = a {
                openers.entry(d.clone()).or_default().push(t.value.map.clone());
            }
        }
    }
    let mut out = Vec::new();
    for t in &pkg.triggers {
        for a in &t.value.then {
            if let Some(h) = hidden_of(a, t.value.map.clone(), format!("trigger {}", t.value.id), &t.file, t.line) {
                out.push(h);
            }
        }
    }
    for d in &pkg.dialogs {
        let from: Vec<Option<String>> = openers.get(&d.value.id).cloned().unwrap_or_default();
        for c in &d.value.choices {
            for a in &c.then {
                for src in &from {
                    if let Some(h) = hidden_of(a, src.clone(), format!("dialog {}, choice {}", d.value.id, c.id), &d.file, d.line) {
                        out.push(h);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| (a.file.as_str(), a.line, a.by.as_str()).cmp(&(b.file.as_str(), b.line, b.by.as_str())));
    out
}

fn hidden_of(a: &Action, from: Option<String>, by: String, file: &str, line: u32) -> Option<Hidden> {
    let (to, kind) = match a {
        Action::Travel { map, at } => (map.clone(), HiddenKind::Travel { at: at.clone() }),
        Action::VictoryTo(next) => (next.clone(), HiddenKind::Victory),
        Action::Ending(_) => (String::new(), HiddenKind::Ending),
        _ => return None,
    };
    Some(Hidden { from, to, kind, by, file: file.to_string(), line })
}

/// The maps in the order a hero meets them: breadth first from the start over the `next` edges
/// and the victories' and travels' hidden edges. Maps not reached come last, in file order.
pub fn order(graph: &Graph) -> Vec<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    if let Some(s) = &graph.start {
        queue.push_back(s.clone());
        seen.insert(s.clone());
    }
    while let Some(id) = queue.pop_front() {
        if graph.node(&id).is_some() {
            out.push(id.clone());
        }
        let mut next: Vec<String> = graph.edges_from(&id).map(|e| e.to.clone()).collect();
        next.extend(graph.hidden.iter().filter(|h| h.from.as_deref() == Some(id.as_str()) && !h.to.is_empty()).map(|h| h.to.clone()));
        for to in next {
            if seen.insert(to.clone()) {
                queue.push_back(to);
            }
        }
    }
    for n in &graph.nodes {
        if seen.insert(n.id.clone()) {
            out.push(n.id.clone());
        }
    }
    out
}

/// Positions for the nodes with none saved: left to right by the level of the breadth-first order
/// (a level per column, its maps down the column). Saved positions win.
pub fn layout(graph: &Graph, saved: &BTreeMap<String, [f32; 2]>) -> BTreeMap<String, [f32; 2]> {
    let mut level: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(s) = &graph.start {
        level.insert(s.clone(), 0);
    }
    // The levels: one more than the level of the edge's source (the first time a map is reached).
    let ord = order(graph);
    for id in &ord {
        let here = level.get(id).copied().unwrap_or(0);
        let mut tos: Vec<String> = graph.edges_from(id).map(|e| e.to.clone()).collect();
        tos.extend(graph.hidden.iter().filter(|h| h.from.as_deref() == Some(id.as_str()) && !h.to.is_empty()).map(|h| h.to.clone()));
        for to in tos {
            level.entry(to).or_insert(here + 1);
        }
    }
    let mut row_in_level: BTreeMap<usize, usize> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for id in &ord {
        if let Some(p) = saved.get(id) {
            out.insert(id.clone(), *p);
            continue;
        }
        let lv = level.get(id).copied().unwrap_or(0);
        let row = row_in_level.entry(lv).or_insert(0);
        out.insert(id.clone(), [40.0 + lv as f32 * 240.0, 40.0 + *row as f32 * 130.0]);
        *row += 1;
    }
    out
}

/// The saved positions of the campaign's nodes (`.editor/layout.json`, `{"campaign": {"id": [x, y]}}`).
pub fn read_layout(bytes: Option<&[u8]>) -> BTreeMap<String, [f32; 2]> {
    let Some(v) = bytes.and_then(|b| serde_json::from_slice::<Json>(b).ok()) else { return BTreeMap::new() };
    let Some(map) = v.get("campaign").and_then(Json::as_object) else { return BTreeMap::new() };
    map.iter()
        .filter_map(|(id, p)| {
            let a = p.as_array()?;
            Some((id.clone(), [a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32]))
        })
        .collect()
}

/// The text of `layout.json` with the campaign's positions set to `positions` (ids sorted). The
/// other keys of the file (the story's positions) are kept as they are.
pub fn layout_json(existing: Option<&[u8]>, positions: &BTreeMap<String, [f32; 2]>) -> Vec<u8> {
    let mut root: Json = existing.and_then(|b| serde_json::from_slice::<Json>(b).ok()).filter(Json::is_object).unwrap_or_else(|| json!({}));
    let mut campaign = serde_json::Map::new();
    for (id, [x, y]) in positions {
        campaign.insert(id.clone(), json!([round1(*x), round1(*y)]));
    }
    if let Some(obj) = root.as_object_mut() {
        obj.insert("campaign".into(), Json::Object(campaign));
    }
    let mut bytes = serde_json::to_vec_pretty(&root).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

fn round1(v: f32) -> f64 {
    (f64::from(v) * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::files::Files;

    const CAMP: &str = "start = \"brod\"\n\n[carry]\ngold = false\n\n[[map]]\nid = \"brod\"\n[[map.next]]\nto = \"marka\"\n[[map.next]]\nto = \"salt\"\nif = [ { var = \"ally\", value = 2 } ]\n\n[[map]]\nid = \"marka\"\nrevisit = true\n\n[[map]]\nid = \"salt\"\n";

    /// A package with maps brod, marka and salt, a trigger that travels and a dialog that ends.
    fn package() -> Package {
        let mut files = Files::default();
        files.insert("package.toml", "id = \"x\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"t\"\nauthor = \"a\"\nlicense = \"MIT\"\n");
        files.insert("texts/en.toml", "t = \"T\"\n\n[brod]\ntitle = \"Brod\"\n");
        for m in ["brod", "marka", "salt"] {
            files.insert(&format!("maps/{m}/map.toml"), format!("title = \"{}\"\n", if m == "brod" { "brod.title" } else { "t" }));
        }
        files.insert("triggers/a.toml", "[[trigger]]\nid = \"go\"\nmap = \"brod\"\nwhen = { event = \"day_start\" }\nthen = [ { travel = \"marka\", at = \"gate\" } ]\n\n[[trigger]]\nid = \"win\"\nmap = \"marka\"\nwhen = { event = \"day_start\" }\nthen = [ { victory = true, next = \"salt\" } ]\n\n[[trigger]]\nid = \"open\"\nmap = \"salt\"\nwhen = { event = \"day_start\" }\nthen = [ { dialog = \"the_end\" } ]\n");
        files.insert("dialogs/a.toml", "[[dialog]]\nid = \"the_end\"\ntitle = \"t\"\ntext = \"t\"\n\n[[dialog.choice]]\nid = \"ok\"\ntext = \"t\"\nthen = [ { ending = \"the_end\" } ]\n");
        let (pkg, _) = Package::parse(files);
        pkg
    }

    #[test]
    fn the_nodes_and_the_ordered_edges_read_from_the_text() {
        let pkg = package();
        let g = build(&pkg, Some(CAMP), "en");
        assert_eq!(g.start.as_deref(), Some("brod"));
        assert_eq!(g.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["brod", "marka", "salt"]);
        assert!(g.node("marka").unwrap().revisit);
        let brod: Vec<_> = g.edges_from("brod").collect();
        assert_eq!(brod.iter().map(|e| (e.to.as_str(), e.index)).collect::<Vec<_>>(), [("marka", 0), ("salt", 1)]);
        assert!(brod[0].conds.is_empty());
        assert!(brod[1].conds[0].contains("var = \"ally\""), "{:?}", brod[1].conds);
        assert!(!g.carry.flags.iter().find(|f| f.0 == "gold").unwrap().1);
        assert!(g.carry.flags.iter().find(|f| f.0 == "hero").unwrap().1, "missing flags default on");
    }

    #[test]
    fn the_hidden_edges_come_from_the_triggers_and_dialogs() {
        let g = build(&package(), Some(CAMP), "en");
        let travel = g.hidden.iter().find(|h| matches!(h.kind, HiddenKind::Travel { .. })).unwrap();
        assert_eq!((travel.from.as_deref(), travel.to.as_str()), (Some("brod"), "marka"));
        assert_eq!(travel.by, "trigger go");
        let win = g.hidden.iter().find(|h| h.kind == HiddenKind::Victory).unwrap();
        assert_eq!(win.to, "salt");
        let end = g.hidden.iter().find(|h| h.kind == HiddenKind::Ending).unwrap();
        assert_eq!(end.from.as_deref(), Some("salt"), "the dialog is opened on salt");
        assert_eq!(end.by, "dialog the_end, choice ok");
    }

    #[test]
    fn the_order_is_breadth_first_from_the_start_and_unreached_maps_come_last() {
        let g = build(&package(), Some("start = \"brod\"\n[[map]]\nid = \"brod\"\n[[map]]\nid = \"far\"\n[[map]]\nid = \"marka\"\n[[map.next]]\nto = \"x\"\n[[map]]\nid = \"salt\"\n"), "en");
        let ord = order(&g);
        assert_eq!(ord[0], "brod");
        assert_eq!(ord, ["brod", "marka", "salt", "far"], "the campaign's maps only, far last");
        assert!(ord.iter().position(|x| x == "far").unwrap() > 0);
    }

    #[test]
    fn auto_layout_puts_each_level_in_its_own_column_and_keeps_saved_positions() {
        let g = build(&package(), Some(CAMP), "en");
        let mut saved = BTreeMap::new();
        saved.insert("salt".to_string(), [500.0, 10.0]);
        let pos = layout(&g, &saved);
        assert_eq!(pos["brod"][0], 40.0);
        assert!(pos["marka"][0] > pos["brod"][0], "a level to the right");
        assert_eq!(pos["salt"], [500.0, 10.0], "the saved one wins");
    }

    #[test]
    fn the_layout_file_round_trips() {
        let mut positions = BTreeMap::new();
        positions.insert("brod".to_string(), [40.0_f32, 80.5]);
        let bytes = layout_json(None, &positions);
        assert!(String::from_utf8_lossy(&bytes).contains("\"brod\": ["));
        assert_eq!(read_layout(Some(&bytes))["brod"], [40.0, 80.5]);
        assert!(read_layout(Some(b"not json")).is_empty());
        assert!(read_layout(None).is_empty());
    }

    #[test]
    fn a_missing_campaign_is_an_empty_graph() {
        let g = build(&package(), None, "en");
        assert!(!g.exists);
        assert!(g.nodes.is_empty());
        assert_eq!(g.hidden.len(), 3, "the triggers' hidden edges still show");
    }
}
