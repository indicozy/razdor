//! Format-preserving edits of any package TOML file: the package settings, the map header, the
//! campaign graph, the named characters' rename and the texts. The same `toml_edit` path as the
//! map tables (`studio::edit`): only the touched value or table entry changes, so the comments,
//! blank lines and key order survive. A table is reached by its path of [`Hop`]s from the root:
//! a plain `[table]` by its key, an entry of an array of tables by its index or by its `id`.
//!
//! The edits do not check the package's rules; the forms check their own input (a date, a
//! relation, a version), and `pack::check` reads the result after every change.

use toml_edit::{ArrayOfTables, Document, DocumentMut, Item, Table, TomlError, Value};

use super::edit::array_value;

/// How a [`Hop`] picks its table among the entries of an array of tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// A plain `[table]` under the key (no index).
    Table,
    /// The entry at this index of the array of tables (file order).
    Index(usize),
    /// The entry whose `id` is this text (an array of tables with ids).
    Id(String),
}

/// One step of a path: a key, and which table under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hop {
    pub key: String,
    pub pick: Pick,
}

/// A plain table under `key`.
pub fn table(key: &str) -> Hop {
    Hop { key: key.to_string(), pick: Pick::Table }
}

/// The entry `index` of the array of tables under `key`.
pub fn entry(key: &str, index: usize) -> Hop {
    Hop { key: key.to_string(), pick: Pick::Index(index) }
}

/// The entry with `id` of the array of tables under `key`.
pub fn entry_id(key: &str, id: &str) -> Hop {
    Hop { key: key.to_string(), pick: Pick::Id(id.to_string()) }
}

/// One change to a TOML file. Each names the table it changes by its path from the root (an empty
/// path is the root table).
#[derive(Clone, Debug)]
pub enum DocEdit {
    /// Sets `key` of the table at `path` to `value`. A missing table on the path is made (a
    /// `[table]` header); a missing key is added at the table's end.
    Set { path: Vec<Hop>, key: String, value: Value },
    /// Removes `key` of the table at `path` (its line; the comment lines above it stay).
    Remove { path: Vec<Hop>, key: String },
    /// Appends an entry to the array of tables `key` at `path` (created when missing), with these
    /// keys.
    Push { path: Vec<Hop>, key: String, fields: Vec<(String, Value)> },
    /// Removes the entry `pick` of the array of tables `key` at `path`, with its comments.
    Drop { path: Vec<Hop>, key: String, pick: Pick },
    /// Moves the entry `from` of the array of tables `key` at `path` to index `to` (the entries
    /// between shift by one).
    Move { path: Vec<Hop>, key: String, from: usize, to: usize },
}

impl DocEdit {
    /// The undo label of the edit: `set end of brod`.
    pub fn label(&self) -> String {
        match self {
            DocEdit::Set { key, .. } => format!("set {key}"),
            DocEdit::Remove { key, .. } => format!("remove {key}"),
            DocEdit::Push { key, .. } => format!("add {key}"),
            DocEdit::Drop { key, .. } => format!("delete {key}"),
            DocEdit::Move { key, .. } => format!("move {key}"),
        }
    }
}

/// The text with `edits` applied in order. An error (a path that is not there, a text that does not
/// parse) leaves nothing changed: the caller keeps its text.
pub fn apply_all(text: &str, edits: &[DocEdit]) -> Result<String, String> {
    let mut out = text.to_string();
    for edit in edits {
        out = apply_text(&out, edit)?;
    }
    Ok(out)
}

/// The text with one edit applied.
pub fn apply_text(text: &str, edit: &DocEdit) -> Result<String, String> {
    if let DocEdit::Remove { path, key } = edit {
        return remove_key_lines(text, path, key);
    }
    let mut doc: DocumentMut = text.parse().map_err(|e: TomlError| format!("line {}: {}", line_of_error(text, &e), e.message()))?;
    match edit {
        DocEdit::Set { path, key, value } => {
            let t = walk_create(doc.as_table_mut(), path)?;
            put(t, key, value.clone());
        }
        DocEdit::Push { path, key, fields } => {
            let t = walk_create(doc.as_table_mut(), path)?;
            let mut row = Table::new();
            for (k, v) in fields {
                row.insert(k, Item::Value(v.clone()));
            }
            match t.get_mut(key) {
                Some(Item::ArrayOfTables(a)) => a.push(row),
                Some(_) => return Err(format!("`{key}` is not an array of tables")),
                None => {
                    let mut a = ArrayOfTables::new();
                    a.push(row);
                    t.insert(key, Item::ArrayOfTables(a));
                }
            }
        }
        DocEdit::Drop { path, key, pick } => {
            let t = walk(doc.as_table_mut(), path)?;
            let Some(Item::ArrayOfTables(a)) = t.get_mut(key) else {
                return Err(format!("no `{key}` entries to delete"));
            };
            let i = pick_index(a, key, pick)?;
            a.remove(i);
        }
        DocEdit::Move { path, key, from, to } => {
            let t = walk(doc.as_table_mut(), path)?;
            let Some(Item::ArrayOfTables(a)) = t.get_mut(key) else {
                return Err(format!("no `{key}` entries to move"));
            };
            if *from >= a.len() || *to >= a.len() {
                return Err(format!("`{key}` has {} entries: cannot move {} to {}", a.len(), from + 1, to + 1));
            }
            let row = a.remove(*from);
            a.insert(*to, row);
            // toml_edit orders the entries by their stored positions: the moved order is only kept
            // when the entries have none (they then come out in the array's order).
            for t in a.iter_mut() {
                t.set_position(None);
            }
        }
        DocEdit::Remove { .. } => unreachable!("handled above"),
    }
    Ok(doc.to_string())
}

/// The index of the entry `pick` in `a`.
fn pick_index(a: &ArrayOfTables, key: &str, pick: &Pick) -> Result<usize, String> {
    match pick {
        Pick::Table => Err(format!("`{key}` is an array of tables: name an entry")),
        Pick::Index(i) if *i < a.len() => Ok(*i),
        Pick::Index(i) => Err(format!("no `{key}` entry {}", i + 1)),
        Pick::Id(id) => a
            .iter()
            .position(|t| t.get("id").and_then(Item::as_str) == Some(id.as_str()))
            .ok_or_else(|| format!("no `{key}` with id \"{id}\"")),
    }
}

/// The table at `path`, every table on it existing. A missing step is an error.
fn walk<'a>(root: &'a mut Table, path: &[Hop]) -> Result<&'a mut Table, String> {
    let mut cur = root;
    for hop in path {
        let item = cur.get_mut(&hop.key).ok_or_else(|| format!("no `{}` in the file", hop.key))?;
        cur = match (item, &hop.pick) {
            (Item::Table(t), Pick::Table) => t,
            (Item::ArrayOfTables(a), pick) => {
                let i = pick_index(a, &hop.key, pick)?;
                a.get_mut(i).ok_or_else(|| format!("no `{}` entry {}", hop.key, i + 1))?
            }
            _ => return Err(format!("`{}` is not a table", hop.key)),
        };
    }
    Ok(cur)
}

/// The table at `path`, made where it is missing (a plain table under a key; an entry of an
/// array of tables must exist).
fn walk_create<'a>(root: &'a mut Table, path: &[Hop]) -> Result<&'a mut Table, String> {
    let mut cur = root;
    for (n, hop) in path.iter().enumerate() {
        if hop.pick == Pick::Table && cur.get(&hop.key).is_none() {
            let mut t = Table::new();
            // Only the table the edit writes into is a header; the ones above it are implicit.
            t.set_implicit(n + 1 < path.len());
            cur.insert(&hop.key, Item::Table(t));
        }
        let item = cur.get_mut(&hop.key).ok_or_else(|| format!("no `{}` in the file", hop.key))?;
        cur = match (item, &hop.pick) {
            (Item::Table(t), Pick::Table) => t,
            (Item::ArrayOfTables(a), pick) => {
                let i = pick_index(a, &hop.key, pick)?;
                a.get_mut(i).ok_or_else(|| format!("no `{}` entry {}", hop.key, i + 1))?
            }
            _ => return Err(format!("`{}` is not a table", hop.key)),
        };
    }
    Ok(cur)
}

/// Sets `key` of `t` to `new`, keeping the old value's layout (its spacing and trailing comment),
/// or adds the key at the end of the table.
fn put(t: &mut Table, key: &str, mut new: Value) {
    match t.get_mut(key) {
        Some(item) => {
            if let Some(old) = item.as_value() {
                *new.decor_mut() = old.decor().clone();
            }
            *item = Item::Value(new);
        }
        None => {
            t.insert(key, Item::Value(new));
        }
    }
}

/// The text without the lines of `key` in the table at `path` (the comment lines above it stay).
fn remove_key_lines(text: &str, path: &[Hop], key: &str) -> Result<String, String> {
    let doc = Document::parse(text).map_err(|e| format!("line {}: {}", line_of_error(text, &e), e.message()))?;
    let t = walk_ref(doc.as_table(), path)?;
    let (k, item) = t.get_key_value(key).ok_or_else(|| format!("no `{key}` to remove"))?;
    let (Some(ks), Some(is)) = (k.span(), item.span()) else {
        return Err(format!("`{key}` has no place in the file"));
    };
    let start = text[..ks.start].rfind('\n').map_or(0, |i| i + 1);
    let end = text[is.end.max(ks.end).min(text.len())..].find('\n').map_or(text.len(), |i| is.end.max(ks.end) + i + 1);
    Ok(format!("{}{}", &text[..start], &text[end..]))
}

/// The table at `path` of a parsed file (the read-only twin of [`walk`]).
fn walk_ref<'a>(root: &'a Table, path: &[Hop]) -> Result<&'a Table, String> {
    let mut cur = root;
    for hop in path {
        let item = cur.get(&hop.key).ok_or_else(|| format!("no `{}` in the file", hop.key))?;
        cur = match (item, &hop.pick) {
            (Item::Table(t), Pick::Table) => t,
            (Item::ArrayOfTables(a), pick) => {
                let i = pick_index(a, &hop.key, pick)?;
                a.get(i).ok_or_else(|| format!("no `{}` entry {}", hop.key, i + 1))?
            }
            _ => return Err(format!("`{}` is not a table", hop.key)),
        };
    }
    Ok(cur)
}

/// The 1-based line where a TOML error starts.
fn line_of_error(text: &str, e: &TomlError) -> u32 {
    e.span().map_or(1, |s| crate::pack::load::line_of(text, s.start))
}

/// A TOML array of strings.
pub fn str_array(items: &[String]) -> Value {
    array_value(items.iter().map(|s| Value::from(s.as_str())).collect(), false)
}

/// A TOML array of integers.
pub fn int_array(items: &[i64]) -> Value {
    array_value(items.iter().map(|n| Value::from(*n)).collect(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# the package\nid = \"x\"\ntitle = \"x.title\" # shown\n\n# the carry\n[carry]\nhero = true\n\n# first\n[[map]]\nid = \"a\"\n[[map.next]]\nto = \"b\"\n\n[[map]]\nid = \"b\"\n";

    /// The text without its blank lines (toml_edit puts one before a new entry).
    fn squash(s: &str) -> String {
        s.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\n") + "\n"
    }

    #[test]
    fn set_keeps_the_comments_and_the_trailing_comment() {
        let out = apply_text(SAMPLE, &DocEdit::Set { path: vec![], key: "title".into(), value: Value::from("y.title") }).unwrap();
        assert_eq!(out, SAMPLE.replace("title = \"x.title\" # shown", "title = \"y.title\" # shown"));
    }

    #[test]
    fn set_creates_a_missing_table_and_adds_a_key() {
        let out = apply_text(SAMPLE, &DocEdit::Set { path: vec![table("terrain_palette")], key: "grass_plain".into(), value: Value::from("#60a040") }).unwrap();
        assert!(out.contains("[terrain_palette]\ngrass_plain = \"#60a040\""), "{out}");
        assert!(out.starts_with(&SAMPLE[..SAMPLE.find("[carry]").unwrap()]), "the start of the file is kept");
    }

    #[test]
    fn set_reaches_an_entry_of_an_array_by_id_and_by_index() {
        let out = apply_text(SAMPLE, &DocEdit::Set { path: vec![entry_id("map", "b")], key: "revisit".into(), value: Value::from(true) }).unwrap();
        assert!(out.ends_with("[[map]]\nid = \"b\"\nrevisit = true\n"), "{out}");
        let out = apply_text(&SAMPLE.replace("[[map]]\nid = \"a\"\n", "[[map]]\nid = \"a\"\n[[map.next]]\nto = \"b\"\n"), &DocEdit::Set { path: vec![entry("map", 0), entry("next", 0)], key: "to".into(), value: Value::from("c") }).unwrap();
        assert!(out.contains("to = \"c\""), "{out}");
    }

    #[test]
    fn push_appends_an_entry_after_the_entries_of_its_table() {
        let fields = vec![("to".to_string(), Value::from("c"))];
        let out = squash(&apply_text(SAMPLE, &DocEdit::Push { path: vec![entry_id("map", "a")], key: "next".into(), fields }).unwrap());
        assert!(out.contains("[[map.next]]\nto = \"b\"\n[[map.next]]\nto = \"c\"\n"), "{out}");
        assert!(out.contains("[[map]]\nid = \"b\""), "the next map is still there: {out}");
    }

    #[test]
    fn drop_and_move_reorder_and_delete_entries() {
        let two = "[[map]]\nid = \"a\"\n[[map.next]]\nto = \"b\"\n[[map.next]]\nto = \"c\"\n";
        let moved = squash(&apply_text(two, &DocEdit::Move { path: vec![entry_id("map", "a")], key: "next".into(), from: 1, to: 0 }).unwrap());
        assert!(moved.contains("[[map.next]]\nto = \"c\"\n[[map.next]]\nto = \"b\""), "{moved}");
        let dropped = squash(&apply_text(two, &DocEdit::Drop { path: vec![entry_id("map", "a")], key: "next".into(), pick: Pick::Index(0) }).unwrap());
        assert!(!dropped.contains("to = \"b\""), "{dropped}");
        assert!(dropped.contains("to = \"c\""));
        assert!(apply_text(two, &DocEdit::Drop { path: vec![entry_id("map", "a")], key: "next".into(), pick: Pick::Index(5) }).is_err());
    }

    #[test]
    fn remove_takes_the_key_line_and_keeps_the_comment_above_it() {
        let out = apply_text(SAMPLE, &DocEdit::Remove { path: vec![], key: "title".into() }).unwrap();
        assert!(!out.contains("title ="), "{out}");
        assert!(out.contains("# the package\nid = \"x\"\n"), "{out}");
        let out = apply_text(SAMPLE, &DocEdit::Remove { path: vec![table("carry")], key: "hero".into() }).unwrap();
        assert!(out.contains("# the carry\n[carry]\n"), "{out}");
    }

    #[test]
    fn a_text_that_does_not_parse_is_refused() {
        let broken = "[[map]\n";
        assert!(apply_text(broken, &DocEdit::Set { path: vec![], key: "x".into(), value: Value::from(1_i64) }).is_err());
    }

    #[test]
    fn apply_all_runs_in_order_and_labels_name_the_key() {
        let edits = [
            DocEdit::Set { path: vec![], key: "title".into(), value: Value::from("q") },
            DocEdit::Set { path: vec![table("carry")], key: "gold".into(), value: Value::from(false) },
        ];
        let out = apply_all(SAMPLE, &edits).unwrap();
        assert!(out.contains("title = \"q\"") && out.contains("gold = false"), "{out}");
        assert_eq!(edits[0].label(), "set title");
    }
}
