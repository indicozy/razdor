//! The story flow graph of one map (the Story screen): the dialogs and the triggers that apply to
//! the map are nodes, a dialog's choices are its output ports, and each variable the story names
//! is a node of its own. The edges are labelled: `opens` and `ends` (to a dialog), `picked` (a
//! choice to the `dialog_choice` trigger that answers it), `changes` and `if X` (a variable to what
//! it gates), `sets X` (what writes it). The model is read from the package's parsed triggers and
//! dialogs; the node positions are kept in `.editor/layout.json` under `story`, per map.
//!
//! The graph is what the files say: the wiring the screen makes is `studio::story` edits.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value as Json};

use crate::pack::load::Package;
use crate::pack::triggers::{Action, Cond, Event};

/// The key of the story's positions in `layout.json` (`{"story": {"<map>": {"<node>": [x, y]}}}`).
pub const SECTION: &str = "story";

/// What a node is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dialog,
    Trigger,
    Var,
}

/// One node: its id (`d:<dialog>`, `t:<trigger>` or `v:<var>`), its kind and name, and where its
/// table is (the file and the line; a variable has none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    /// A short line for the node: a trigger's event, a dialog's title key, a variable's type.
    pub detail: String,
    /// A dialog's choice ids, in the order shown: its output ports.
    pub choices: Vec<String>,
    pub file: String,
    pub line: u32,
}

/// A labelled edge from a node (or one of a dialog's ports) to a node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: String,
    /// The choice the edge leaves from (a dialog's port), `None` for the node itself.
    pub port: Option<String>,
    pub to: String,
    pub label: String,
}

/// The graph of one map.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Story {
    pub map: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Story {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

/// The node id of a dialog, a trigger or a variable.
pub fn dialog_id(name: &str) -> String {
    format!("d:{name}")
}

pub fn trigger_id(name: &str) -> String {
    format!("t:{name}")
}

pub fn var_id(name: &str) -> String {
    format!("v:{name}")
}

/// A trigger's event as a short line.
pub fn event_text(e: &Event) -> String {
    match e {
        Event::GameStart => "game_start".into(),
        Event::DayStart => "day_start".into(),
        Event::WeekStart => "week_start".into(),
        Event::NewMoon => "new_moon".into(),
        Event::HeroEnters(p) => format!("hero_enters {p:?}"),
        Event::HeroEntersBuilding(b) => format!("hero_enters_building {b}"),
        Event::ArmyDefeated(a) => format!("army_defeated {a}"),
        Event::BuildingCaptured(b) => format!("building_captured {b}"),
        Event::BuildingLost(b) => format!("building_lost {b}"),
        Event::SimVictory => "sim_victory".into(),
        Event::DialogChoice { dialog, choice } => format!("dialog_choice {dialog}.{choice}"),
        Event::VarChanged(v) => format!("var_changed {v}"),
        Event::ArmyArrives { army, .. } => format!("army_arrives {army}"),
    }
}

/// The graph of a map: the triggers that apply to it (those without a map and those naming it), the
/// dialogs they open or answer (and the dialogs those open), the dialogs of `dialogs/<map>.toml`,
/// and the variables that the edges name.
pub fn build(pkg: &Package, map: &str) -> Story {
    let applies = |m: &Option<String>| m.as_deref().is_none_or(|m| m == map);
    let triggers: Vec<_> = pkg.triggers.iter().filter(|t| applies(&t.value.map)).collect();
    // The dialogs in the story: opened or ended by a trigger of the map, answered by one, or
    // opened or ended by a choice of one of them; and those of the map's own file.
    let mut included: BTreeSet<String> = BTreeSet::new();
    for t in &triggers {
        for a in &t.value.then {
            if let Some(d) = opened_dialog(a) {
                included.insert(d.to_string());
            }
        }
        if let Event::DialogChoice { dialog, .. } = &t.value.when {
            included.insert(dialog.clone());
        }
    }
    let own_file = format!("dialogs/{map}.toml");
    for d in &pkg.dialogs {
        if d.file == own_file {
            included.insert(d.value.id.clone());
        }
    }
    loop {
        let before = included.len();
        let reached: Vec<String> = pkg
            .dialogs
            .iter()
            .filter(|d| included.contains(&d.value.id))
            .flat_map(|d| d.value.choices.iter())
            .flat_map(|c| c.then.iter())
            .filter_map(opened_dialog)
            .map(str::to_string)
            .collect();
        included.extend(reached);
        if included.len() == before {
            break;
        }
    }
    let mut story = Story { map: map.to_string(), ..Story::default() };
    let mut vars: BTreeSet<String> = BTreeSet::new();
    for d in pkg.dialogs.iter().filter(|d| included.contains(&d.value.id)) {
        story.nodes.push(Node {
            id: dialog_id(&d.value.id),
            kind: Kind::Dialog,
            name: d.value.id.clone(),
            detail: d.value.title.clone(),
            choices: d.value.choices.iter().map(|c| c.id.clone()).collect(),
            file: d.file.clone(),
            line: d.line,
        });
    }
    for t in &triggers {
        story.nodes.push(Node {
            id: trigger_id(&t.value.id),
            kind: Kind::Trigger,
            name: t.value.id.clone(),
            detail: event_text(&t.value.when),
            choices: Vec::new(),
            file: t.file.clone(),
            line: t.line,
        });
    }
    for t in &triggers {
        let to = trigger_id(&t.value.id);
        match &t.value.when {
            Event::DialogChoice { dialog, choice } if included.contains(dialog) => {
                story.edges.push(Edge { from: dialog_id(dialog), port: Some(choice.clone()), to: to.clone(), label: "picked".into() });
            }
            Event::VarChanged(v) => {
                vars.insert(v.clone());
                story.edges.push(Edge { from: var_id(v), port: None, to: to.clone(), label: "changes".into() });
            }
            _ => {}
        }
        for c in &t.value.cond {
            if let Cond::Var { name, .. } = c {
                vars.insert(name.clone());
                story.edges.push(Edge { from: var_id(name), port: None, to: to.clone(), label: format!("if {name}") });
            }
        }
        for a in &t.value.then {
            if let Some(v) = written_var(a) {
                vars.insert(v.to_string());
                story.edges.push(Edge { from: to.clone(), port: None, to: var_id(v), label: format!("sets {v}") });
            }
            if let Some(d) = opened_dialog(a) {
                story.edges.push(Edge { from: to.clone(), port: None, to: dialog_id(d), label: ends_or_opens(a).into() });
            }
        }
    }
    for d in pkg.dialogs.iter().filter(|d| included.contains(&d.value.id)) {
        let from = dialog_id(&d.value.id);
        for c in &d.value.choices {
            for cond in &c.cond {
                if let Cond::Var { name, .. } = cond {
                    vars.insert(name.clone());
                    story.edges.push(Edge { from: var_id(name), port: None, to: from.clone(), label: format!("if {name}") });
                }
            }
            for a in &c.then {
                if let Some(v) = written_var(a) {
                    vars.insert(v.to_string());
                    story.edges.push(Edge { from: from.clone(), port: Some(c.id.clone()), to: var_id(v), label: format!("sets {v}") });
                }
                if let Some(next) = opened_dialog(a) {
                    story.edges.push(Edge { from: from.clone(), port: Some(c.id.clone()), to: dialog_id(next), label: ends_or_opens(a).into() });
                }
            }
        }
    }
    for v in &vars {
        let kind = pkg.vars.iter().find(|d| &d.value.name == v).map(|d| match d.value.kind {
            crate::pack::triggers::VarKind::Int => "number",
            crate::pack::triggers::VarKind::Bool => "yes/no",
        });
        let (file, line) = pkg.vars.iter().find(|d| &d.value.name == v).map(|d| (d.file.clone(), d.line)).unwrap_or_default();
        story.nodes.push(Node { id: var_id(v), kind: Kind::Var, name: v.clone(), detail: kind.unwrap_or("not declared").into(), choices: Vec::new(), file, line });
    }
    story
}

/// The dialog an action opens (`dialog`) or ends with (`ending`).
fn opened_dialog(a: &Action) -> Option<&str> {
    match a {
        Action::Dialog(d) | Action::Ending(d) => Some(d),
        _ => None,
    }
}

fn ends_or_opens(a: &Action) -> &'static str {
    match a {
        Action::Ending(_) => "ends",
        _ => "opens",
    }
}

/// The variable an action writes.
fn written_var(a: &Action) -> Option<&str> {
    match a {
        Action::Set { name, .. } | Action::Add { name, .. } | Action::RandomVar { name, .. } => Some(name),
        _ => None,
    }
}

/// The size of a node on the canvas, in points: a dialog grows with its choices, a trigger is a
/// fixed box and a variable a pill.
pub fn size(n: &Node) -> [f32; 2] {
    match n.kind {
        Kind::Dialog => [200.0, 34.0 + 20.0 * n.choices.len().max(1) as f32],
        Kind::Trigger => [200.0, 54.0],
        Kind::Var => [130.0, 26.0],
    }
}

/// The column of a node: the longest way to it from a node with nothing before it (a cycle is
/// cut after as many rounds as there are nodes). Variables stay in the first column: what writes
/// one does not move it, so a story that sets what it tests keeps its variables on the left.
fn levels(story: &Story) -> BTreeMap<String, usize> {
    let mut level: BTreeMap<String, usize> = story.nodes.iter().map(|n| (n.id.clone(), 0)).collect();
    for _ in 0..story.nodes.len() {
        let mut moved = false;
        for e in &story.edges {
            let here = level.get(&e.from).copied().unwrap_or(0);
            let next = level.get(&e.to).copied().unwrap_or(0);
            if next < here + 1 && e.from != e.to && !e.to.starts_with("v:") {
                level.insert(e.to.clone(), here + 1);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    level
}

/// The distance between two columns of the graph, in points (a node is 200 wide).
const COLUMN_W: f32 = 300.0;
/// The gap between two nodes of a column.
const ROW_GAP: f32 = 24.0;

/// Positions for the nodes with none saved. Nodes are grouped by kind in three columns: the variables
/// on the left, the triggers in the middle and the dialogs on the right. Each column is ordered by the
/// node's level (what leads to it comes first), then by the story's order, and its nodes stack down
/// the column. Saved positions win.
pub fn layout(story: &Story, saved: &BTreeMap<String, [f32; 2]>) -> BTreeMap<String, [f32; 2]> {
    let level = levels(story);
    let mut columns: [Vec<&Node>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for n in &story.nodes {
        let column = match n.kind {
            Kind::Var => 0,
            Kind::Trigger => 1,
            Kind::Dialog => 2,
        };
        columns[column].push(n);
    }
    let mut out = BTreeMap::new();
    for (column, nodes) in columns.iter_mut().enumerate() {
        nodes.sort_by_key(|n| level.get(&n.id).copied().unwrap_or(0));
        let mut y = 40.0_f32;
        for n in nodes.iter() {
            if let Some(p) = saved.get(&n.id) {
                out.insert(n.id.clone(), *p);
                continue;
            }
            out.insert(n.id.clone(), [40.0 + column as f32 * COLUMN_W, y]);
            y += size(n)[1] + ROW_GAP;
        }
    }
    out
}

/// The id of the one node that stands for every variable while they are collapsed.
pub const VARS_ID: &str = "v:*";

/// The graph with its variables collapsed into one node (`VARS_ID`, its detail the count): every edge to or
/// from a variable runs to or from that node, and an edge repeated by it is kept once. A graph with no
/// variable is returned as it is.
pub fn collapse_vars(story: &Story) -> Story {
    let count = story.nodes.iter().filter(|n| n.kind == Kind::Var).count();
    if count == 0 {
        return story.clone();
    }
    let mut out = Story { map: story.map.clone(), ..Story::default() };
    out.nodes.push(Node { id: VARS_ID.into(), kind: Kind::Var, name: "*".into(), detail: count.to_string(), choices: Vec::new(), file: String::new(), line: 0 });
    out.nodes.extend(story.nodes.iter().filter(|n| n.kind != Kind::Var).cloned());
    let fold = |id: &str| if id.starts_with("v:") { VARS_ID.to_string() } else { id.to_string() };
    for e in &story.edges {
        let edge = Edge { from: fold(&e.from), port: e.port.clone(), to: fold(&e.to), label: e.label.clone() };
        if !out.edges.contains(&edge) {
            out.edges.push(edge);
        }
    }
    out
}

/// The scroll offset (in screen points) that centres a node at `p` (its top-left, graph points) of `size`
/// on the canvas drawn at zoom `z`, in a view of `view` points. The offset stays inside the canvas of
/// `canvas` points (drawn size), so the view never scrolls past its edges.
pub fn centred_offset(p: [f32; 2], size: [f32; 2], z: f32, view: [f32; 2], canvas: [f32; 2]) -> [f32; 2] {
    let axis = |at: f32, half: f32, view: f32, canvas: f32| {
        let max = (canvas - view).max(0.0);
        ((at + half) * z - view / 2.0).clamp(0.0, max)
    };
    [axis(p[0], size[0] / 2.0, view[0], canvas[0]), axis(p[1], size[1] / 2.0, view[1], canvas[1])]
}

/// The saved positions of a map's story nodes (`layout.json`).
pub fn read_layout(bytes: Option<&[u8]>, map: &str) -> BTreeMap<String, [f32; 2]> {
    let Some(v) = bytes.and_then(|b| serde_json::from_slice::<Json>(b).ok()) else { return BTreeMap::new() };
    let Some(maps) = v.get(SECTION).and_then(Json::as_object).and_then(|m| m.get(map)).and_then(Json::as_object) else {
        return BTreeMap::new();
    };
    maps.iter()
        .filter_map(|(id, p)| {
            let a = p.as_array()?;
            Some((id.clone(), [a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32]))
        })
        .collect()
}

/// The text of `layout.json` with `map`'s story positions set to `positions`. The other keys of
/// the file (the campaign's positions, others) are kept as they are.
pub fn layout_json(existing: Option<&[u8]>, map: &str, positions: &BTreeMap<String, [f32; 2]>) -> Vec<u8> {
    let mut root: Json = existing.and_then(|b| serde_json::from_slice::<Json>(b).ok()).filter(Json::is_object).unwrap_or_else(|| json!({}));
    let obj = root.as_object_mut().expect("an object");
    let story = obj.entry(SECTION).or_insert_with(|| json!({}));
    if !story.is_object() {
        *story = json!({});
    }
    let mut nodes = serde_json::Map::new();
    for (id, [x, y]) in positions {
        nodes.insert(id.clone(), json!([round1(*x), round1(*y)]));
    }
    if let Some(m) = story.as_object_mut() {
        m.insert(map.to_string(), Json::Object(nodes));
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

    /// Two dialogs (ford opens toll on its first choice), a trigger that opens ford and a
    /// `dialog_choice` trigger that sets a var; a var gates one trigger.
    fn package() -> Package {
        let mut files = Files::default();
        files.insert("package.toml", "id = \"x\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"t\"\nauthor = \"a\"\nlicense = \"MIT\"\n");
        files.insert("texts/en.toml", "t = \"T\"\n");
        files.insert("maps/brod/map.toml", "title = \"t\"\n");
        files.insert("maps/salt/map.toml", "title = \"t\"\n");
        files.insert(
            "triggers/a.toml",
            "[vars]\ndread = 0\nford_held = false\n\n[[trigger]]\nid = \"go\"\nmap = \"brod\"\nwhen = { event = \"hero_enters\", at = [1, 1] }\nif = [ { var = \"ford_held\", value = false } ]\nthen = [ { dialog = \"ford\" } ]\n\n[[trigger]]\nid = \"pick\"\nwhen = { event = \"dialog_choice\", dialog = \"ford\", choice = \"pay\" }\nthen = [ { set = \"ford_held\", value = true }, { dialog = \"toll\" } ]\n\n[[trigger]]\nid = \"elsewhere\"\nmap = \"salt\"\nwhen = { event = \"day_start\" }\nthen = [ { dialog = \"far\" } ]\n",
        );
        files.insert(
            "dialogs/a.toml",
            "[[dialog]]\nid = \"ford\"\ntitle = \"ford.title\"\ntext = \"ford.text\"\n\n[[dialog.choice]]\nid = \"pay\"\ntext = \"ford.pay\"\nif = [ { var = \"dread\", op = \"<\", value = 3 } ]\nthen = [ { add = \"dread\", value = 1 } ]\n\n[[dialog.choice]]\nid = \"run\"\ntext = \"ford.run\"\nthen = [ { dialog = \"toll\" } ]\n\n[[dialog]]\nid = \"toll\"\ntitle = \"toll.title\"\ntext = \"toll.text\"\n\n[[dialog]]\nid = \"far\"\ntitle = \"far.title\"\ntext = \"far.text\"\n",
        );
        Package::parse(files).0
    }

    #[test]
    fn the_nodes_of_a_map_are_its_triggers_and_the_dialogs_they_reach() {
        let s = build(&package(), "brod");
        let names: Vec<&str> = s.nodes.iter().filter(|n| n.kind == Kind::Dialog).map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["ford", "toll"], "far belongs to salt only");
        let triggers: Vec<&str> = s.nodes.iter().filter(|n| n.kind == Kind::Trigger).map(|n| n.name.as_str()).collect();
        assert_eq!(triggers, ["go", "pick"]);
        let ford = s.node("d:ford").unwrap();
        assert_eq!(ford.choices, ["pay", "run"], "the choices are the ports");
    }

    #[test]
    fn the_edges_are_labelled_and_leave_from_the_choice_port() {
        let s = build(&package(), "brod");
        let has = |from: &str, port: Option<&str>, to: &str, label: &str| {
            s.edges.iter().any(|e| e.from == from && e.port.as_deref() == port && e.to == to && e.label == label)
        };
        assert!(has("t:go", None, "d:ford", "opens"));
        assert!(has("v:ford_held", None, "t:go", "if ford_held"));
        assert!(has("d:ford", Some("pay"), "t:pick", "picked"));
        assert!(has("t:pick", None, "v:ford_held", "sets ford_held"));
        assert!(has("t:pick", None, "d:toll", "opens"));
        assert!(has("d:ford", Some("pay"), "v:dread", "sets dread"), "{:?}", s.edges);
        assert!(has("v:dread", None, "d:ford", "if dread"));
        assert!(has("d:ford", Some("run"), "d:toll", "opens"));
    }

    #[test]
    fn a_var_node_has_its_declared_type_and_an_undeclared_one_says_so() {
        let s = build(&package(), "brod");
        assert_eq!(s.node("v:dread").unwrap().detail, "number");
        assert_eq!(s.node("v:ford_held").unwrap().detail, "yes/no");
    }

    #[test]
    fn auto_layout_groups_the_nodes_by_kind_and_keeps_saved_positions() {
        let s = build(&package(), "brod");
        let mut saved = BTreeMap::new();
        saved.insert("t:go".to_string(), [900.0, 10.0]);
        let pos = layout(&s, &saved);
        assert_eq!(pos["t:go"], [900.0, 10.0], "the saved one wins");
        assert!(pos["v:dread"][0] < pos["t:pick"][0], "the variables are the left column");
        assert!(pos["t:pick"][0] < pos["d:ford"][0], "the triggers are one column left of the dialogs");
        assert_eq!(pos["d:ford"][0], pos["d:toll"][0], "the dialogs share their column");
        assert_eq!(pos["v:dread"][0], pos["v:ford_held"][0], "the variables share their column");
        assert_ne!(pos["v:dread"][1], pos["v:ford_held"][1], "and stack down it");
    }

    #[test]
    fn the_variables_collapse_into_one_node_with_their_edges_folded_onto_it() {
        let s = build(&package(), "brod");
        let c = collapse_vars(&s);
        assert_eq!(c.nodes.iter().filter(|n| n.kind == Kind::Var).count(), 1, "one variables node");
        assert_eq!(c.node(VARS_ID).expect("the variables node").detail, "2", "it counts the variables it stands for");
        assert!(c.edges.iter().any(|e| e.from == VARS_ID && e.to == "t:go" && e.label == "if ford_held"), "an edge runs from it");
        assert!(c.edges.iter().any(|e| e.from == "d:ford" && e.to == VARS_ID && e.label == "sets dread"), "and into it");
        assert!(c.edges.iter().all(|e| !e.from.starts_with("v:") || e.from == VARS_ID), "no edge still names one variable");
        assert!(c.edges.iter().all(|e| !e.to.starts_with("v:") || e.to == VARS_ID), "no edge still names one variable");
        assert_eq!(collapse_vars(&c).edges, c.edges, "collapsing a collapsed graph keeps its edges");
    }

    #[test]
    fn a_graph_with_no_variable_is_not_changed_by_collapsing() {
        let mut s = build(&package(), "brod");
        s.nodes.retain(|n| n.kind != Kind::Var);
        s.edges.retain(|e| !e.from.starts_with("v:") && !e.to.starts_with("v:"));
        assert_eq!(collapse_vars(&s), s);
    }

    #[test]
    fn the_view_centres_on_a_node_and_stays_inside_the_canvas() {
        // A node at (300, 200), 200 by 54, at zoom 1 on a canvas of 1000 by 800, in a view of 400 by 300.
        let off = centred_offset([300.0, 200.0], [200.0, 54.0], 1.0, [400.0, 300.0], [1000.0, 800.0]);
        assert_eq!(off, [300.0 + 100.0 - 200.0, 200.0 + 27.0 - 150.0], "the node's centre is the view's centre");
        assert_eq!(centred_offset([0.0, 0.0], [200.0, 54.0], 1.0, [400.0, 300.0], [1000.0, 800.0]), [0.0, 0.0], "not past the top-left");
        assert_eq!(centred_offset([990.0, 790.0], [200.0, 54.0], 1.0, [400.0, 300.0], [1000.0, 800.0]), [600.0, 500.0], "not past the far corner");
    }

    #[test]
    fn the_layout_file_keeps_the_other_sections_and_round_trips() {
        let mut positions = BTreeMap::new();
        positions.insert("d:ford".to_string(), [40.0_f32, 80.5]);
        let existing = br#"{"campaign": {"brod": [1, 2]}}"#;
        let bytes = layout_json(Some(existing), "brod", &positions);
        let text = String::from_utf8_lossy(&bytes).to_string();
        assert!(text.contains("\"campaign\""), "the campaign's positions stay: {text}");
        assert_eq!(read_layout(Some(&bytes), "brod")["d:ford"], [40.0, 80.5]);
        assert!(read_layout(Some(&bytes), "salt").is_empty(), "another map has none");
        assert!(read_layout(Some(b"not json"), "brod").is_empty());
    }
}
