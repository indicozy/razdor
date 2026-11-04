//! Navigation of the Сюжет and Данные workspaces (UX5): the story outline (the triggers and dialogs
//! grouped by map, with a search) and «Открыть в файле» (the file and line where an object is written).
//! Both read the parsed package and the files' text; neither changes anything.

use crate::pack::load::{line_of, Package};

use super::edit::spans;
use super::map::{locator_of, Selection};
use super::story_graph::event_text;

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
