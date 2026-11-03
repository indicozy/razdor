//! The named characters of a package (P3): each `named = "<id>"` a `give_unit` gives (in a trigger or
//! a dialog choice), where it is given, where it is required (`has_named_unit`, `to`/`in` =
//! `"named:<id>"`), where it is removed (`remove_unit`); the warning when a character is required on
//! a map that comes before every map that gives it; and the rename across all the files that name
//! it (the triggers, the dialogs and the names' text keys `named.<id>`, `<map>.named.<id>`).
//!
//! The scan reads the files' text with `toml_edit`, so each site carries its own line. The order
//! of the maps is the campaign's (`campaign_graph::order`).

use std::collections::BTreeMap;

use toml_edit::{Document, Item, Table, Value};

use crate::pack::load::{line_of, Package};
use crate::pack::triggers::Action;

/// What a site does with the character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteKind {
    /// `give_unit … named = "<id>"`: the character joins the hero's army.
    Give,
    /// Needed: `has_named_unit = "<id>"`, or a holder `"named:<id>"` (an item or XP goes to him).
    Need,
    /// `remove_unit = "<id>"`: the character leaves his holder.
    Remove,
}

/// One place that names a character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub file: String,
    pub line: u32,
    /// The map the trigger applies to (`None`: every map, or a dialog no map opens).
    pub map: Option<String>,
    /// `trigger <id>` or `dialog <id>, choice <id>`.
    pub by: String,
    pub kind: SiteKind,
}

/// A named character: its id and every place that names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Character {
    pub id: String,
    pub gives: Vec<Site>,
    pub needs: Vec<Site>,
    pub removes: Vec<Site>,
}

/// A character required where it may not be given yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub id: String,
    pub file: String,
    pub line: u32,
    pub message: String,
}

/// Every character the package names, sorted by id.
pub fn characters(pkg: &Package) -> Vec<Character> {
    let openers = openers(pkg);
    let mut by_id: BTreeMap<String, Character> = BTreeMap::new();
    for (id, site) in named_sites(pkg, &openers) {
        let c = by_id.entry(id.clone()).or_insert_with(|| Character { id, gives: Vec::new(), needs: Vec::new(), removes: Vec::new() });
        match site.kind {
            SiteKind::Give => c.gives.push(site),
            SiteKind::Need => c.needs.push(site),
            SiteKind::Remove => c.removes.push(site),
        }
    }
    by_id.into_values().collect()
}

/// Whether a package file is read for the named characters: the triggers and the dialogs.
fn is_story_file(path: &str) -> bool {
    (path.starts_with("triggers/") || path.starts_with("dialogs/")) && path.ends_with(".toml")
}

/// The maps that open each dialog: a trigger's `dialog = "<id>"` gives its trigger's map (`None`:
/// every map).
fn openers(pkg: &Package) -> BTreeMap<String, Vec<Option<String>>> {
    let mut out: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
    for t in &pkg.triggers {
        for a in &t.value.then {
            if let Action::Dialog(d) = a {
                out.entry(d.clone()).or_default().push(t.value.map.clone());
            }
        }
    }
    out
}

/// One named character's site as the scan found it.
type Named = (String, Site);

/// The sites of every named character in the story files, with the character's id.
fn named_sites(pkg: &Package, openers: &BTreeMap<String, Vec<Option<String>>>) -> Vec<Named> {
    let mut out = Vec::new();
    for file in pkg.files.paths().filter(|p| is_story_file(p)).map(str::to_string).collect::<Vec<_>>() {
        let Some(text) = pkg.files.text(&file) else { continue };
        let Ok(doc) = Document::parse(text) else { continue };
        for (key, item) in doc.as_table().iter() {
            let Item::ArrayOfTables(entries) = item else { continue };
            for entry in entries.iter() {
                let id = entry.get("id").and_then(Item::as_str).unwrap_or("?").to_string();
                let (by, map) = match key {
                    "trigger" => (format!("trigger {id}"), entry.get("map").and_then(Item::as_str).map(str::to_string)),
                    "dialog" => {
                        let maps = openers.get(&id).cloned().unwrap_or_default();
                        (format!("dialog {id}"), if maps.len() == 1 { maps[0].clone() } else { None })
                    }
                    _ => continue,
                };
                // A dialog's own keys, then each choice with its own label (`dialog d, choice c`).
                let mut found = Vec::new();
                for (k, item) in entry.iter() {
                    if !(key == "dialog" && k == "choice") {
                        walk_item(item, &mut |kind, name, at| found.push((by.clone(), name, kind, at)));
                    }
                }
                if let Some(Item::ArrayOfTables(choices)) = entry.get("choice") {
                    for c in choices.iter() {
                        let cid = c.get("id").and_then(Item::as_str).unwrap_or("?");
                        walk_table(c, &mut |kind, name, at| found.push((format!("{by}, choice {cid}"), name, kind, at)));
                    }
                }
                for (label, name, kind, at) in found {
                    let site = Site { file: file.clone(), line: line_of(text, at), map: map.clone(), by: label, kind };
                    out.push((name, site));
                }
            }
        }
    }
    out
}

/// Calls `f` for each place a named character is named under `table`: the character's id, what
/// names it and the byte offset of the name's value.
fn walk_table(table: &Table, f: &mut dyn FnMut(SiteKind, String, usize)) {
    for (_, item) in table.iter() {
        walk_item(item, f);
    }
}

fn walk_item(item: &Item, f: &mut dyn FnMut(SiteKind, String, usize)) {
    match item {
        Item::Value(v) => walk_value(v, f),
        Item::Table(t) => walk_table(t, f),
        Item::ArrayOfTables(a) => a.iter().for_each(|t| walk_table(t, f)),
        Item::None => {}
    }
}

/// The inline tables are the actions and conditions: each names a character by one of its keys.
fn walk_value(v: &Value, f: &mut dyn FnMut(SiteKind, String, usize)) {
    match v {
        Value::Array(a) => a.iter().for_each(|x| walk_value(x, f)),
        Value::InlineTable(t) => {
            let at = |key: &str| t.get(key).and_then(Value::span).map_or(0, |s| s.start);
            if t.get("give_unit").is_some() {
                if let Some(n) = t.get("named").and_then(Value::as_str) {
                    f(SiteKind::Give, n.to_string(), at("named"));
                }
            }
            if let Some(n) = t.get("has_named_unit").and_then(Value::as_str) {
                f(SiteKind::Need, n.to_string(), at("has_named_unit"));
            }
            if let Some(n) = t.get("remove_unit").and_then(Value::as_str) {
                f(SiteKind::Remove, n.to_string(), at("remove_unit"));
            }
            for (key, value) in t.iter() {
                if let Some(n) = value.as_str().and_then(|s| s.strip_prefix("named:")) {
                    f(SiteKind::Need, n.to_string(), value.span().map_or(0, |s| s.start));
                    let _ = key;
                }
            }
            for (_, value) in t.iter() {
                if !matches!(value, Value::String(_)) {
                    walk_value(value, f);
                }
            }
        }
        _ => {}
    }
}

/// The warnings: a character required on a map that comes before every map that gives it (in the
/// campaign's order), or required and never given. A give whose trigger applies to every map is
/// not an order to check against.
pub fn warnings(chars: &[Character], order: &[String]) -> Vec<Warning> {
    let pos = |m: &str| order.iter().position(|x| x == m);
    let mut out = Vec::new();
    for c in chars {
        let first_give = c.gives.iter().filter_map(|s| s.map.as_deref().and_then(pos).map(|p| (p, s))).min_by_key(|(p, _)| *p);
        for need in &c.needs {
            let Some(here) = need.map.as_deref().and_then(pos) else { continue };
            match first_give {
                Some((gp, give)) if here < gp => out.push(Warning {
                    id: c.id.clone(),
                    file: need.file.clone(),
                    line: need.line,
                    message: format!(
                        "`{}` is required on {} ({}), but its first give is on {} ({}:{}): the character may not be there yet",
                        c.id,
                        need.map.as_deref().unwrap_or("?"),
                        need.by,
                        give.map.as_deref().unwrap_or("?"),
                        give.file,
                        give.line
                    ),
                }),
                Some(_) => {}
                None if c.gives.is_empty() => out.push(Warning {
                    id: c.id.clone(),
                    file: need.file.clone(),
                    line: need.line,
                    message: format!("`{}` is required ({}) but no give_unit names it", c.id, need.by),
                }),
                None => {}
            }
        }
    }
    out
}

/// Whether `id` is a valid new character id (`[a-z0-9_]+`).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The text of a story file with the character renamed: its `named` keys, its `has_named_unit` and
/// `remove_unit` values and its `named:` holders. `None` when nothing in it names `old`.
pub fn rename_in_story(text: &str, old: &str, new: &str) -> Result<Option<String>, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let mut changed = false;
    let holder = format!("named:{old}");
    let holder_new = format!("named:{new}");
    fn visit_table(t: &mut Table, old: &str, new: &str, holder: &str, holder_new: &str, changed: &mut bool) {
        for (_, item) in t.iter_mut() {
            visit_item(item, old, new, holder, holder_new, changed);
        }
    }
    fn visit_item(item: &mut Item, old: &str, new: &str, holder: &str, holder_new: &str, changed: &mut bool) {
        match item {
            Item::Value(v) => visit_value(v, old, new, holder, holder_new, changed),
            Item::Table(t) => visit_table(t, old, new, holder, holder_new, changed),
            Item::ArrayOfTables(a) => a.iter_mut().for_each(|t| visit_table(t, old, new, holder, holder_new, changed)),
            Item::None => {}
        }
    }
    fn set(v: &mut Value, to: &str) {
        let decor = v.decor().clone();
        *v = Value::from(to);
        *v.decor_mut() = decor;
    }
    fn visit_value(v: &mut Value, old: &str, new: &str, holder: &str, holder_new: &str, changed: &mut bool) {
        match v {
            Value::Array(a) => a.iter_mut().for_each(|x| visit_value(x, old, new, holder, holder_new, changed)),
            Value::InlineTable(t) => {
                let gives = t.get("give_unit").is_some();
                for (key, value) in t.iter_mut() {
                    let key = key.get().to_string();
                    let Some(s) = value.as_str().map(str::to_string) else {
                        visit_value(value, old, new, holder, holder_new, changed);
                        continue;
                    };
                    let rename = match key.as_str() {
                        "named" if gives && s == old => Some(new.to_string()),
                        "has_named_unit" | "remove_unit" if s == old => Some(new.to_string()),
                        _ if s == holder => Some(holder_new.to_string()),
                        _ => None,
                    };
                    if let Some(to) = rename {
                        set(value, &to);
                        *changed = true;
                    }
                }
            }
            _ => {}
        }
    }
    visit_table(doc.as_table_mut(), old, new, &holder, &holder_new, &mut changed);
    Ok(changed.then(|| doc.to_string()))
}

/// The text of a `texts/<lang>.toml` with the names' keys renamed: `named.<old>` at the root and
/// `<map>.named.<old>` under each map of `maps`. `None` when there is no such key.
pub fn rename_in_texts(text: &str, old: &str, new: &str, maps: &[String]) -> Result<Option<String>, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let mut changed = false;
    let mut groups: Vec<Vec<&str>> = vec![vec!["named"]];
    for m in maps {
        groups.push(vec![m.as_str(), "named"]);
    }
    for group in groups {
        let mut cur: Option<&mut Table> = Some(doc.as_table_mut());
        for seg in &group {
            cur = match cur.and_then(|t| t.get_mut(seg)).and_then(Item::as_table_mut) {
                Some(t) => Some(t),
                None => None,
            };
        }
        if let Some(t) = cur {
            if let Some(item) = t.remove(old) {
                t.insert(new, item);
                changed = true;
            }
        }
    }
    Ok(changed.then(|| doc.to_string()))
}

/// The files a rename changes, as `(path, new text)`: the story files that name `old`, and the
/// texts that have its name keys. `text_of` gives a file's current text (`None` when it is missing).
/// Nothing is changed when `new` is not a valid id, is already a character, or is `old`.
pub fn rename_plan(pkg: &Package, old: &str, new: &str, text_of: &dyn Fn(&str) -> Option<String>) -> Result<Vec<(String, String)>, String> {
    if !valid_id(new) {
        return Err(format!("`{new}` is not an id: use lowercase letters, digits and _ only"));
    }
    if old == new {
        return Err("the new id is the same".into());
    }
    if characters(pkg).iter().any(|c| c.id == new) {
        return Err(format!("a character `{new}` already exists"));
    }
    let maps: Vec<String> = pkg.maps.keys().cloned().collect();
    let mut plan = Vec::new();
    let mut files: Vec<String> = pkg.files.paths().filter(|p| is_story_file(p)).map(str::to_string).collect();
    files.sort();
    for file in files {
        let Some(text) = text_of(&file) else { continue };
        if let Some(next) = rename_in_story(&text, old, new)? {
            plan.push((file, next));
        }
    }
    let mut langs: Vec<&String> = pkg.texts.keys().collect();
    langs.sort();
    for lang in langs {
        let file = format!("texts/{lang}.toml");
        let Some(text) = text_of(&file) else { continue };
        if let Some(next) = rename_in_texts(&text, old, new, &maps)? {
            plan.push((file, next));
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::files::Files;

    fn package() -> Package {
        let mut files = Files::default();
        files.insert("package.toml", "id = \"x\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"t\"\nauthor = \"a\"\nlicense = \"MIT\"\n");
        files.insert("texts/en.toml", "t = \"T\"\n\n[named]\nilm = \"Brother Ilm\"\n\n[brod.named]\nilm = \"Ilm of the ford\"\n");
        files.insert("maps/brod/map.toml", "title = \"t\"\n");
        files.insert("maps/marka/map.toml", "title = \"t\"\n");
        files.insert(
            "triggers/a.toml",
            "[[trigger]]\nid = \"meet\"\nmap = \"brod\"\nwhen = { event = \"day_start\" }\nif = [ { has_named_unit = \"ilm\", unit = \"dt:pilgrim\" } ]\nthen = [ { give_item = \"charm\", to = \"named:ilm\" } ]\n\n[[trigger]]\nid = \"join\"\nmap = \"marka\"\nwhen = { event = \"day_start\" }\nthen = [ { give_unit = \"dt:pilgrim\", named = \"ilm\" } ]\n",
        );
        files.insert("dialogs/a.toml", "[[dialog]]\nid = \"d\"\ntitle = \"t\"\ntext = \"t\"\n\n[[dialog.choice]]\nid = \"c\"\ntext = \"t\"\nthen = [ { remove_unit = \"ilm\" } ]\n");
        files.insert("triggers/b.toml", "[[trigger]]\nid = \"open\"\nmap = \"brod\"\nwhen = { event = \"day_start\" }\nthen = [ { dialog = \"d\" } ]\n");
        let (pkg, _) = Package::parse(files);
        pkg
    }

    #[test]
    fn the_sites_say_where_a_character_is_given_required_and_removed() {
        let chars = characters(&package());
        let ilm = chars.iter().find(|c| c.id == "ilm").expect("ilm is named");
        assert_eq!(ilm.gives.len(), 1);
        assert_eq!(ilm.gives[0].file, "triggers/a.toml");
        assert_eq!(ilm.gives[0].map.as_deref(), Some("marka"));
        assert_eq!(ilm.gives[0].line, 12, "the line of the named value");
        assert_eq!(ilm.needs.len(), 2, "has_named_unit and the named: holder");
        assert_eq!(ilm.removes.len(), 1);
        assert_eq!(ilm.removes[0].by, "dialog d, choice c");
    }

    #[test]
    fn a_character_required_before_it_is_given_is_warned() {
        let pkg = package();
        let chars = characters(&pkg);
        let order = vec!["brod".to_string(), "marka".to_string()];
        let w = warnings(&chars, &order);
        assert!(w.iter().any(|w| w.message.contains("required on brod")), "{w:?}");
        let fine = warnings(&chars, &["marka".to_string(), "brod".to_string()]);
        assert!(fine.iter().all(|w| !w.message.contains("required on brod")), "a give before the need: {fine:?}");
    }

    #[test]
    fn a_rename_changes_every_file_that_names_the_character() {
        let pkg = package();
        let texts = |f: &str| pkg.files.text(f).map(str::to_string);
        let plan = rename_plan(&pkg, "ilm", "brother_ilm", &texts).unwrap();
        let files: Vec<&str> = plan.iter().map(|(f, _)| f.as_str()).collect();
        assert!(files.contains(&"triggers/a.toml") && files.contains(&"dialogs/a.toml") && files.contains(&"texts/en.toml"), "{files:?}");
        let (_, a) = plan.iter().find(|(f, _)| f == "triggers/a.toml").unwrap();
        assert!(a.contains("has_named_unit = \"brother_ilm\"") && a.contains("named:brother_ilm") && a.contains("named = \"brother_ilm\""), "{a}");
        let (_, t) = plan.iter().find(|(f, _)| f == "texts/en.toml").unwrap();
        assert!(t.contains("brother_ilm = \"Brother Ilm\"") && t.contains("brother_ilm = \"Ilm of the ford\""), "{t}");
        assert!(!a.contains("\"ilm\""), "{a}");
    }

    #[test]
    fn a_rename_refuses_a_bad_or_taken_id() {
        let pkg = package();
        let texts = |f: &str| pkg.files.text(f).map(str::to_string);
        assert!(rename_plan(&pkg, "ilm", "Brother", &texts).is_err());
        assert!(rename_plan(&pkg, "ilm", "ilm", &texts).is_err());
        let mut files = Files::default();
        files.insert("package.toml", "id = \"x\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"t\"\nauthor = \"a\"\nlicense = \"MIT\"\n");
        files.insert("triggers/a.toml", "[[trigger]]\nid = \"j\"\nwhen = { event = \"day_start\" }\nthen = [ { give_unit = \"dt:pilgrim\", named = \"tess\" }, { give_unit = \"dt:pilgrim\", named = \"ilm\" } ]\n");
        let (pkg2, _) = Package::parse(files);
        let texts2 = |f: &str| pkg2.files.text(f).map(str::to_string);
        assert!(rename_plan(&pkg2, "ilm", "tess", &texts2).is_err(), "tess already exists");
    }

    #[test]
    fn a_rename_keeps_the_comments_of_the_file() {
        let text = "# the join\n[[trigger]]\nid = \"join\"\nthen = [ { give_unit = \"dt:pilgrim\", named = \"ilm\" } ] # once\n";
        let out = rename_in_story(text, "ilm", "ivo").unwrap().unwrap();
        assert_eq!(out, text.replace("named = \"ilm\"", "named = \"ivo\""));
        assert!(rename_in_story(text, "nobody", "x").unwrap().is_none());
    }
}
