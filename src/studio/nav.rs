//! Navigation of the Сюжет and Данные workspaces (UX5): the story outline (the triggers and dialogs
//! grouped by map, with a search) and «Открыть в файле» (the file and line where an object is written).
//! Both read the parsed package and the files' text; neither changes anything.

use crate::pack::load::{line_of, Package};

use super::edit::spans;
use super::map::{locator_of, Selection};
use super::story_graph::event_text;
use crate::pack::sim::Moment;
use crate::rules::triggers::Trace;

/// The group of the triggers that apply to every map, and of the dialogs outside a map's file.
pub const EVERY_MAP: &str = "";

/// What an outline row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutlineKind {
    Trigger,
    Dialog,
}

/// One row of the outline: a trigger (its `when` as the subtitle) or a dialog (its title key).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineRow {
    pub kind: OutlineKind,
    pub id: String,
    pub sub: String,
    pub file: String,
    pub line: u32,
}

/// The rows of one map (`map` is the map's id, `EVERY_MAP` for the rest).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineGroup {
    pub map: String,
    pub rows: Vec<OutlineRow>,
}

/// The map a dialog file belongs to: `dialogs/<map>.toml` is the map's own dialogs.
fn dialog_map(file: &str) -> &str {
    file.strip_prefix("dialogs/").and_then(|f| f.strip_suffix(".toml")).unwrap_or(EVERY_MAP)
}

/// The outline of the package: its triggers and dialogs grouped by map (the maps in id order, the
/// triggers that apply to every map and the other dialogs first). A non-empty `query` keeps the rows
/// whose id, subtitle or map contains it (case-insensitive) and drops the groups left empty.
pub fn outline(pkg: &Package, query: &str) -> Vec<OutlineGroup> {
    let mut groups: Vec<OutlineGroup> = Vec::new();
    let mut push = |map: String, row: OutlineRow| match groups.iter_mut().find(|g| g.map == map) {
        Some(g) => g.rows.push(row),
        None => groups.push(OutlineGroup { map, rows: vec![row] }),
    };
    for t in &pkg.triggers {
        let map = t.value.map.clone().unwrap_or_else(|| EVERY_MAP.to_string());
        push(map, OutlineRow { kind: OutlineKind::Trigger, id: t.value.id.clone(), sub: event_text(&t.value.when), file: t.file.clone(), line: t.line });
    }
    for d in &pkg.dialogs {
        push(dialog_map(&d.file).to_string(), OutlineRow { kind: OutlineKind::Dialog, id: d.value.id.clone(), sub: d.value.title.clone(), file: d.file.clone(), line: d.line });
    }
    // The map order: the rest first, then the maps by id.
    groups.sort_by(|a, b| (!a.map.is_empty(), &a.map).cmp(&(!b.map.is_empty(), &b.map)));
    let q = query.trim().to_lowercase();
    if !q.is_empty() {
        for g in &mut groups {
            let map_hit = g.map.to_lowercase().contains(&q);
            g.rows.retain(|r| map_hit || r.id.to_lowercase().contains(&q) || r.sub.to_lowercase().contains(&q));
        }
        groups.retain(|g| !g.rows.is_empty());
    }
    groups
}

/// What «Открыть в файле» opens: an object of the shown map, or a trigger, dialog, unit or item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Object {
    Map(Selection),
    Trigger(String),
    Dialog(String),
    Unit(String),
    Item(String),
}

/// The file and line (1-based) where `what` is written, or `None` when the package does not name it.
/// `text_of` reads a package file's text; a map's object is found in its `map.toml` through its table's span.
pub fn open_target(pkg: &Package, text_of: &dyn Fn(&str) -> Option<String>, map: &str, what: &Object) -> Option<(String, u32)> {
    match what {
        Object::Trigger(id) => pkg.triggers.iter().find(|t| &t.value.id == id).map(|t| (t.file.clone(), t.line)),
        Object::Dialog(id) => pkg.dialogs.iter().find(|d| &d.value.id == id).map(|d| (d.file.clone(), d.line)),
        Object::Unit(id) => pkg.units.iter().find(|u| &u.value.id == id).map(|u| (u.file.clone(), u.line)),
        Object::Item(id) => pkg.items.iter().find(|i| &i.value.id == id).map(|i| (i.file.clone(), i.line)),
        Object::Map(sel) => {
            let file = format!("maps/{map}/map.toml");
            let spec = pkg.maps.get(map)?.spec.as_ref()?;
            let at = locator_of(&file, spec, sel)?;
            let text = text_of(&file)?;
            let (_, range) = spans(&file, &text).ok()?.into_iter().find(|(l, _)| *l == at)?;
            Some((file, line_of(&text, range.start)))
        }
    }
}

/// Where a studio jump lands (Ctrl+P, a score line, an AI log entry): the workspace it opens and the object
/// in it. The UI turns it into the workspace's state (`ui/studio`); this is what it means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    /// Карта: the map, and an object of it or a cell (`object` is `None` for a cell).
    Map { map: String, object: Option<Selection>, cell: Option<(i64, i64)> },
    /// Сюжет: a trigger or a dialog node of the story graph.
    Story(StoryNode),
    /// Сюжет: a variable (the outline's Переменные tab).
    Var(String),
    /// Сюжет: a named character (the outline's Персонажи tab).
    Character(String),
    /// Данные: a unit or an item record of the spreadsheets.
    Unit(String),
    Item(String),
    /// Данные › Файлы: a package file, at a line (1-based).
    File { path: String, line: u32 },
}

/// A node of the story graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoryNode {
    Trigger(String),
    Dialog(String),
}

/// The workspace a destination opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Map,
    Story,
    Data,
}

impl Destination {
    /// The workspace the destination is in.
    pub fn area(&self) -> Area {
        match self {
            Destination::Map { .. } => Area::Map,
            Destination::Story(_) | Destination::Var(_) | Destination::Character(_) => Area::Story,
            Destination::Unit(_) | Destination::Item(_) | Destination::File { .. } => Area::Data,
        }
    }
}

/// The destination of a score line's target on `map` (the Оценка tab's jump).
pub fn of_target(map: &str, target: &crate::studio::scores::Target) -> Destination {
    use crate::studio::scores::Target;
    let object = |sel| Destination::Map { map: map.to_string(), object: Some(sel), cell: None };
    match target {
        Target::Hero => object(Selection::Hero),
        Target::Army(id) => object(Selection::Army(id.clone())),
        Target::Building(id) => object(Selection::Building(id.clone())),
        Target::Point(i) => object(Selection::Point(*i)),
        Target::Object(i) => object(Selection::Object(*i)),
        Target::Cell(x, y) => Destination::Map { map: map.to_string(), object: None, cell: Some((*x, *y)) },
        Target::Line { file, line } => Destination::File { path: file.clone(), line: *line },
    }
}

/// Whether `map` names an army with this id (the map's own table, not a spawn of the story).
fn map_has_army(pkg: &Package, map: &str, army: &str) -> bool {
    pkg.maps.get(map).and_then(|m| m.spec.as_ref()).is_some_and(|s| s.army.iter().any(|a| a.get_ref().id == army))
}

/// The destination of an AI log entry on `map`: the trigger that fired, the dialog, the variable, or the
/// army (when the map names it) or its cell. `None` for a moment with nothing on the map to show (the
/// travel to another map, the end of the story reached without a dialog).
pub fn of_moment(pkg: &Package, map: &str, moment: &Moment) -> Option<Destination> {
    let (what, cell) = (&moment.what, moment.cell);
    let army = |id: &str| -> Option<Destination> {
        if map_has_army(pkg, map, id) {
            return Some(Destination::Map { map: map.to_string(), object: Some(Selection::Army(id.to_string())), cell: None });
        }
        cell.map(|(x, y)| Destination::Map { map: map.to_string(), object: None, cell: Some((i64::from(x), i64::from(y))) })
    };
    match what {
        Trace::Fired { trigger, .. } => Some(Destination::Story(StoryNode::Trigger(trigger.clone()))),
        Trace::Dialog { id, .. } => Some(Destination::Story(StoryNode::Dialog(id.clone()))),
        Trace::Answer { dialog, .. } => Some(Destination::Story(StoryNode::Dialog(dialog.clone()))),
        Trace::Ending(dialog) => Some(Destination::Story(StoryNode::Dialog(dialog.clone()))),
        Trace::Var { name, .. } => Some(Destination::Var(name.clone())),
        Trace::Spawn { army: id } | Trace::Goal { army: id, .. } | Trace::Arrive { army: id, .. } => army(id),
        Trace::End(_) | Trace::Travel(_) => None,
    }
}

/// The kinds of the Ctrl+P list (the palette's group of each entry).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoKind {
    File,
    Map,
    Army,
    Building,
    Trigger,
    Dialog,
    Unit,
    Item,
    Character,
    Var,
}

/// One entry of the Ctrl+P list: its kind, its id (or path), its context (the map, the file's folder), and
/// where choosing it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoTo {
    pub kind: GoKind,
    pub id: String,
    pub sub: String,
    pub dest: Destination,
}

/// Every object and file of the package the Ctrl+P list offers: the files (`files`, the package's text
/// files), the maps, each map's armies and buildings, the triggers, dialogs, units, items, characters and
/// variables. Each entry resolves to a destination (a test checks it against the example packages).
pub fn goto_index(pkg: &Package, files: &[String]) -> Vec<GoTo> {
    let mut out = Vec::new();
    for path in files {
        out.push(GoTo { kind: GoKind::File, id: path.clone(), sub: String::new(), dest: Destination::File { path: path.clone(), line: 1 } });
    }
    for (id, entry) in &pkg.maps {
        out.push(GoTo { kind: GoKind::Map, id: id.clone(), sub: entry.file.clone(), dest: Destination::Map { map: id.clone(), object: None, cell: None } });
        if let Some(spec) = &entry.spec {
            for a in &spec.army {
                let id_a = a.get_ref().id.clone();
                let dest = Destination::Map { map: id.clone(), object: Some(Selection::Army(id_a.clone())), cell: None };
                out.push(GoTo { kind: GoKind::Army, id: id_a, sub: id.clone(), dest });
            }
            for b in &spec.building {
                let id_b = b.get_ref().id.clone();
                let dest = Destination::Map { map: id.clone(), object: Some(Selection::Building(id_b.clone())), cell: None };
                out.push(GoTo { kind: GoKind::Building, id: id_b, sub: id.clone(), dest });
            }
        }
    }
    for t in &pkg.triggers {
        let sub = t.value.map.clone().unwrap_or_default();
        out.push(GoTo { kind: GoKind::Trigger, id: t.value.id.clone(), sub, dest: Destination::Story(StoryNode::Trigger(t.value.id.clone())) });
    }
    for d in &pkg.dialogs {
        out.push(GoTo { kind: GoKind::Dialog, id: d.value.id.clone(), sub: d.file.clone(), dest: Destination::Story(StoryNode::Dialog(d.value.id.clone())) });
    }
    for u in &pkg.units {
        out.push(GoTo { kind: GoKind::Unit, id: u.value.id.clone(), sub: u.file.clone(), dest: Destination::Unit(u.value.id.clone()) });
    }
    for i in &pkg.items {
        out.push(GoTo { kind: GoKind::Item, id: i.value.id.clone(), sub: i.file.clone(), dest: Destination::Item(i.value.id.clone()) });
    }
    for c in super::characters::characters(pkg) {
        out.push(GoTo { kind: GoKind::Character, id: c.id.clone(), sub: String::new(), dest: Destination::Character(c.id.clone()) });
    }
    for v in &pkg.vars {
        out.push(GoTo { kind: GoKind::Var, id: v.value.name.clone(), sub: v.file.clone(), dest: Destination::Var(v.value.name.clone()) });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::load::Package;

    fn example() -> Package {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let files = crate::pack::files::Files::from_dir(&root).expect("the example package reads");
        Package::parse(files).0
    }

    #[test]
    fn the_outline_groups_the_story_by_map_with_the_when_as_the_subtitle() {
        let pkg = example();
        let groups = outline(&pkg, "");
        let total: usize = groups.iter().map(|g| g.rows.len()).sum();
        assert_eq!(total, pkg.triggers.len() + pkg.dialogs.len(), "every trigger and dialog is in the outline");
        let triggers = groups.iter().flat_map(|g| &g.rows).find(|r| r.kind == OutlineKind::Trigger).expect("a trigger row");
        assert!(!triggers.sub.is_empty(), "the trigger's event is its subtitle");
        // The triggers that apply to every map come first.
        if let Some(first) = groups.first() {
            assert!(first.map.is_empty() || groups.iter().all(|g| !g.map.is_empty()), "the every-map group leads");
        }
    }

    #[test]
    fn the_outline_search_keeps_the_matching_rows_and_drops_the_empty_groups() {
        let pkg = example();
        let all: usize = outline(&pkg, "").iter().map(|g| g.rows.len()).sum();
        let some = outline(&pkg, "ford");
        let rows: Vec<&OutlineRow> = some.iter().flat_map(|g| &g.rows).collect();
        assert!(!rows.is_empty() || all == 0, "the search finds the rows of a name");
        assert!(rows.iter().all(|r| {
            let q = "ford";
            r.id.to_lowercase().contains(q) || r.sub.to_lowercase().contains(q) || some.iter().any(|g| g.rows.contains(r) && g.map.to_lowercase().contains(q))
        }));
        assert!(some.iter().all(|g| !g.rows.is_empty()), "no empty group is kept");
        assert!(outline(&pkg, "zz-no-such-row-zz").is_empty());
    }

    #[test]
    fn open_in_file_resolves_a_trigger_a_unit_and_an_army() {
        let pkg = example();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let text_of = |f: &str| std::fs::read_to_string(root.join(f)).ok();
        let t = pkg.triggers.first().expect("a trigger");
        assert_eq!(open_target(&pkg, &text_of, "", &Object::Trigger(t.value.id.clone())), Some((t.file.clone(), t.line)));
        let u = pkg.units.first().expect("a unit");
        assert_eq!(open_target(&pkg, &text_of, "", &Object::Unit(u.value.id.clone())), Some((u.file.clone(), u.line)));
        let (map, army) = pkg
            .maps
            .iter()
            .find_map(|(m, mf)| mf.spec.as_ref().and_then(|s| s.army.first()).map(|a| (m.clone(), a.get_ref().id.clone())))
            .expect("a map with an army");
        let (file, line) = open_target(&pkg, &text_of, &map, &Object::Map(Selection::Army(army.clone()))).expect("the army's line");
        assert_eq!(file, format!("maps/{map}/map.toml"));
        let text = text_of(&file).expect("the map file");
        assert!(text.lines().nth(line as usize - 1).is_some_and(|l| l.contains("[[army]]")), "line {line} is the army's table: {:?}", text.lines().nth(line as usize - 1));
        assert!(open_target(&pkg, &text_of, &map, &Object::Map(Selection::Army("no-such-army".into()))).is_none());
    }
}

#[cfg(test)]
mod destination_tests {
    use super::*;
    use crate::pack::files::Files;
    use crate::pack::load::Package;
    use crate::pack::sim::Moment;
    use crate::rules::triggers::{Ending, Trace};
    use crate::studio::scores::Target;

    fn example(name: &str) -> Package {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs").join(name);
        Package::parse(Files::from_dir(&root).expect("the example package reads")).0
    }

    fn moment(what: Trace, cell: Option<(i32, i32)>) -> Moment {
        Moment { day: 1, text: String::new(), what, trigger: None, cell }
    }

    #[test]
    fn a_score_line_resolves_to_its_map_object_or_file() {
        assert_eq!(of_target("marka", &Target::Army("caravan".into())), Destination::Map { map: "marka".into(), object: Some(Selection::Army("caravan".into())), cell: None });
        assert_eq!(of_target("marka", &Target::Cell(3, 4)).area(), Area::Map);
        assert_eq!(of_target("marka", &Target::Cell(3, 4)), Destination::Map { map: "marka".into(), object: None, cell: Some((3, 4)) });
        let line = of_target("marka", &Target::Line { file: "triggers/marka.toml".into(), line: 9 });
        assert_eq!(line, Destination::File { path: "triggers/marka.toml".into(), line: 9 });
        assert_eq!(line.area(), Area::Data);
        assert_eq!(of_target("marka", &Target::Hero).area(), Area::Map);
    }

    #[test]
    fn an_ai_log_entry_of_a_fired_trigger_or_dialog_opens_the_story() {
        let pkg = example("pepelnaya-marka");
        let fired = moment(Trace::Fired { trigger: "m_diary_start".into(), moment: "hero_enters".into() }, None);
        let dest = of_moment(&pkg, "marka", &fired).expect("a trigger jump");
        assert_eq!(dest, Destination::Story(StoryNode::Trigger("m_diary_start".into())));
        assert_eq!(dest.area(), Area::Story);
        let answer = moment(Trace::Answer { dialog: "council".into(), choice: Some("order".into()) }, None);
        assert_eq!(of_moment(&pkg, "marka", &answer), Some(Destination::Story(StoryNode::Dialog("council".into()))));
        let var = moment(Trace::Var { name: "gold".into(), value: crate::pack::triggers::VarValue::Int(3) }, None);
        assert_eq!(of_moment(&pkg, "marka", &var), Some(Destination::Var("gold".into())));
    }

    #[test]
    fn an_ai_log_entry_of_an_army_opens_the_army_when_the_map_names_it() {
        let pkg = example("pepelnaya-marka");
        let caravan = moment(Trace::Arrive { army: "caravan".into(), cell: (58, 50), building: None }, Some((58, 50)));
        assert_eq!(of_moment(&pkg, "marka", &caravan), Some(Destination::Map { map: "marka".into(), object: Some(Selection::Army("caravan".into())), cell: None }));
        // A spawned army the map does not name opens its cell instead.
        let spawned = moment(Trace::Spawn { army: "reinforcements".into() }, Some((10, 12)));
        assert_eq!(of_moment(&pkg, "marka", &spawned), Some(Destination::Map { map: "marka".into(), object: None, cell: Some((10, 12)) }));
        // Nothing to show: a travel, and a moment with neither an army nor a cell.
        assert_eq!(of_moment(&pkg, "marka", &moment(Trace::Travel("brod".into()), None)), None);
        assert_eq!(of_moment(&pkg, "marka", &moment(Trace::End(Ending::Victory), None)), None);
    }

    #[test]
    fn the_ctrl_p_list_covers_every_kind_and_each_entry_resolves() {
        let pkg = example("pepelnaya-marka");
        let files: Vec<String> = vec!["package.toml".into(), "triggers/marka.toml".into()];
        let index = goto_index(&pkg, &files);
        let kinds = [GoKind::File, GoKind::Map, GoKind::Army, GoKind::Building, GoKind::Trigger, GoKind::Dialog, GoKind::Unit, GoKind::Item, GoKind::Character, GoKind::Var];
        for kind in kinds {
            assert!(index.iter().any(|g| g.kind == kind), "{kind:?} is in the list");
        }
        for g in &index {
            assert!(resolves(&pkg, &g.dest), "{:?} {} resolves to {:?}", g.kind, g.id, g.dest);
        }
    }

    /// Whether a destination names something the package has: its map, its army or building, its node,
    /// its variable, character, record or file.
    fn resolves(pkg: &Package, dest: &Destination) -> bool {
        match dest {
            Destination::Map { map, object, .. } => {
                let Some(spec) = pkg.maps.get(map).and_then(|m| m.spec.as_ref()) else { return false };
                match object {
                    None => true,
                    Some(Selection::Army(id)) => spec.army.iter().any(|a| &a.get_ref().id == id),
                    Some(Selection::Building(id)) => spec.building.iter().any(|b| &b.get_ref().id == id),
                    Some(Selection::Hero) => true,
                    Some(Selection::Point(i)) => *i < spec.point.len(),
                    Some(Selection::Object(i)) => *i < spec.object.len(),
                }
            }
            Destination::Story(StoryNode::Trigger(id)) => pkg.triggers.iter().any(|t| &t.value.id == id),
            Destination::Story(StoryNode::Dialog(id)) => pkg.dialogs.iter().any(|d| &d.value.id == id),
            Destination::Var(name) => pkg.vars.iter().any(|v| &v.value.name == name),
            Destination::Character(id) => super::super::characters::characters(pkg).iter().any(|c| &c.id == id),
            Destination::Unit(id) => pkg.units.iter().any(|u| &u.value.id == id),
            Destination::Item(id) => pkg.items.iter().any(|i| &i.value.id == id),
            Destination::File { path, line } => *line >= 1 && pkg.files.text(path).is_some(),
        }
    }

    #[test]
    fn the_ctrl_p_list_resolves_on_the_hello_story_example_too() {
        let pkg = example("hello-story");
        for g in goto_index(&pkg, &[]) {
            assert!(resolves(&pkg, &g.dest), "{:?} {} resolves", g.kind, g.id);
        }
    }
}

