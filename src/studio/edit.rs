//! Format-preserving edits of a map file (`maps/<id>/map.toml`) and the byte spans of its tables.
//!
//! The studio writes a map by changing one key of one table in the file's `toml_edit` document
//! and printing the document back: comments, blank lines, key order, inline-table layout and the
//! `# --- generated … ---` blocks survive, because only the touched value is replaced. A table is
//! found by its `id` when it has one, else by its index among the tables of its kind (the order
//! of the file). Keys the format does not have are refused: the allow-lists are the format's
//! (`docs/reference/rzpack-format.md`, «Maps»).

use std::ops::Range;

use toml_edit::{Array, Document, DocumentMut, Item, Table, TomlError, Value};

/// The kinds of table a map file holds that the studio edits or selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// `[hero]`: one table.
    Hero,
    /// `[[building]]`.
    Building,
    /// `[[army]]`.
    Army,
    /// `[[point]]`, a lantern.
    Point,
    /// `[[object]]`, a terrain object.
    Object,
}

impl Kind {
    /// The key of the map file's table (`hero`, `building` …).
    pub fn key(self) -> &'static str {
        match self {
            Kind::Hero => "hero",
            Kind::Building => "building",
            Kind::Army => "army",
            Kind::Point => "point",
            Kind::Object => "object",
        }
    }

    /// The keys the format has in this table. An edit names one of these, or it is refused.
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Kind::Hero => &["cell", "classes", "gold", "mana", "troops"],
            Kind::Building => &["id", "type", "name", "description", "cell", "size", "faction", "income", "gold_max", "garrison", "recruit"],
            Kind::Army => &["id", "name", "cell", "faction", "model", "ship", "gold", "troops", "leader", "battle"],
            Kind::Point => &["id", "cell", "radius", "active"],
            Kind::Object => &["kind", "cell", "sprite"],
        }
    }
}

/// Which table of which file: the file (package-relative), the kind, the `id` when the table has
/// one, and the index among the tables of its kind (0-based, file order). `index` is used when
/// `id` is `None`; the hero's is always 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locator {
    pub file: String,
    pub kind: Kind,
    pub id: Option<String>,
    pub index: usize,
}

impl Locator {
    /// The table's name for a message: `hero`, `army toll`, `point #2`.
    pub fn name(&self) -> String {
        match (&self.kind, &self.id) {
            (Kind::Hero, _) => "hero".to_string(),
            (kind, Some(id)) => format!("{} {id}", kind.key()),
            (kind, None) => format!("{} #{}", kind.key(), self.index + 1),
        }
    }
}

/// One change to a map file's table. Each is one undo step.
#[derive(Clone, Debug)]
pub enum Edit {
    /// Moves the table's `cell` (the building's is its footprint's bottom-right cell).
    SetCell { at: Locator, cell: (i32, i32) },
    /// Sets one key of the table, which the format has (`Kind::keys`).
    SetValue { at: Locator, key: String, value: Value },
    /// Replaces an array value whole (`troops`, `garrison`, `recruit`, `classes`) with `items`: inline
    /// tables or strings. The array is written one item to a line when the old one was written so.
    SetArray { at: Locator, key: String, items: Vec<Value> },
    /// Removes an optional key of the table (its line).
    RemoveKey { at: Locator, key: String },
    /// Appends a new table of `at.kind` after the last table of that kind (at the end of the file
    /// when there is none). `fragment` is its TOML text, header included; `at.id` names it.
    Insert { at: Locator, fragment: String },
    /// Removes a table: its lines go, the comment lines above it stay. A hero cannot be removed.
    Remove { at: Locator },
}

impl Edit {
    /// The table the edit changes (for an insert, the table it adds).
    pub fn locator(&self) -> &Locator {
        match self {
            Edit::SetCell { at, .. } | Edit::SetValue { at, .. } | Edit::SetArray { at, .. } | Edit::RemoveKey { at, .. } => at,
            Edit::Insert { at, .. } | Edit::Remove { at } => at,
        }
    }

    /// The undo label: `move army toll`, `set gold of hero`, `add building village_2`.
    pub fn label(&self) -> String {
        match self {
            Edit::SetCell { at, .. } => format!("move {}", at.name()),
            Edit::SetValue { at, key, .. } | Edit::SetArray { at, key, .. } => format!("set {key} of {}", at.name()),
            Edit::RemoveKey { at, key } => format!("remove {key} from {}", at.name()),
            Edit::Insert { at, .. } => format!("add {} {}", at.kind.key(), at.id.as_deref().unwrap_or("")),
            Edit::Remove { at } => format!("delete {}", at.name()),
        }
    }
}

/// Where a TOML text stops parsing: the 1-based line and the parser's message. `None` when the
/// text parses.
pub fn parse_error(text: &str) -> Option<(u32, String)> {
    match text.parse::<DocumentMut>() {
        Ok(_) => None,
        Err(e) => Some((error_line(text, &e), e.message().to_string())),
    }
}

fn error_line(text: &str, e: &TomlError) -> u32 {
    e.span().map_or(1, |s| crate::pack::load::line_of(text, s.start))
}

/// The table of `at` in `doc`: the `[hero]` table, or the `[[kind]]` entry with the locator's id
/// (or its index when it has none).
fn table_mut<'d>(doc: &'d mut DocumentMut, at: &Locator) -> Result<&'d mut Table, String> {
    let key = at.kind.key();
    if at.kind == Kind::Hero {
        return match doc.get_mut(key) {
            Some(Item::Table(t)) => Ok(t),
            Some(_) => Err("[hero] is not a table".to_string()),
            None => Err("the map has no [hero] table".to_string()),
        };
    }
    let Some(Item::ArrayOfTables(entries)) = doc.get_mut(key) else {
        return Err(format!("the map has no [[{key}]] entries"));
    };
    let pos = match &at.id {
        Some(id) => entries
            .iter()
            .position(|t| t.get("id").and_then(Item::as_str) == Some(id.as_str()))
            .ok_or_else(|| format!("no [[{key}]] with id \"{id}\""))?,
        None => at.index,
    };
    entries.get_mut(pos).ok_or_else(|| format!("no [[{key}]] entry {}", pos + 1))
}

/// Replaces the value of `key` in `t`, keeping the old value's layout (its spacing and a
/// trailing comment), or adds the key at the end of the table when it is not there.
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

/// `[x, y]` as a TOML array.
fn cell_value(x: i32, y: i32) -> Value {
    let mut a = Array::new();
    a.push(i64::from(x));
    a.push(i64::from(y));
    Value::Array(a)
}

/// The edit applied to the text of its map file. The text must parse (see [`parse_error`]); the
/// result is the new text. An edit that names a key the format does not have, a table that is
/// not there, or a negative cell is an error, and the text is not changed.
pub fn apply_to_text(text: &str, edit: &Edit) -> Result<String, String> {
    if let Edit::Insert { at, fragment } = edit {
        return insert_table(text, at, fragment);
    }
    if let Edit::Remove { at } = edit {
        if at.kind == Kind::Hero {
            return Err("the hero cannot be removed: every map has one".to_string());
        }
        let r = find_span(text, at)?;
        return Ok(remove_lines(text, r));
    }
    let mut doc: DocumentMut = text.parse().map_err(|e: TomlError| format!("line {}: {}", error_line(text, &e), e.message()))?;
    match edit {
        Edit::SetCell { at, cell: (x, y) } => {
            if *x < 0 || *y < 0 {
                return Err(format!("cell [{x}, {y}] is outside the map"));
            }
            let t = table_mut(&mut doc, at)?;
            put(t, "cell", cell_value(*x, *y));
        }
        Edit::SetValue { at, key, value } => {
            if !at.kind.keys().contains(&key.as_str()) {
                return Err(format!("[[{}]] has no key `{key}`", at.kind.key()));
            }
            let t = table_mut(&mut doc, at)?;
            put(t, key, value.clone());
        }
        Edit::SetArray { at, key, items } => {
            if !at.kind.keys().contains(&key.as_str()) {
                return Err(format!("[[{}]] has no key `{key}`", at.kind.key()));
            }
            let t = table_mut(&mut doc, at)?;
            let multiline = t.get(key).and_then(Item::as_array).is_some_and(|a| a.to_string().contains('\n'));
            put(t, key, array_value(items.clone(), multiline));
        }
        Edit::RemoveKey { at, key } => {
            if !at.kind.keys().contains(&key.as_str()) {
                return Err(format!("[[{}]] has no key `{key}`", at.kind.key()));
            }
            // Removed as text, not through the document: `toml_edit` would drop the comment lines
            // above the key with it.
            return remove_key_lines(text, at, key);
        }
        Edit::Insert { .. } | Edit::Remove { .. } => unreachable!("handled above"),
    }
    Ok(doc.to_string())
}

/// The text without the lines of `key` in the table `at` (the comment lines above it stay).
fn remove_key_lines(text: &str, at: &Locator, key: &str) -> Result<String, String> {
    let doc = Document::parse(text).map_err(|e| format!("line {}: {}", error_line(text, &e), e.message()))?;
    let t = table_ref(&doc, at)?;
    let (k, item) = t.get_key_value(key).ok_or_else(|| format!("{} has no `{key}`", at.name()))?;
    let (Some(ks), Some(is)) = (k.span(), item.span()) else {
        return Err(format!("`{key}` of {} has no place in the file", at.name()));
    };
    let start = text[..ks.start].rfind('\n').map_or(0, |i| i + 1);
    let end = line_end(text, is.end.max(ks.end));
    Ok(format!("{}{}", &text[..start], &text[end..]))
}

/// The table `at` of a parsed map file (the read-only twin of `table_mut`).
fn table_ref<'d, S>(doc: &'d Document<S>, at: &Locator) -> Result<&'d Table, String> {
    let key = at.kind.key();
    if at.kind == Kind::Hero {
        return match doc.get(key) {
            Some(Item::Table(t)) => Ok(t),
            _ => Err("the map has no [hero] table".to_string()),
        };
    }
    let Some(Item::ArrayOfTables(entries)) = doc.get(key) else {
        return Err(format!("the map has no [[{key}]] entries"));
    };
    let pos = match &at.id {
        Some(id) => entries
            .iter()
            .position(|t| t.get("id").and_then(Item::as_str) == Some(id.as_str()))
            .ok_or_else(|| format!("no [[{key}]] with id \"{id}\""))?,
        None => at.index,
    };
    entries.get(pos).ok_or_else(|| format!("no [[{key}]] entry {}", pos + 1))
}

/// An array of `items`, one item to a line (four spaces in, a trailing comma) when `multiline`.
pub fn array_value(items: Vec<Value>, multiline: bool) -> Value {
    let mut a = Array::new();
    for item in items {
        a.push(item);
    }
    if multiline && !a.is_empty() {
        for item in a.iter_mut() {
            item.decor_mut().set_prefix("\n    ");
        }
        a.set_trailing_comma(true);
        a.set_trailing("\n");
    }
    Value::Array(a)
}

/// The byte offset just after the line that holds `pos` (the text's end when it is the last line).
fn line_end(text: &str, pos: usize) -> usize {
    text[pos.min(text.len())..].find('\n').map_or(text.len(), |i| pos + i + 1)
}

/// The byte range of the table `at` in `text`: the span of its header through its last value.
fn find_span(text: &str, at: &Locator) -> Result<Range<usize>, String> {
    let all = spans(&at.file, text)?;
    all.into_iter()
        .find(|(l, _)| l.kind == at.kind && (at.id.is_some() && l.id == at.id || at.id.is_none() && l.index == at.index))
        .map(|(_, r)| r)
        .ok_or_else(|| format!("{} is not in the file", at.name()))
}

/// The text without the lines of `r`: from the line of its start through the line of its end. A blank
/// line that stood above the table and one below it are not both kept.
fn remove_lines(text: &str, r: Range<usize>) -> String {
    let start = text[..r.start].rfind('\n').map_or(0, |i| i + 1);
    let mut end = line_end(text, r.end);
    if text[..start].ends_with("\n\n") && text[end..].starts_with('\n') {
        end += 1;
    }
    format!("{}{}", &text[..start], &text[end..])
}

/// The text with `fragment` appended after the last table of the kind of `at` (after its line, and
/// after a `# --- end generated ---` line that follows it). A table of the kind must not be missing
/// its place: with no table of the kind the fragment goes at the end of the file.
fn insert_table(text: &str, at: &Locator, fragment: &str) -> Result<String, String> {
    if at.kind == Kind::Hero {
        return Err("the hero is not added: every map has one".to_string());
    }
    let all = spans(&at.file, text)?;
    let last = all.iter().filter(|(l, _)| l.kind == at.kind).map(|(_, r)| r.end).max();
    let mut pos = match last {
        Some(end) => {
            let after = line_end(text, end);
            if text[after..].lines().next().is_some_and(|l| l.trim_start().starts_with("# --- end generated")) {
                line_end(text, after)
            } else {
                after
            }
        }
        None => text.len(),
    };
    if pos > text.len() {
        pos = text.len();
    }
    let mut out = String::with_capacity(text.len() + fragment.len() + 4);
    out.push_str(&text[..pos]);
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
    }
    out.push_str(fragment);
    if !fragment.ends_with('\n') {
        out.push('\n');
    }
    let rest = &text[pos..];
    if !rest.is_empty() && !rest.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(rest);
    Ok(out)
}

/// The byte range of a table: from its header through the end of its last key or value. The
/// comments above a table are not in it. `toml_edit` gives only the header's span, so the body's
/// end is the largest end of its keys and values.
fn table_range(t: &Table) -> Option<Range<usize>> {
    let header = t.span()?;
    let mut end = header.end;
    for (key, item) in t.iter() {
        if let Some((k, _)) = t.get_key_value(key) {
            end = end.max(k.span().map_or(0, |r| r.end));
        }
        end = end.max(item.span().map_or(0, |r| r.end));
    }
    Some(header.start..end)
}

/// The byte range of each table the studio knows in a map file's text (its header through its
/// last key), in file order. The hero's is `[hero]`'s; a `[[kind]]` entry's is its own. Ranges
/// do not overlap. An error with the line when the text does not parse.
pub fn spans(file: &str, text: &str) -> Result<Vec<(Locator, Range<usize>)>, String> {
    let doc = Document::parse(text).map_err(|e| format!("line {}: {}", error_line(text, &e), e.message()))?;
    let mut out = Vec::new();
    for kind in [Kind::Hero, Kind::Building, Kind::Army, Kind::Point, Kind::Object] {
        match doc.get(kind.key()) {
            Some(Item::Table(t)) if kind == Kind::Hero => {
                if let Some(r) = table_range(t) {
                    out.push((Locator { file: file.to_string(), kind, id: None, index: 0 }, r));
                }
            }
            Some(Item::ArrayOfTables(entries)) => {
                for (index, t) in entries.iter().enumerate() {
                    if let Some(r) = table_range(t) {
                        let id = t.get("id").and_then(Item::as_str).map(str::to_string);
                        out.push((Locator { file: file.to_string(), kind, id, index }, r));
                    }
                }
            }
            _ => {}
        }
    }
    out.sort_by_key(|(_, r)| r.start);
    Ok(out)
}

/// The table that holds byte `offset` of the map file's text, if any.
pub fn locator_at(file: &str, text: &str, offset: usize) -> Option<Locator> {
    spans(file, text).ok()?.into_iter().find(|(_, r)| r.contains(&offset)).map(|(l, _)| l)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "title = \"x\"\n\n# the hero\n[hero]\ncell = [1, 2] # start\n\n[[army]]\nid = \"toll\"\ncell = [5, 6]\n\n[[point]]\ncell = [9, 9]\n";

    fn loc(kind: Kind, id: Option<&str>, index: usize) -> Locator {
        Locator { file: "maps/x/map.toml".into(), kind, id: id.map(str::to_string), index }
    }

    #[test]
    fn set_cell_changes_one_value_and_keeps_the_trailing_comment() {
        let edit = Edit::SetCell { at: loc(Kind::Hero, None, 0), cell: (3, 4) };
        let out = apply_to_text(SAMPLE, &edit).unwrap();
        assert_eq!(out, SAMPLE.replace("cell = [1, 2] # start", "cell = [3, 4] # start"));
    }

    #[test]
    fn set_cell_finds_an_army_by_its_id() {
        let edit = Edit::SetCell { at: loc(Kind::Army, Some("toll"), 0), cell: (7, 8) };
        let out = apply_to_text(SAMPLE, &edit).unwrap();
        assert!(out.contains("id = \"toll\"\ncell = [7, 8]\n"), "{out}");
        let missing = Edit::SetCell { at: loc(Kind::Army, Some("nope"), 0), cell: (7, 8) };
        assert!(apply_to_text(SAMPLE, &missing).unwrap_err().contains("nope"));
    }

    #[test]
    fn set_value_refuses_a_key_the_format_does_not_have() {
        let edit = Edit::SetValue { at: loc(Kind::Army, Some("toll"), 0), key: "colour".into(), value: Value::from("red") };
        let err = apply_to_text(SAMPLE, &edit).unwrap_err();
        assert!(err.contains("colour"), "{err}");
        let ok = Edit::SetValue { at: loc(Kind::Army, Some("toll"), 0), key: "gold".into(), value: Value::from(40_i64) };
        let out = apply_to_text(SAMPLE, &ok).unwrap();
        assert!(out.contains("id = \"toll\""), "{out}");
    }

    #[test]
    fn a_negative_cell_is_refused() {
        let edit = Edit::SetCell { at: loc(Kind::Point, None, 0), cell: (-1, 3) };
        assert!(apply_to_text(SAMPLE, &edit).is_err());
    }

    #[test]
    fn spans_are_disjoint_and_locate_the_tables() {
        let s = spans("maps/x/map.toml", SAMPLE).unwrap();
        assert_eq!(s.len(), 3);
        for w in s.windows(2) {
            assert!(w[0].1.end <= w[1].1.start, "disjoint");
        }
        assert!(s.iter().all(|(_, r)| r.end <= SAMPLE.len()));
        let at = SAMPLE.find("id = \"toll\"").unwrap();
        assert_eq!(locator_at("maps/x/map.toml", SAMPLE, at).unwrap(), loc(Kind::Army, Some("toll"), 0));
        // A comment above the hero's header belongs to no table.
        assert_eq!(locator_at("maps/x/map.toml", SAMPLE, SAMPLE.find("# the hero").unwrap()), None);
        assert_eq!(locator_at("maps/x/map.toml", SAMPLE, 0), None, "a root key is not a table");
    }

    #[test]
    fn a_broken_text_has_no_spans_and_says_where() {
        let broken = "[[army]\nid = 1\n";
        assert!(spans("maps/x/map.toml", broken).is_err());
        let (line, _) = parse_error(broken).unwrap();
        assert_eq!(line, 1);
        assert!(parse_error(SAMPLE).is_none());
    }

    #[test]
    fn labels_name_the_table() {
        assert_eq!(Edit::SetCell { at: loc(Kind::Army, Some("toll"), 0), cell: (0, 0) }.label(), "move army toll");
        assert_eq!(Edit::SetCell { at: loc(Kind::Hero, None, 0), cell: (0, 0) }.label(), "move hero");
        assert_eq!(Edit::SetCell { at: loc(Kind::Point, None, 1), cell: (0, 0) }.label(), "move point #2");
    }
}
