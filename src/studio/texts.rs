//! The texts table of a package (P3): one row per text key the package uses or defines, one column
//! per language in `texts/`. A key used but defined in no language is red; a key defined but never
//! used is grey; a key that some language lacks (or has empty) is amber. Each cell is an edit of
//! its language's file (`texts/<lang>.toml`) through `toml_edit`: a new key goes at the end of its
//! dotted group's table (or, when the group is new, at the end of the file as a new `[group]`).
//!
//! The named characters' name keys (`named.<id>`, `<map>.named.<id>`) are used when their character
//! is named by the story, so a defined name key of a character is not grey.

use std::collections::{BTreeMap, BTreeSet};

use toml_edit::Value;

use super::characters;
use super::doc::{table, DocEdit, Hop};
use crate::pack::load::Package;

/// How a row stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Defined in every language, and used (or a name key of a character).
    Complete,
    /// Used, and defined in no language.
    Undefined,
    /// Defined, and never used.
    Unused,
    /// Defined in some languages; these lack it (missing or empty).
    Missing(Vec<String>),
}

/// One key of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    /// The text in each language (the table's `langs` order); empty when missing.
    pub cells: Vec<String>,
    pub status: Status,
    /// Where the package uses the key: `file:line` (the first use).
    pub used_at: Option<(String, u32)>,
}

/// The table: its languages (the `texts/` files) and its rows, sorted by key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub langs: Vec<String>,
    pub rows: Vec<Row>,
}

/// Whether `key` is a character's name key (`named.<id>` or `<map>.named.<id>`).
fn is_name_key(key: &str, chars: &BTreeSet<String>) -> bool {
    let parts: Vec<&str> = key.split('.').collect();
    match parts.as_slice() {
        ["named", id] => chars.contains(*id),
        [_, "named", id] => chars.contains(*id),
        _ => false,
    }
}

/// The table of the package's texts (as the files say now: `pkg.texts` is the last good read).
pub fn table_of(pkg: &Package) -> Table {
    let langs: Vec<String> = pkg.texts.keys().cloned().collect();
    let chars: BTreeSet<String> = characters::characters(pkg).into_iter().map(|c| c.id).collect();
    let mut used: BTreeMap<String, (String, u32)> = BTreeMap::new();
    for (key, file, line) in crate::pack::check::used_keys(pkg) {
        used.entry(key).or_insert((file, line.unwrap_or(1)));
    }
    let mut keys: BTreeSet<String> = used.keys().cloned().collect();
    for texts in pkg.texts.values() {
        keys.extend(texts.keys().cloned());
    }
    let rows = keys
        .into_iter()
        .map(|key| {
            let cells: Vec<String> = langs.iter().map(|l| pkg.texts.get(l).and_then(|t| t.get(&key)).cloned().unwrap_or_default()).collect();
            let defined = cells.iter().any(|c| !c.is_empty());
            let is_used = used.contains_key(&key);
            let status = if is_used && !defined {
                Status::Undefined
            } else if !is_used && defined && !is_name_key(&key, &chars) {
                Status::Unused
            } else {
                let lacking: Vec<String> = langs.iter().zip(&cells).filter(|(_, c)| c.is_empty()).map(|(l, _)| l.clone()).collect();
                if defined && !lacking.is_empty() {
                    Status::Missing(lacking)
                } else {
                    Status::Complete
                }
            };
            Row { used_at: used.get(&key).cloned(), key, cells, status }
        })
        .collect();
    Table { langs, rows }
}

/// The edit that sets `key` in language `lang` to `value` (its file `texts/<lang>.toml`): the key's
/// dotted groups are tables of the file, and the key is the last segment.
pub fn set_cell(lang: &str, key: &str, value: &str) -> (String, DocEdit) {
    let mut parts: Vec<&str> = key.split('.').collect();
    let last = parts.pop().unwrap_or(key).to_string();
    let path: Vec<Hop> = parts.iter().map(|p| table(p)).collect();
    (format!("texts/{lang}.toml"), DocEdit::Set { path, key: last, value: Value::from(value) })
}

/// The text of a new `texts/<lang>.toml`: every key of `keys` with an empty text, grouped by its
/// dotted parts (a `[group]` header per group, the keys sorted).
pub fn empty_file(keys: &[String]) -> String {
    let mut sorted: Vec<&String> = keys.iter().collect();
    sorted.sort();
    sorted.dedup();
    let mut out = String::from("# Texts of one language: one key = \"text\" per line, grouped by its dotted parts.\n");
    let mut group: Option<String> = None;
    for key in sorted {
        let mut parts: Vec<&str> = key.split('.').collect();
        let last = parts.pop().unwrap_or("");
        let g = parts.join(".");
        if group.as_deref() != Some(g.as_str()) {
            if !g.is_empty() {
                out.push_str(&format!("\n[{}]\n", parts.iter().map(|p| bare(p)).collect::<Vec<_>>().join(".")));
            }
            group = Some(g);
        }
        out.push_str(&format!("{} = \"\"\n", bare(last)));
    }
    out
}

/// A key part as TOML writes it: bare when it is made of letters, digits, `_` and `-`, else quoted.
fn bare(part: &str) -> String {
    if !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        part.to_string()
    } else {
        format!("\"{}\"", part.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// A language code as a file name: lowercase letters, digits and `-` (`ru`, `en`, `pt-br`).
pub fn valid_lang(lang: &str) -> bool {
    !lang.is_empty() && lang.len() <= 16 && lang.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::files::Files;
    use crate::studio::doc::apply_text;

    fn package(texts_en: &str, texts_ru: Option<&str>) -> Package {
        let mut files = Files::default();
        files.insert("package.toml", "id = \"x\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"story.title\"\nauthor = \"a\"\nlicense = \"MIT\"\n");
        files.insert("texts/en.toml", texts_en);
        if let Some(ru) = texts_ru {
            files.insert("texts/ru.toml", ru);
        }
        files.insert("maps/brod/map.toml", "title = \"brod.title\"\n");
        files.insert("triggers/a.toml", "[[trigger]]\nid = \"j\"\nmap = \"brod\"\nwhen = { event = \"day_start\" }\nthen = [ { give_unit = \"dt:pilgrim\", named = \"ilm\" }, { dialog = \"d\" } ]\n");
        files.insert("dialogs/a.toml", "[[dialog]]\nid = \"d\"\ntitle = \"story.nowhere\"\ntext = \"story.gone\"\n");
        let (pkg, _) = Package::parse(files);
        pkg
    }

    fn row<'a>(t: &'a Table, key: &str) -> &'a Row {
        t.rows.iter().find(|r| r.key == key).unwrap_or_else(|| panic!("no row {key}: {:?}", t.rows.iter().map(|r| &r.key).collect::<Vec<_>>()))
    }

    #[test]
    fn missing_undefined_and_unused_keys_are_marked() {
        let en = "unused = \"u\"\n\n[story]\ntitle = \"Story\"\ngone = \"Gone\"\n\n[named]\nilm = \"Brother Ilm\"\n";
        let pkg = package(en, Some("[story]\ntitle = \"История\"\n"));
        let t = table_of(&pkg);
        assert_eq!(t.langs, ["en", "ru"]);
        assert_eq!(row(&t, "story.title").status, Status::Complete);
        assert_eq!(row(&t, "story.gone").status, Status::Missing(vec!["ru".into()]), "defined in en only");
        assert_eq!(row(&t, "story.nowhere").status, Status::Undefined, "used, defined nowhere");
        assert_eq!(row(&t, "unused").status, Status::Unused, "defined, never used");
        assert_eq!(row(&t, "named.ilm").status, Status::Missing(vec!["ru".into()]), "a character's name is used when the character is named");
    }

    #[test]
    fn a_cell_edit_writes_its_own_language_file_and_keeps_the_rest() {
        let en = "# the english\ntitle = \"Story\" # shown\n\n[unit.knight]\nname = \"Knight\"\n";
        let (file, edit) = set_cell("en", "unit.knight.description", "A knight.");
        assert_eq!(file, "texts/en.toml");
        let out = apply_text(en, &edit).unwrap();
        assert!(out.starts_with("# the english\ntitle = \"Story\" # shown\n"), "{out}");
        assert!(out.contains("[unit.knight]\nname = \"Knight\"\ndescription = \"A knight.\""), "{out}");
        let (_, edit) = set_cell("en", "new.group.key", "v");
        let out = apply_text(en, &edit).unwrap();
        assert!(out.ends_with("\n[new.group]\nkey = \"v\"\n") || out.contains("[new.group]\nkey = \"v\""), "a new group is a new table at the end: {out}");
    }

    #[test]
    fn a_new_language_file_has_every_key_empty_and_grouped() {
        let keys = vec!["title".to_string(), "unit.knight.name".to_string(), "unit.knight.description".to_string(), "unit.archer.name".to_string()];
        let text = empty_file(&keys);
        let parsed: toml::Table = toml::from_str(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        let flat = crate::pack::texts::flatten(&parsed).unwrap();
        assert_eq!(flat.len(), 4);
        assert!(flat.values().all(String::is_empty));
        assert!(flat.contains_key("unit.archer.name"));
        assert!(text.contains("[unit.knight]\ndescription = \"\"\nname = \"\""), "{text}");
    }

    #[test]
    fn a_key_with_odd_characters_is_quoted() {
        let text = empty_file(&["a.b c".to_string()]);
        let parsed: toml::Table = toml::from_str(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert!(crate::pack::texts::flatten(&parsed).unwrap().contains_key("a.b c"), "{text}");
    }

    #[test]
    fn the_language_codes_are_file_names() {
        assert!(valid_lang("ru") && valid_lang("pt-br"));
        assert!(!valid_lang("../x") && !valid_lang("RU") && !valid_lang(""));
    }
}
