//! The rules of a content package. `check` runs them all and returns the report, starting
//! from what reading the package found (`Package::parse`).

use std::collections::{BTreeMap, BTreeSet};

use rustybuzz::Face;

use super::aliases;
use super::compile::{ITEM_LIMIT, UNIT_LIMIT};
use super::map_check;
use super::content::{ItemSpec, UnitSpec};
use super::diag::Report;
use super::load::{is_game_file, key_line, Located, Package};
use super::manifest::{self, Manifest, BASES, FORMAT, RULESETS};
use super::refs::{valid_id, valid_package_id, DtKind, Ref};

/// Directories a package may have at its top.
const TOP_DIRS: [&str; 8] = ["texts", "units", "items", "maps", "dialogs", "triggers", "art", "scripts"];
/// Files a package may have at its top.
const TOP_FILES: [&str; 3] = ["package.toml", "README.md", "LICENSE"];

/// The TrueType fonts Razdor ships (`data/fonts`): a text's character none of them draws needs a
/// system font to show (`check_text_chars`).
const BUNDLED_FONTS: [&[u8]; 8] = [
    include_bytes!("../../data/fonts/Becker-Medium.ttf"),
    include_bytes!("../../data/fonts/Kurale-Regular.ttf"),
    include_bytes!("../../data/fonts/NotoSans-Regular.ttf"),
    include_bytes!("../../data/fonts/NotoSansDevanagari-Regular.ttf"),
    include_bytes!("../../data/fonts/PT_Sans-Bold.ttf"),
    include_bytes!("../../data/fonts/PT_Sans-Regular.ttf"),
    include_bytes!("../../data/fonts/PT_Serif-Bold.ttf"),
    include_bytes!("../../data/fonts/PT_Serif-Regular.ttf"),
];

/// Every rule of the format, plus the reading errors.
pub fn check(pkg: &Package) -> Report {
    let mut r = pkg.problems.clone();
    check_entries(pkg, &mut r);
    check_manifest(pkg, &mut r);
    check_texts_present(pkg, &mut r);
    check_text_keys(pkg, &mut r);
    check_text_chars(pkg, &mut r);
    let units = index_ids(&pkg.units, |u| u.id.as_str(), "unit", &mut r);
    let items = index_ids(&pkg.items, |i| i.id.as_str(), "item", &mut r);
    check_bases(pkg, &units, &items, &mut r);
    check_cycles(&units, &mut r);
    super::triggers::check(pkg, &mut r);
    check_limits(pkg, &mut r);
    map_check::check_maps(pkg, &mut r);
    r
}

/// The package's units and items must fit the ids a troop slot and an event hold (`compile`).
fn check_limits(pkg: &Package, r: &mut Report) {
    if let Some(extra) = pkg.units.get(UNIT_LIMIT) {
        r.error_hint(
            &extra.file,
            Some(extra.line),
            format!("the package has {} units: {UNIT_LIMIT} at most", pkg.units.len()),
            "a troop slot holds a unit id in one byte, so a package's units are ids 200 to 254",
        );
    }
    if let Some(extra) = pkg.items.get(ITEM_LIMIT) {
        r.error_hint(
            &extra.file,
            Some(extra.line),
            format!("the package has {} items: {ITEM_LIMIT} at most", pkg.items.len()),
            "an event and a hero hold an item id in one byte, so a package's items are ids 200 to 255",
        );
    }
}

/// Rules 6 and 7: the package's top level, and no game files.
fn check_entries(pkg: &Package, r: &mut Report) {
    let mut warned: BTreeSet<&str> = BTreeSet::new();
    for path in pkg.files.paths() {
        if is_game_file(path) {
            r.error_hint(
                path,
                None,
                "original game files are not allowed in a package",
                "remove it: a package refers to the base game with dt: ids and never ships its files",
            );
            continue;
        }
        let (top, nested) = match path.split_once('/') {
            Some((top, _)) => (top, true),
            None => (path, false),
        };
        let known = if nested { TOP_DIRS.contains(&top) } else { TOP_FILES.contains(&top) };
        if !known && warned.insert(top) {
            r.warning_hint(
                top,
                None,
                format!("unknown entry `{top}`: it is not part of the package and is not read"),
                format!("the top level holds: package.toml, README.md, LICENSE, {}", TOP_DIRS.map(|d| format!("{d}/")).join(" ")),
            );
        }
    }
}

/// Rule 1: `package.toml`.
fn check_manifest(pkg: &Package, r: &mut Report) {
    const FILE: &str = "package.toml";
    let Some(m) = &pkg.manifest else {
        if pkg.files.get(FILE).is_none() {
            r.error_hint("", None, "package.toml is missing", "every package has a package.toml at its top");
        }
        return;
    };
    let text = pkg.files.text(FILE).unwrap_or("");
    let line = |key: &str| key_line(text, key);
    if m.format != FORMAT {
        r.error_hint(FILE, line("format"), format!("format is {}; this Razdor reads format {FORMAT}", m.format), format!("set format = {FORMAT}"));
    }
    if m.license != "MIT" {
        r.error_hint(FILE, line("license"), format!("license is \"{}\"; packages are MIT-licensed", m.license), "set license = \"MIT\"");
    }
    if !manifest::valid_version(&m.version) {
        r.error_hint(FILE, line("version"), format!("version \"{}\" is not x.y.z", m.version), "use three numbers, e.g. \"0.1.0\"");
    }
    if !valid_package_id(&m.id) {
        r.error_hint(FILE, line("id"), format!("id \"{}\" is not a package id", m.id), "use lowercase letters, digits and -, e.g. \"hello-story\"");
    }
    if !RULESETS.contains(&m.rules.as_str()) {
        r.error_hint(FILE, line("rules"), format!("rules \"{}\": only the dt ruleset exists yet", m.rules), "set rules = \"dt\" or leave it out");
    }
    if !BASES.contains(&m.base.as_str()) {
        r.warning_hint(
            FILE,
            line("base"),
            format!("base \"{}\" is not one of {}", m.base, BASES.join(", ")),
            "check the name of the base package",
        );
    }
    for req in &m.requires {
        if let Err(e) = manifest::parse_requirement(req) {
            r.error_hint(FILE, line("requires"), format!("requires: {e}"), "write each entry as id@version, e.g. \"elves-pack@^1.0\"");
        }
    }
}

/// Rule 2: at least one language.
fn check_texts_present(pkg: &Package, r: &mut Report) {
    if pkg.texts.is_empty() {
        r.error_hint("", None, "the package has no texts/<lang>.toml", "add texts/en.toml (or any one language) with the texts the package uses");
    }
}

/// Rule 3: every text key used exists, in one language at least and in every language.
fn check_text_keys(pkg: &Package, r: &mut Report) {
    let mut used: Vec<(&str, &str, Option<u32>)> = Vec::new();
    if let Some(m) = &pkg.manifest {
        let text = pkg.files.text("package.toml").unwrap_or("");
        used.push((m.title.as_str(), "package.toml", key_line(text, "title")));
        if let Some(d) = &m.description {
            used.push((d.as_str(), "package.toml", key_line(text, "description")));
        }
    }
    for Located { file, line, value } in &pkg.units {
        used.push((value.name.as_str(), file, Some(*line)));
        if let Some(d) = &value.description {
            used.push((d.as_str(), file, Some(*line)));
        }
    }
    for Located { file, line, value } in &pkg.items {
        used.push((value.name.as_str(), file, Some(*line)));
        if let Some(d) = &value.description {
            used.push((d.as_str(), file, Some(*line)));
        }
    }
    for Located { file, line, value } in &pkg.dialogs {
        used.push((value.title.as_str(), file, Some(*line)));
        used.push((value.text.as_str(), file, Some(*line)));
        for c in &value.choices {
            used.push((c.text.as_str(), file, Some(*line)));
        }
    }
    let map_keys = map_check::text_keys(pkg);
    for (key, file, line) in &map_keys {
        used.push((key.as_str(), file.as_str(), *line));
    }
    let mut seen = BTreeSet::new();
    for (key, file, line) in used {
        if !seen.insert(key) {
            continue;
        }
        let have: Vec<&str> = pkg.texts.iter().filter(|(_, t)| t.contains_key(key)).map(|(l, _)| l.as_str()).collect();
        if have.is_empty() {
            let lang = pkg.texts.keys().next().map_or("en", String::as_str);
            r.error_hint(file, line, format!("text key `{key}` has no text in any language"), format!("add it to texts/{lang}.toml: {}", stanza(key)));
            continue;
        }
        let missing: Vec<&str> = pkg.texts.keys().map(String::as_str).filter(|l| !have.contains(l)).collect();
        if !missing.is_empty() {
            r.warning_hint(
                file,
                line,
                format!("text key `{key}` has no text in {}", missing.join(", ")),
                format!("add it to texts/{}.toml: {}", missing[0], stanza(key)),
            );
        }
    }
}

/// Rule 4 (a warning): a text's characters that none of the bundled fonts draws. The text still
/// shows on a device with a system font for them; one warning per key, in the first language
/// that has the problem.
fn check_text_chars(pkg: &Package, r: &mut Report) {
    let faces: Vec<Face> = BUNDLED_FONTS.iter().filter_map(|d| Face::from_slice(d, 0)).collect();
    let mut warned: BTreeSet<&str> = BTreeSet::new();
    for (lang, texts) in &pkg.texts {
        let file = format!("texts/{lang}.toml");
        let source = pkg.files.text(&file).unwrap_or("");
        for (key, value) in texts {
            if warned.contains(key.as_str()) {
                continue;
            }
            let missing = uncovered(value, &faces);
            if missing.is_empty() {
                continue;
            }
            warned.insert(key.as_str());
            let shown: String = missing.iter().take(5).collect();
            r.warning(
                &file,
                text_line(source, key),
                format!("text \"{key}\" uses characters Razdor's own fonts lack ({shown}): they need a system font and may not show on every device"),
            );
        }
    }
}

/// The characters of `text` that none of `faces` draws, each once, in their order. Whitespace,
/// controls and the invisible joiners need no glyph.
fn uncovered(text: &str, faces: &[Face]) -> Vec<char> {
    let mut out: Vec<char> = Vec::new();
    for c in text.chars() {
        let invisible = matches!(c, '\u{00AD}' | '\u{200B}'..='\u{200F}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}');
        if c.is_whitespace() || c.is_control() || invisible || out.contains(&c) {
            continue;
        }
        if !faces.iter().any(|f| f.glyph_index(c).filter(|g| g.0 != 0).is_some_and(|g| f.glyph_bounding_box(g).is_some())) {
            out.push(c);
        }
    }
    out
}

/// The line of `key` in a texts file: its `name = …` under the `[table]` header of its dotted
/// key (`unit.knight2.name` under `[unit.knight2]`), or the file's top level.
fn text_line(source: &str, key: &str) -> Option<u32> {
    let (table, last) = key.rsplit_once('.').unwrap_or(("", key));
    let mut in_table = table.is_empty();
    for (i, line) in source.lines().enumerate() {
        let t = line.trim();
        if let Some(header) = t.strip_prefix('[') {
            in_table = header.trim_end_matches(']').trim() == table;
        } else if in_table && t.strip_prefix(last).is_some_and(|rest| rest.trim_start().starts_with('=')) {
            return Some(i as u32 + 1);
        }
    }
    None
}

/// The TOML for a dotted key, as the author writes it (`[unit.x]` and `name = "…"`).
fn stanza(key: &str) -> String {
    match key.rsplit_once('.') {
        Some((table, last)) => format!("[{table}] {last} = \"…\""),
        None => format!("{key} = \"…\""),
    }
}

/// Rule 4: ids are valid and unique per kind. Returns the first definition of each valid id.
fn index_ids<'a, T>(items: &'a [Located<T>], id: impl Fn(&T) -> &str, kind: &str, r: &mut Report) -> BTreeMap<&'a str, &'a Located<T>> {
    let mut first: BTreeMap<&str, &Located<T>> = BTreeMap::new();
    for item in items {
        let name = id(&item.value);
        if !valid_id(name) {
            r.error_hint(&item.file, Some(item.line), format!("{kind} id \"{name}\" is not valid"), "use lowercase letters, digits and _, e.g. dawn_knight");
            continue;
        }
        match first.get(name) {
            Some(prev) => r.error_hint(
                &item.file,
                Some(item.line),
                format!("duplicate {kind} id `{name}`"),
                format!("already defined at {}:{}", prev.file, prev.line),
            ),
            None => {
                first.insert(name, item);
            }
        }
    }
    first
}

/// The rule 5 scope of one kind of content.
struct Scope<'a> {
    kind: DtKind,
    /// `unit` or `item`, and its article (`a`, `an`).
    what: &'static str,
    article: &'static str,
    ids: &'a BTreeSet<&'a str>,
    manifest: Option<&'a Manifest>,
}

/// Rule 5: every base resolves.
fn check_bases(pkg: &Package, units: &BTreeMap<&str, &Located<UnitSpec>>, items: &BTreeMap<&str, &Located<ItemSpec>>, r: &mut Report) {
    let unit_ids: BTreeSet<&str> = units.keys().copied().collect();
    let item_ids: BTreeSet<&str> = items.keys().copied().collect();
    let unit_scope = Scope { kind: DtKind::Unit, what: "unit", article: "a", ids: &unit_ids, manifest: pkg.manifest.as_ref() };
    for u in &pkg.units {
        check_base(&unit_scope, &u.file, u.line, &u.value.base, r);
    }
    let item_scope = Scope { kind: DtKind::Item, what: "item", article: "an", ids: &item_ids, manifest: pkg.manifest.as_ref() };
    for i in &pkg.items {
        check_base(&item_scope, &i.file, i.line, &i.value.base, r);
    }
}

fn check_base(scope: &Scope, file: &str, line: u32, base: &Ref, r: &mut Report) {
    let what = scope.what;
    match base {
        Ref::Dt { kind, alias } if *kind != scope.kind => {
            let hint = if aliases::resolve(scope.kind, alias).is_some() {
                format!("did you mean `{}`?", Ref::Dt { kind: scope.kind, alias: alias.clone() })
            } else {
                suggestions_hint(scope.kind, alias)
            };
            let article = scope.article;
            r.error_hint(file, Some(line), format!("{article} {what} can only be based on {article} {what}, not on `{base}`"), hint);
        }
        Ref::Dt { kind, alias } => {
            if aliases::resolve(*kind, alias).is_none() {
                r.error_hint(file, Some(line), format!("unknown base `{base}`: no {what} has the alias `{alias}`"), suggestions_hint(*kind, alias));
            }
        }
        Ref::Local { id } => {
            if !scope.ids.contains(id.as_str()) {
                r.error_hint(
                    file,
                    Some(line),
                    format!("base `{id}`: no {what} with this id in the package"),
                    format!("define it in {what}s/*.toml, or base on a dt: alias"),
                );
            }
        }
        Ref::Package { package, id } => {
            let Some(m) = scope.manifest else { return };
            if package == &m.id {
                r.error_hint(file, Some(line), format!("base `{base}` names this package"), format!("write it as `{id}`"));
            } else if !m.requires.iter().any(|q| manifest::parse_requirement(q).is_ok_and(|q| &q.package == package)) {
                r.error_hint(
                    file,
                    Some(line),
                    format!("base `{base}`: package `{package}` is not in requires"),
                    format!("add \"{package}@^1.0\" to requires in package.toml"),
                );
            }
        }
    }
}

/// The hint for an unknown `dt:` alias: the closest aliases, or where the table is.
fn suggestions_hint(kind: DtKind, alias: &str) -> String {
    let near = aliases::suggest(kind, alias);
    if near.is_empty() {
        "no alias is close; the table is data/pack/dt-aliases.toml (Latin names of the English titles)".into()
    } else {
        format!("closest: {}", near.join(", "))
    }
}

/// Base chains of units: a unit based on units based on … on itself is an error.
fn check_cycles(units: &BTreeMap<&str, &Located<UnitSpec>>, r: &mut Report) {
    let next: BTreeMap<&str, &str> = units
        .iter()
        .filter_map(|(id, u)| match &u.value.base {
            Ref::Local { id: base } => Some((*id, base.as_str())),
            _ => None,
        })
        .collect();
    let mut done: BTreeSet<&str> = BTreeSet::new();
    for &start in next.keys() {
        if done.contains(start) {
            continue;
        }
        let mut path: Vec<&str> = Vec::new();
        let mut cur = Some(start);
        while let Some(c) = cur {
            if done.contains(c) {
                break;
            }
            if let Some(pos) = path.iter().position(|p| *p == c) {
                let mut cycle: Vec<&str> = path[pos..].to_vec();
                cycle.push(c);
                if let Some(u) = units.get(c) {
                    r.error_hint(&u.file, Some(u.line), format!("base cycle: {}", cycle.join(" -> ")), "a unit cannot be based on itself, directly or through others");
                }
                break;
            }
            path.push(c);
            cur = next.get(c).copied();
        }
        done.extend(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::diag::{Diagnostic, Severity};
    use crate::pack::files::Files;

    fn package(files: &[(&str, &str)]) -> Package {
        let mut f = Files::default();
        for (path, text) in files {
            f.insert(path, text.as_bytes());
        }
        Package::parse(f).0
    }

    /// A valid package with one unit and one item; `extra` adds or replaces files.
    fn with(extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let base = [
            ("package.toml", MANIFEST),
            ("texts/en.toml", "[story]\ntitle = \"Story\"\n\n[unit.knight2]\nname = \"Knight\"\n\n[item.relic]\nname = \"Relic\"\n"),
            ("units/heroes.toml", "[[unit]]\nid = \"knight2\"\nbase = \"dt:knight\"\nname = \"unit.knight2.name\"\nhits = 12\n"),
            ("items/relics.toml", "[[item]]\nid = \"relic\"\nbase = \"dt:item/viking_sword\"\nname = \"item.relic.name\"\n"),
        ];
        let mut out: Vec<(String, String)> = base.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect();
        for (p, t) in extra {
            out.retain(|(q, _)| q.as_str() != *p);
            if !t.is_empty() {
                out.push((p.to_string(), t.to_string()));
            }
        }
        out
    }

    const MANIFEST: &str = "id = \"demo\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"story.title\"\nauthor = \"Razdor\"\nlicense = \"MIT\"\n";

    fn check_pairs(pairs: &[(String, String)]) -> Report {
        let refs: Vec<(&str, &str)> = pairs.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        check(&package(&refs))
    }

    fn errors(r: &Report) -> Vec<String> {
        r.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.message.clone()).collect()
    }

    fn assert_clean(r: &Report) {
        assert!(r.diagnostics.is_empty(), "{}", r.to_text());
    }

    #[test]
    fn a_character_no_bundled_font_draws_is_a_warning_once_per_key() {
        // 中 is in no bundled font; Cyrillic and Latin are, so the default package stays clean.
        assert_clean(&check_pairs(&with(&[])));
        let en = "[story]\ntitle = \"Story\"\n\n[unit.knight2]\nname = \"Рыцарь 中文中\"\n\n[item.relic]\nname = \"Relic\"\n";
        // The same key in a second language is not a second warning.
        let ru = "[unit.knight2]\nname = \"Рыцарь 中\"\n";
        let r = check_pairs(&with(&[("texts/en.toml", en), ("texts/ru.toml", ru)]));
        assert!(!r.has_errors(), "{}", r.to_text());
        let warned: Vec<&Diagnostic> = r.diagnostics.iter().filter(|d| d.message.contains("uses characters")).collect();
        assert_eq!(warned.len(), 1, "{}", r.to_text());
        assert_eq!(warned[0].severity, Severity::Warning);
        assert_eq!(warned[0].file, "texts/en.toml");
        assert_eq!(warned[0].line, Some(5));
        assert_eq!(
            warned[0].message,
            "text \"unit.knight2.name\" uses characters Razdor's own fonts lack (中文): they need a system font and may not show on every device"
        );
    }

    #[test]
    fn the_example_passes_clean() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/hello-story");
        let pkg = Package::parse(Files::from_dir(&dir).unwrap()).0;
        assert_clean(&check(&pkg));
    }

    #[test]
    fn the_test_package_is_clean_before_each_break() {
        assert_clean(&check_pairs(&with(&[])));
    }

    #[test]
    fn an_unknown_dt_alias_is_an_error_with_a_hint() {
        let r = check_pairs(&with(&[("units/heroes.toml", "[[unit]]\nid = \"knight2\"\nbase = \"dt:knigth\"\nname = \"unit.knight2.name\"\n")]));
        assert!(errors(&r).iter().any(|m| m.contains("unknown base `dt:knigth`")), "{}", r.to_text());
        let hint = r.diagnostics[0].hint.clone().unwrap_or_default();
        assert!(hint.contains("dt:knight"), "{hint}");
    }

    #[test]
    fn a_unit_alias_for_an_item_is_an_error() {
        let r = check_pairs(&with(&[("items/relics.toml", "[[item]]\nid = \"relic\"\nbase = \"dt:knight\"\nname = \"item.relic.name\"\n")]));
        let errs = errors(&r);
        assert_eq!(errs.len(), 1, "{}", r.to_text());
        assert!(errs[0].contains("an item can only be based on an item"), "{errs:?}");
        assert!(r.diagnostics[0].hint.is_some());
    }

    #[test]
    fn a_missing_key_in_every_language_is_an_error() {
        let r = check_pairs(&with(&[("items/relics.toml", "[[item]]\nid = \"relic\"\nbase = \"dt:item/viking_sword\"\nname = \"item.gone.name\"\n")]));
        assert!(r.diagnostics.iter().any(|d| d.severity == Severity::Error && d.message.contains("item.gone.name") && d.message.contains("any language")), "{}", r.to_text());
    }

    #[test]
    fn a_key_missing_in_one_language_is_a_warning() {
        let pairs = with(&[
            ("texts/ru.toml", "[story]\ntitle = \"История\"\n"),
            ("items/relics.toml", "[[item]]\nid = \"relic\"\nbase = \"dt:item/viking_sword\"\nname = \"item.relic.name\"\n"),
        ]);
        let r = check_pairs(&pairs);
        assert!(!r.has_errors(), "{}", r.to_text());
        assert!(r.diagnostics.iter().any(|d| d.severity == Severity::Warning && d.message.contains("ru")), "{}", r.to_text());
    }

    #[test]
    fn a_licence_other_than_mit_is_an_error() {
        let manifest = MANIFEST.replace("MIT", "GPL-3.0-only");
        let r = check_pairs(&with(&[("package.toml", &manifest)]));
        assert!(errors(&r).iter().any(|m| m.contains("MIT-licensed")), "{}", r.to_text());
    }

    #[test]
    fn a_duplicate_unit_id_is_an_error() {
        let pairs = with(&[("units/more.toml", "[[unit]]\nid = \"knight2\"\nbase = \"dt:knight\"\nname = \"unit.knight2.name\"\n")]);
        let r = check_pairs(&pairs);
        let d = r.diagnostics.iter().find(|d| d.message.contains("duplicate unit id")).expect("reported");
        assert_eq!(d.file, "units/more.toml");
        assert_eq!(d.line, Some(1));
        assert!(d.hint.clone().unwrap_or_default().contains("units/heroes.toml:1"));
    }

    #[test]
    fn a_base_cycle_is_an_error() {
        let units = "[[unit]]\nid = \"a\"\nbase = \"b\"\nname = \"unit.knight2.name\"\n\n[[unit]]\nid = \"b\"\nbase = \"a\"\nname = \"unit.knight2.name\"\n";
        let r = check_pairs(&with(&[("units/heroes.toml", units)]));
        assert!(errors(&r).iter().any(|m| m.starts_with("base cycle: a -> b -> a") || m.starts_with("base cycle: b -> a -> b")), "{}", r.to_text());
    }

    #[test]
    fn a_unit_based_on_a_local_unit_is_fine() {
        let units = "[[unit]]\nid = \"a\"\nbase = \"dt:knight\"\nname = \"unit.knight2.name\"\n\n[[unit]]\nid = \"b\"\nbase = \"a\"\nname = \"unit.knight2.name\"\n";
        assert_clean(&check_pairs(&with(&[("units/heroes.toml", units)])));
    }

    #[test]
    fn another_package_must_be_in_requires() {
        let units = "[[unit]]\nid = \"elf\"\nbase = \"elves-pack:elf_archer\"\nname = \"unit.knight2.name\"\n";
        let r = check_pairs(&with(&[("units/heroes.toml", units)]));
        assert!(errors(&r).iter().any(|m| m.contains("package `elves-pack` is not in requires")), "{}", r.to_text());
        let manifest = format!("{MANIFEST}requires = [\"elves-pack@^1.0\"]\n");
        let r = check_pairs(&with(&[("package.toml", &manifest), ("units/heroes.toml", units)]));
        assert_clean(&r);
    }

    #[test]
    fn a_game_file_is_an_error() {
        let r = check_pairs(&with(&[("maps/one.DTm", "x")]));
        assert!(errors(&r).iter().any(|m| m.contains("original game files are not allowed")), "{}", r.to_text());
        let r = check_pairs(&with(&[("texts/Rus_Units.ini", "x")]));
        assert!(r.has_errors());
        let r = check_pairs(&with(&[("art/Persones.UGS", "x")]));
        assert!(r.has_errors(), "{}", r.to_text());
    }

    #[test]
    fn an_unknown_top_level_entry_is_a_warning() {
        let r = check_pairs(&with(&[("notes/todo.md", "x")]));
        assert!(!r.has_errors(), "{}", r.to_text());
        assert!(r.diagnostics.iter().any(|d| d.severity == Severity::Warning && d.file == "notes"), "{}", r.to_text());
    }

    #[test]
    fn the_manifest_rules() {
        let bad = MANIFEST.replace("format = 1", "format = 2").replace("version = \"0.1.0\"", "version = \"0.1\"");
        let r = check_pairs(&with(&[("package.toml", &bad)]));
        let errs = errors(&r);
        assert!(errs.iter().any(|m| m.contains("format is 2")), "{errs:?}");
        assert!(errs.iter().any(|m| m.contains("not x.y.z")), "{errs:?}");
        let rules = format!("{MANIFEST}rules = \"lua\"\n");
        assert!(errors(&check_pairs(&with(&[("package.toml", &rules)]))).iter().any(|m| m.contains("only the dt ruleset")));
        let base = format!("{MANIFEST}base = \"other-mod\"\n");
        let r = check_pairs(&with(&[("package.toml", &base)]));
        assert!(!r.has_errors() && r.warnings() == 1, "{}", r.to_text());
    }

    #[test]
    fn a_package_with_no_texts_is_an_error() {
        let pairs: Vec<(String, String)> = with(&[]).into_iter().filter(|(p, _)| !p.starts_with("texts/")).collect();
        let r = check_pairs(&pairs);
        assert!(errors(&r).iter().any(|m| m.contains("no texts/<lang>.toml")));
    }

    #[test]
    fn the_syntax_error_line_is_reported() {
        let r = check_pairs(&with(&[("units/heroes.toml", "[[unit]]\nid = \"knight2\"\nbase = \"dt:knight\"\nname = \n")]));
        let d = r.diagnostics.iter().find(|d| d.file == "units/heroes.toml").expect("reported");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.line, Some(4));
    }

    #[test]
    fn a_zip_path_out_of_the_package_is_an_error() {
        let bytes = zip_of(&[("package.toml", MANIFEST), ("../evil.toml", "x")]);
        let files = Files::from_zip(&bytes).unwrap();
        let r = check(&Package::parse(files).0);
        assert!(r.diagnostics.iter().any(|d| d.severity == Severity::Error && d.file == "../evil.toml"), "{}", r.to_text());
    }

    fn zip_of(entries: &[(&str, &str)]) -> Vec<u8> {
        let bytes: Vec<(&str, &[u8])> = entries.iter().map(|(n, t)| (*n, t.as_bytes())).collect();
        zip_bytes(&bytes)
    }

    /// A zip of files as bytes (a picture is not text).
    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn a_zip_round_trip_matches_the_folder() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/hello-story");
        let folder = Files::from_dir(&dir).unwrap();
        let entries: Vec<(&str, &[u8])> = folder.iter().collect();
        let bytes = zip_bytes(&entries);
        let from_zip = Files::from_zip(&bytes).unwrap();
        assert_eq!(from_zip, folder);
        assert_eq!(check(&Package::parse(from_zip).0), check(&Package::parse(folder).0));
    }
}
