//! Reads a package's files into a `Package`: the manifest, the texts and the content. Syntax
//! errors become diagnostics with their line; the rules are in `check`.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;

use super::content::{ItemFile, ItemSpec, UnitFile, UnitSpec};
use super::diag::{Report, Severity};
use super::files::Files;
use super::manifest::Manifest;
use super::triggers::{self, Dialog, DialogFile, Trigger, TriggerFile, Var};

/// Something read from one file, with where it was.
#[derive(Clone, Debug, PartialEq)]
pub struct Located<T> {
    /// Package-relative path.
    pub file: String,
    /// 1-based line of the table in `file`.
    pub line: u32,
    pub value: T,
}

/// A package as read. `problems` holds what reading found; `check` starts from it.
#[derive(Debug)]
pub struct Package {
    pub files: Files,
    pub manifest: Option<Manifest>,
    /// Language (file stem of `texts/<lang>.toml`) to key to text.
    pub texts: BTreeMap<String, BTreeMap<String, String>>,
    pub units: Vec<Located<UnitSpec>>,
    pub items: Vec<Located<ItemSpec>>,
    /// `[vars]` of the `triggers/*.toml` files.
    pub vars: Vec<Located<Var>>,
    pub triggers: Vec<Located<Trigger>>,
    pub dialogs: Vec<Located<Dialog>>,
    pub problems: Report,
}

/// The game's own files: never allowed in a package (`check` reports them).
pub const GAME_EXTENSIONS: [&str; 7] = ["ini", "dtm", "ugs", "lit", "spi", "exe", "dll"];

/// Whether `path` has a game file's extension (case-insensitive).
pub fn is_game_file(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && GAME_EXTENSIONS.iter().any(|g| g.eq_ignore_ascii_case(ext)))
}

/// 1-based line of byte `offset` in `text`.
pub fn line_of(text: &str, offset: usize) -> u32 {
    let end = offset.min(text.len());
    text.as_bytes()[..end].iter().filter(|&&b| b == b'\n').count() as u32 + 1
}

/// The line of the first `key = …` line of a TOML text, if any.
pub fn key_line(text: &str, key: &str) -> Option<u32> {
    text.lines().position(|l| {
        l.trim_start()
            .strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
    .map(|i| i as u32 + 1)
}

/// Parses a TOML file into `T`; a failure becomes an error with its line.
fn parse_toml<T: DeserializeOwned>(file: &str, text: &str, report: &mut Report) -> Option<T> {
    match toml::from_str::<T>(text) {
        Ok(v) => Some(v),
        Err(e) => {
            let line = e.span().map(|s| line_of(text, s.start));
            report.error(file, line, e.message());
            None
        }
    }
}

impl Package {
    /// Reads `files`. The report has what could not be read: syntax errors, invalid UTF-8, unsafe
    /// archive paths. Rules are checked by `check::check`, which also reports these.
    pub fn parse(files: Files) -> (Package, Report) {
        let mut report = Report { diagnostics: files.problems.clone() };
        let mut package = Package {
            files: Files::default(),
            manifest: None,
            texts: BTreeMap::new(),
            units: Vec::new(),
            items: Vec::new(),
            vars: Vec::new(),
            triggers: Vec::new(),
            dialogs: Vec::new(),
            problems: Report::default(),
        };
        // Game files are never read; `check` reports them.
        let paths: Vec<String> = files.paths().map(str::to_string).collect();

        // package.toml
        if let Some(text) = text_of(&files, "package.toml", &mut report) {
            package.manifest = parse_toml::<Manifest>("package.toml", text, &mut report);
        }

        // texts/<lang>.toml
        for path in paths.iter().filter(|p| p.starts_with("texts/")) {
            let Some(lang) = path.strip_prefix("texts/").and_then(|r| r.strip_suffix(".toml")) else {
                if !is_game_file(path) {
                    report.warning(path, None, "not read: texts are texts/<lang>.toml");
                }
                continue;
            };
            if lang.is_empty() || lang.contains('/') {
                report.warning(path, None, "not read: texts are texts/<lang>.toml");
                continue;
            }
            let Some(text) = text_of(&files, path, &mut report) else { continue };
            let Some(table) = parse_toml::<toml::Table>(path, text, &mut report) else { continue };
            match super::texts::flatten(&table) {
                Ok(flat) => {
                    package.texts.insert(lang.to_string(), flat);
                }
                Err(message) => report.error(path, None, message),
            }
        }

        // units/**/*.toml and items/**/*.toml
        for path in paths.iter().filter(|p| p.starts_with("units/")) {
            if let Some(text) = content_text(&files, path, &mut report) {
                if let Some(f) = parse_toml::<UnitFile>(path, text, &mut report) {
                    for u in f.unit {
                        let line = line_of(text, u.span().start);
                        package.units.push(Located { file: path.clone(), line, value: u.into_inner() });
                    }
                }
            }
        }
        for path in paths.iter().filter(|p| p.starts_with("items/")) {
            if let Some(text) = content_text(&files, path, &mut report) {
                if let Some(f) = parse_toml::<ItemFile>(path, text, &mut report) {
                    for i in f.item {
                        let line = line_of(text, i.span().start);
                        package.items.push(Located { file: path.clone(), line, value: i.into_inner() });
                    }
                }
            }
        }

        // triggers/*.toml: variables and triggers. A table that does not read is reported at its
        // line and left out; `check` reads the rest.
        for path in paths.iter().filter(|p| p.starts_with("triggers/") && p.ends_with(".toml")) {
            let Some(text) = story_text(&files, path, &mut report) else { continue };
            let Some(f) = parse_toml::<TriggerFile>(path, text, &mut report) else { continue };
            match triggers::parse_vars(&f.vars) {
                Ok(vars) => {
                    for v in vars {
                        let line = key_line(text, &v.name);
                        package.vars.push(Located { file: path.clone(), line: line.unwrap_or(1), value: v });
                    }
                }
                Err(p) => report.error_hint(path, key_line(text, "vars").or(Some(1)), p.message, p.hint),
            }
            for t in f.trigger {
                let line = line_of(text, t.span().start);
                match triggers::parse_trigger(t.get_ref()) {
                    Ok(trigger) => package.triggers.push(Located { file: path.clone(), line, value: trigger }),
                    Err(p) => report.error_hint(path, Some(line), p.message, p.hint),
                }
            }
        }
        // dialogs/*.toml: dialog windows with their choices.
        for path in paths.iter().filter(|p| p.starts_with("dialogs/") && p.ends_with(".toml")) {
            let Some(text) = story_text(&files, path, &mut report) else { continue };
            let Some(f) = parse_toml::<DialogFile>(path, text, &mut report) else { continue };
            for d in f.dialog {
                let line = line_of(text, d.span().start);
                match triggers::parse_dialog(d.get_ref()) {
                    Ok(dialog) => package.dialogs.push(Located { file: path.clone(), line, value: dialog }),
                    Err(p) => report.error_hint(path, Some(line), p.message, p.hint),
                }
            }
        }

        package.problems = report.clone();
        package.files = files;
        (package, report)
    }
}

/// The text of a file that is read, or `None` after reporting why it is not valid UTF-8.
fn text_of<'a>(files: &'a Files, path: &str, report: &mut Report) -> Option<&'a str> {
    match files.get(path) {
        None => None,
        Some(bytes) => match std::str::from_utf8(bytes) {
            Ok(t) => Some(t),
            Err(_) => {
                report.push(Severity::Error, path, None, "not UTF-8 text", Some("save the file as UTF-8".into()));
                None
            }
        },
    }
}

/// The text of a triggers or dialogs file: TOML, else reported and not read.
fn story_text<'a>(files: &'a Files, path: &str, report: &mut Report) -> Option<&'a str> {
    if is_game_file(path) {
        return None;
    }
    text_of(files, path, report)
}

/// Content files are TOML; other files there are not read (a game file is `check`'s error).
fn content_text<'a>(files: &'a Files, path: &str, report: &mut Report) -> Option<&'a str> {
    if is_game_file(path) {
        return None;
    }
    if !path.ends_with(".toml") {
        report.warning(path, None, "not read: units and items are .toml files");
        return None;
    }
    text_of(files, path, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_extensions_are_case_insensitive() {
        assert!(is_game_file("maps/a.DTm"));
        assert!(is_game_file("x/Rus_Units.ini"));
        assert!(is_game_file("a.UGS"));
        assert!(!is_game_file("units/a.toml"));
        assert!(!is_game_file(".ini"));
    }

    #[test]
    fn line_numbers() {
        assert_eq!(line_of("a\nb\nc", 0), 1);
        assert_eq!(line_of("a\nb\nc", 2), 2);
        assert_eq!(line_of("a\nb\nc", 100), 3);
        assert_eq!(key_line("id = \"x\"\n  license = \"MIT\"\n", "license"), Some(2));
        assert_eq!(key_line("licence = \"MIT\"\n", "license"), None);
    }

    #[test]
    fn a_syntax_error_has_its_line() {
        let mut files = Files::default();
        files.insert("package.toml", "id = \"x\"\nversion = \n");
        let (_, report) = Package::parse(files);
        assert_eq!(report.diagnostics.len(), 1);
        let d = &report.diagnostics[0];
        assert_eq!(d.file, "package.toml");
        assert_eq!(d.line, Some(2));
        assert_eq!(d.severity, Severity::Error);
    }

    #[test]
    fn units_keep_their_line() {
        let mut files = Files::default();
        files.insert(
            "units/a.toml",
            "# header\n\n[[unit]]\nid = \"one\"\nbase = \"dt:knight\"\nname = \"n\"\n\n[[unit]]\nid = \"two\"\nbase = \"dt:knight\"\nname = \"n\"\n",
        );
        let (pkg, report) = Package::parse(files);
        assert!(report.diagnostics.is_empty(), "{report:?}");
        assert_eq!(pkg.units.len(), 2);
        assert_eq!(pkg.units[0].line, 3);
        assert_eq!(pkg.units[1].line, 8);
        assert_eq!(pkg.units[1].value.id, "two");
    }

    #[test]
    fn invalid_utf8_is_reported() {
        let mut files = Files::default();
        files.insert("texts/en.toml", vec![0xff, 0xfe, b'=']);
        let (pkg, report) = Package::parse(files);
        assert!(report.has_errors());
        assert!(pkg.texts.is_empty());
    }
}
