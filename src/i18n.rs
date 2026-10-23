//! Razdor's own interface texts in English and Russian.
//!
//! The English text is the key and the fallback: `tr("Save the game")` gives the Russian of
//! it from the catalog in `data/lang/ru/*.txt` (our own translation, `English = Russian`
//! lines) when the language is Russian, else the text itself. Texts with values use
//! [`trf!`](crate::trf) with named placeholders, so a translation may put them in any order:
//! `trf!("{name} hits {target} for {dealt}", name, target = t.name, dealt)`.
//!
//! Texts that are only named in one place and translated in another (tables of labels) are
//! marked with [`n_`], which returns its argument: the catalog test finds them by it.
//!
//! What the player's install brings (unit, item and spell names, map texts) is already
//! Russian and never passes through here.

use crate::local;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// An interface language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    Ru,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ru];

    /// The code kept in the settings file.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code().eq_ignore_ascii_case(code.trim()))
    }

    /// The other language (the EN / RU switch).
    pub fn other(self) -> Lang {
        match self {
            Lang::En => Lang::Ru,
            Lang::Ru => Lang::En,
        }
    }

    /// The switch's label: "EN" or "RU".
    pub fn label(self) -> &'static str {
        match self {
            Lang::En => "EN",
            Lang::Ru => "RU",
        }
    }
}

/// The current language. The library starts in English (tests read English texts); the game
/// sets the player's choice at startup (`ui::language`, Russian when nothing is saved).
static LANG: AtomicU8 = AtomicU8::new(0);

pub fn lang() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        1 => Lang::Ru,
        _ => Lang::En,
    }
}

pub fn set_lang(lang: Lang) {
    LANG.store(lang as u8, Ordering::Relaxed);
}

/// The Russian catalog, one file per part of the game (file name, contents).
pub const RU_FILES: &[(&str, &str)] = &[
    ("common.txt", include_str!("../data/lang/ru/common.txt")),
    ("game.txt", include_str!("../data/lang/ru/game.txt")),
    ("world.txt", include_str!("../data/lang/ru/world.txt")),
    ("battle.txt", include_str!("../data/lang/ru/battle.txt")),
    ("editor.txt", include_str!("../data/lang/ru/editor.txt")),
    ("events.txt", include_str!("../data/lang/ru/events.txt")),
    ("translate.txt", include_str!("../data/lang/ru/translate.txt")),
];

/// One catalog line: its file and line number, the English key and the translation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub file: &'static str,
    pub line: usize,
    pub key: String,
    pub value: String,
}

/// `\n`, `\t` and `\\` in a catalog text.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            (c, false) => out.push(c),
        }
    }
    out
}

/// Parses a catalog file: `English = Russian` lines (split at the first " = "), `#`
/// comments and blank lines; `\n` stands for a line break and `\=` for an `=` that must not
/// be taken for the separator (an English text with " = " in it). Malformed lines are errors.
pub fn parse_catalog(file: &'static str, text: &str) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(" = ") else {
            return Err(format!("{file}:{}: no \" = \" in {line:?}", i + 1));
        };
        let (key, value) = (unescape(key.trim()), unescape(value.trim()));
        if key.is_empty() || value.is_empty() {
            return Err(format!("{file}:{}: empty text in {line:?}", i + 1));
        }
        entries.push(Entry { file, line: i + 1, key, value });
    }
    Ok(entries)
}

/// Every entry of the Russian catalog, in file order.
pub fn ru_entries() -> Vec<Entry> {
    RU_FILES.iter().flat_map(|(file, text)| parse_catalog(file, text).unwrap_or_else(|e| panic!("{e}"))).collect()
}

fn ru_catalog() -> &'static HashMap<String, String> {
    static RU: OnceLock<HashMap<String, String>> = OnceLock::new();
    RU.get_or_init(|| ru_entries().into_iter().map(|e| (e.key, e.value)).collect())
}

/// `en` in `lang`: the catalog's translation, or `en` itself.
pub fn tr_in(lang: Lang, en: &str) -> &str {
    match lang {
        Lang::En => en,
        Lang::Ru => ru_catalog().get(en).map_or(en, String::as_str),
    }
}

/// `en` in the current language.
pub fn tr(en: &str) -> &str {
    tr_in(lang(), en)
}

/// Marks a text as one to translate where it is shown (with [`tr`]); returns it unchanged.
pub const fn n_(en: &'static str) -> &'static str {
    en
}

local! {
    static SILENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// While set on this thread, [`fill`] (and so `trf!`) gives empty texts: Razdor's battle
/// search, whose positions' logs nobody reads, skips their formatting. Returns the setting
/// it replaces.
pub fn set_silent(on: bool) -> bool {
    SILENT.with(|s| s.replace(on))
}

/// Fills the `{name}` placeholders of `template` with `args`; `{{` and `}}` are braces.
/// An unknown placeholder stays as it is.
pub fn fill(template: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    if SILENT.with(std::cell::Cell::get) {
        return String::new();
    }
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if let (true, Some(end)) = (tail.starts_with('{'), tail.find('}')) {
            let name = &tail[1..end];
            if let Some((_, v)) = args.iter().find(|(n, _)| *n == name) {
                out.push_str(&v.to_string());
                rest = &tail[end + 1..];
                continue;
            }
        }
        out.push_str(&tail[..1]);
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// The `{name}` placeholders of a text, sorted (`{{`/`}}` are not placeholders).
pub fn placeholders(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            rest = &tail[2..];
        } else if let (true, Some(end)) = (tail.starts_with('{'), tail.find('}')) {
            names.push(tail[1..end].to_string());
            rest = &tail[end + 1..];
        } else {
            rest = &tail[1..];
        }
    }
    names.sort();
    names.dedup();
    names
}

/// A translated text with values: `trf!("Gold: {gold}", gold = g.gold)`, or just
/// `trf!("Gold: {gold}", gold)` for a variable of that name. The key must be a literal (the
/// catalog test reads it from the source).
#[macro_export]
macro_rules! trf {
    (@val $name:ident = $val:expr) => { $val };
    (@val $name:ident) => { $name };
    ($key:literal $(, $name:ident $(= $val:expr)?)* $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::tr($key),
            &[$((stringify!($name), &$crate::trf!(@val $name $(= $val)?) as &dyn ::std::fmt::Display)),*],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    #[test]
    fn fills_named_placeholders_in_any_order() {
        let (a, b) = (3, "Ivan");
        assert_eq!(fill("{b} has {a} gold", &[("a", &a), ("b", &b)]), "Ivan has 3 gold");
        assert_eq!(fill("{a}/{a} {{x}} {c}", &[("a", &a)]), "3/3 {x} {c}");
        assert_eq!(fill("no braces", &[]), "no braces");
        assert_eq!(fill("half { open", &[]), "half { open");
        assert_eq!(placeholders("{b} and {a}, {a} {{not}}"), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn the_macro_takes_shorthand_and_expressions() {
        let gold = 5;
        // Not in the catalog: the English text is used as it is in both languages.
        assert_eq!(crate::trf!("{gold} and {more} (test only)", gold, more = gold * 2), "5 and 10 (test only)");
        assert_eq!(tr_in(Lang::Ru, "not a catalog text"), "not a catalog text");
    }

    #[test]
    fn languages_have_codes_and_labels() {
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), Some(l));
            assert_eq!(l.other().other(), l);
        }
        assert_eq!(Lang::from_code(" RU "), Some(Lang::Ru));
        assert_eq!(Lang::from_code("de"), None);
        assert_eq!(parse_catalog("t", "# c\n\nA\\nB = Б\\nВ\n").unwrap()[0].value, "Б\nВ");
        assert!(parse_catalog("t", "no separator").is_err());
        let e = &parse_catalog("t", "0 \\= no, or \\= = 0 = нет, или =").unwrap()[0];
        assert_eq!((e.key.as_str(), e.value.as_str()), ("0 = no, or =", "0 = нет, или ="));
    }

    // --------------------------------------------------------------------------------------
    // The catalog against the sources: every text the code translates is in the catalog,
    // every catalog line is used, and the placeholders agree.
    // --------------------------------------------------------------------------------------

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                rust_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    fn sources() -> Vec<(PathBuf, String)> {
        let mut files = Vec::new();
        rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        files.sort();
        files.into_iter().map(|p| (p.clone(), std::fs::read_to_string(&p).unwrap())).collect()
    }

    /// Reads a Rust string literal starting at the opening quote of `s`: its value and length.
    fn literal(s: &str) -> Option<(String, usize)> {
        let mut out = String::new();
        let mut it = s.char_indices().skip(1);
        while let Some((i, c)) = it.next() {
            match c {
                '"' => return Some((out, i + 1)),
                '\\' => match it.next()?.1 {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '0' => out.push('\0'),
                    '\n' => {
                        // A line continuation: the newline and the next line's indent go.
                        let mut peek = it.clone();
                        while let Some((_, w)) = peek.next() {
                            if !w.is_whitespace() {
                                break;
                            }
                            it = peek.clone();
                        }
                    }
                    other => out.push(other),
                },
                c => out.push(c),
            }
        }
        None
    }

    /// Every key the code passes to `tr(`, `trf!(` and `n_(` as a literal: key -> places.
    fn used_keys() -> BTreeMap<String, Vec<String>> {
        let mut keys: BTreeMap<String, Vec<String>> = BTreeMap::new();
        // This file only names the markers.
        for (path, src) in sources().into_iter().filter(|(p, _)| !p.ends_with("src/i18n.rs")) {
            let name = path.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap_or(&path).display().to_string();
            for (n, line) in src.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                for marker in ["tr(", "trf!(", "n_("] {
                    let mut from = 0;
                    while let Some(i) = line[from..].find(marker) {
                        let at = from + i;
                        from = at + marker.len();
                        let before = line[..at].chars().next_back();
                        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                            continue;
                        }
                        let rest = line[from..].trim_start();
                        if !rest.starts_with('"') {
                            continue;
                        }
                        // The literal may continue on the next lines (`\` at the end).
                        let offset = src.lines().take(n).map(|l| l.len() + 1).sum::<usize>() + (line.len() - rest.len());
                        if let Some((key, _)) = literal(&src[offset..]) {
                            keys.entry(key).or_default().push(format!("{name}:{}", n + 1));
                        }
                    }
                }
            }
        }
        keys
    }

    #[test]
    fn every_translated_text_is_in_the_catalog_and_every_catalog_text_is_used() {
        let entries = ru_entries();
        let used = used_keys();
        let mut problems = Vec::new();
        let mut seen: BTreeMap<&str, &Entry> = BTreeMap::new();
        for e in &entries {
            if let Some(first) = seen.get(e.key.as_str()) {
                if first.value != e.value {
                    problems.push(format!("{}:{} and {}:{}: {:?} translated twice, differently", first.file, first.line, e.file, e.line, e.key));
                }
            } else {
                seen.insert(&e.key, e);
            }
            if e.key.trim() != e.key {
                problems.push(format!("{}:{}: key {:?} has spaces at its ends", e.file, e.line, e.key));
            }
            if placeholders(&e.key) != placeholders(&e.value) {
                problems.push(format!("{}:{}: placeholders differ in {:?} = {:?}", e.file, e.line, e.key, e.value));
            }
            if !used.contains_key(&e.key) {
                problems.push(format!("{}:{}: {:?} is not used by the code", e.file, e.line, e.key));
            }
        }
        for (key, places) in &used {
            if !seen.contains_key(key.as_str()) {
                problems.push(format!("{}: {key:?} is missing from data/lang/ru", places[0]));
            }
        }
        assert!(problems.is_empty(), "{} catalog problems:\n{}", problems.len(), problems.join("\n"));
    }

    /// Drawing helpers of `src/ui` that take the text to show as their first string argument.
    const DRAWERS: &[&str] = &[
        "text(",
        "text_centered(",
        "button(",
        "small_button(",
        "toggle_button(",
        "checkbox(",
        "shadow_text(",
        "shadow_centered(",
        "shadow_right(",
        "strong_text(",
        "strong_centered(",
        "engraved_centered(",
        "strong_right(",
        "window(",
        "title_bar(",
        "hint_strip(",
        "hint_text(",
        "pill_button(",
        "marble_button(",
    ];

    #[test]
    fn interface_texts_go_through_the_catalog() {
        // An English literal handed straight to a drawing helper in src/ui is a text the
        // player would see untranslated.
        let mut problems = Vec::new();
        for (path, src) in sources() {
            let name = path.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap_or(&path).display().to_string();
            if !name.contains("/ui/") {
                continue;
            }
            let body = src.split("#[cfg(test)]\nmod tests").next().unwrap_or(&src);
            for (n, line) in body.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for d in DRAWERS {
                    let mut from = 0;
                    while let Some(i) = line[from..].find(d) {
                        let at = from + i;
                        from = at + d.len();
                        // Methods (`f.text(key, label, …)` of the editor's forms) are not the helpers.
                        if line[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                            continue;
                        }
                        // The first string literal among the arguments, if any, before the call's end.
                        let args = &line[from..];
                        let Some(q) = args.find('"') else { continue };
                        if args[..q].contains(')') || args[..q].contains("tr(") || args[..q].contains("trf!(") {
                            continue;
                        }
                        let Some((s, _)) = literal(&args[q..]) else { continue };
                        let words = fill(&s, &[]).split(['{', '}']).step_by(2).collect::<String>();
                        if words.chars().filter(|c| c.is_ascii_lowercase()).count() >= 2 {
                            problems.push(format!("{name}:{}: {s:?}", n + 1));
                        }
                    }
                }
            }
        }
        assert!(problems.is_empty(), "untranslated interface texts:\n{}", problems.join("\n"));
    }

    /// Labels the Russian original itself shows in Latin (the battle cards' stat strip).
    const ORIGINAL_LATIN: &[&str] =
        &["A: {v}", "Pwr: {power}", "D: {blow}/{shot}", "Mnvr: {mn}", "Ini: {ini}", "Hits: {hp}/{max}", "Hits: {max}"];

    #[test]
    fn the_catalog_is_russian() {
        let latin_only: BTreeSet<String> = ru_entries()
            .into_iter()
            .filter(|e| !(ORIGINAL_LATIN.contains(&e.key.as_str()) && e.value == e.key))
            .filter(|e| !e.value.chars().any(|c| ('а'..='я').contains(&c.to_lowercase().next().unwrap_or(c))))
            .map(|e| format!("{}:{}: {:?}", e.file, e.line, e.value))
            .collect();
        assert!(latin_only.is_empty(), "catalog lines with no Russian:\n{}", latin_only.into_iter().collect::<Vec<_>>().join("\n"));
    }
}
