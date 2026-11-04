//! The units and items of a package (P5): their fields (the format's overrides, each with the base
//! value it overrides), the edit each field makes (`set` or reset), the places that name a unit or
//! an item (`uses`; a delete is refused while there are some), the rename across every file that
//! names it (`rename_plan`, the name keys in the texts too), a new record on a base unit or item
//! (`new_record`), and the prices the studio shows: [`Priced`], the game's content with the
//! package's units and items in it, built once per rebuild on the install (`studio::cost` prices it).
//!
//! What DTMapEdit has that the format cannot hold (per-level gains, nature, magic school and
//! direction, bonuses, icons, upgrades, surrender, starting experience, level multiplier, cost
//! multiplier, gold divisor; for items the stat changes, type, school and bonus) is not here: the
//! package's record takes those from its base (`docs/OPEN_QUESTIONS.md`, package format gaps).

use std::collections::BTreeMap;

use toml_edit::{Document, DocumentMut, Item, Table, Value};

use super::characters::valid_id;
use super::cost::{self, ItemPrice, PriceSkip, UnitPrice};
use super::doc::{entry_id, DocEdit};
use super::texts;
use crate::dt::data::{ArtefactDef, UnitDef};
use crate::dt::install::DtInstall;
use crate::pack::aliases;
use crate::pack::compile::{self, Base};
use crate::pack::content::UnitSpec;
use crate::pack::load::{line_of, Package};
use crate::pack::refs::{DtKind, Ref};
use crate::pack::uses::Use;
use crate::rules::content::Content;
use crate::rules::units::Stats;

/// The two kinds of record a package has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Unit,
    Item,
}

impl Kind {
    /// The table's key in its file: `unit` or `item`.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Unit => "unit",
            Kind::Item => "item",
        }
    }

    /// The folder the records of this kind live in: `units` or `items`.
    pub fn dir(self) -> &'static str {
        match self {
            Kind::Unit => "units",
            Kind::Item => "items",
        }
    }

    /// The `dt:` kind a base of this kind names.
    pub fn dt_kind(self) -> DtKind {
        match self {
            Kind::Unit => DtKind::Unit,
            Kind::Item => DtKind::Item,
        }
    }

    /// The keys a reference of this kind sits under in the package's files: the units a map, a
    /// trigger, a dialog's speaker or a figure names; the items a trigger's condition or action names.
    fn ref_keys(self) -> &'static [&'static str] {
        match self {
            Kind::Unit => &["unit", "troops", "give_unit", "speaker", "hero_figure"],
            Kind::Item => &["item", "has_item", "give_item"],
        }
    }

    /// The key of the record's text name (`unit.<id>.name`).
    fn text_group(self) -> &'static str {
        self.key()
    }
}

/// The override fields of a unit, in the format's order (`docs/reference/rzpack-format.md`).
pub const UNIT_FIELDS: [&str; 15] = [
    "cost",
    "hits",
    "attack_blow",
    "attack_shot",
    "magic_power",
    "defence_blow",
    "defence_shot",
    "protect_life",
    "protect_death",
    "protect_elemental",
    "initiative",
    "manevres",
    "regen",
    "vampirism",
    "evasion",
];

/// The override fields of an item.
pub const ITEM_FIELDS: [&str; 1] = ["cost"];

/// The range a field's value is kept in: the original's window for the stat (records.md §5.1), the
/// 16-bit gold for the price. The format does not check ranges, so these are the forms' limits.
/// `evasion` has no window in the original and no stat of the engine reads it yet.
pub fn field_range(key: &str) -> (i64, i64) {
    match key {
        "cost" => (0, u16::MAX as i64),
        "hits" => (10, 250),
        "initiative" | "manevres" => (1, 250),
        "protect_life" | "protect_death" | "protect_elemental" | "regen" | "vampirism" => (0, 99),
        "evasion" => (0, 1000),
        _ => (0, 250),
    }
}

/// The override a unit's table holds for `key`, if any.
pub fn unit_override(u: &UnitSpec, key: &str) -> Option<i64> {
    let v = match key {
        "cost" => u.cost,
        "hits" => u.hits,
        "attack_blow" => u.attack_blow,
        "attack_shot" => u.attack_shot,
        "magic_power" => u.magic_power,
        "defence_blow" => u.defence_blow,
        "defence_shot" => u.defence_shot,
        "protect_life" => u.protect_life,
        "protect_death" => u.protect_death,
        "protect_elemental" => u.protect_elemental,
        "initiative" => u.initiative,
        "manevres" => u.manevres,
        "regen" => u.regen,
        "vampirism" => u.vampirism,
        "evasion" => u.evasion,
        _ => None,
    };
    v.map(i64::from)
}

/// The value of `key` in a definition (what the override replaces, or what a base shows).
pub fn def_value(d: &UnitDef, key: &str) -> i64 {
    match key {
        "cost" => d.cost as i64,
        "hits" => d.hits as i64,
        "attack_blow" => d.attack_blow as i64,
        "attack_shot" => d.attack_shot as i64,
        "magic_power" => d.magic_power as i64,
        "defence_blow" => d.defence_blow as i64,
        "defence_shot" => d.defence_shot as i64,
        "protect_life" => d.protect_life as i64,
        "protect_death" => d.protect_death as i64,
        "protect_elemental" => d.protect_elemental as i64,
        "initiative" => d.initiative as i64,
        "manevres" => d.manevres as i64,
        "regen" => d.regen as i64,
        "vampirism" => d.vampirism as i64,
        "evasion" => d.evasion.unwrap_or(0) as i64,
        _ => 0,
    }
}

/// One row of a record list: the table's id, its file and line, and its base as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub file: String,
    pub line: u32,
    pub base: String,
    pub name: String,
}

/// The records of `kind` in the package, in file order.
pub fn rows(pkg: &Package, kind: Kind) -> Vec<Row> {
    match kind {
        Kind::Unit => pkg
            .units
            .iter()
            .map(|u| Row { id: u.value.id.clone(), file: u.file.clone(), line: u.line, base: u.value.base.to_string(), name: u.value.name.clone() })
            .collect(),
        Kind::Item => pkg
            .items
            .iter()
            .map(|i| Row { id: i.value.id.clone(), file: i.file.clone(), line: i.line, base: i.value.base.to_string(), name: i.value.name.clone() })
            .collect(),
    }
}

/// Where the package defines record `id` of `kind`: its file and line.
fn defined_at(pkg: &Package, kind: Kind, id: &str) -> Option<(String, u32)> {
    match kind {
        Kind::Unit => pkg.units.iter().find(|u| u.value.id == id).map(|u| (u.file.clone(), u.line)),
        Kind::Item => pkg.items.iter().find(|i| i.value.id == id).map(|i| (i.file.clone(), i.line)),
    }
}

/// The override the record holds for `key`, if any.
pub fn override_of(pkg: &Package, kind: Kind, id: &str, key: &str) -> Option<i64> {
    match kind {
        Kind::Unit => pkg.units.iter().find(|u| u.value.id == id).and_then(|u| unit_override(&u.value, key)),
        Kind::Item => pkg.items.iter().find(|i| i.value.id == id).and_then(|i| i.value.cost.map(i64::from)).filter(|_| key == "cost"),
    }
}

/// The places that name record `id` of `kind`: the base of a record of the same kind, and the keys
/// [`Kind::ref_keys`] of the triggers, dialogs, maps and the other records. Each carries its line and
/// what holds it (`trigger `t``, `dialog `d`, choice `c``, `army `a``). In file and line order.
pub fn uses(pkg: &Package, kind: Kind, id: &str) -> Vec<Use> {
    let mut out = Vec::new();
    let target = Ref::Local { id: id.to_string() };
    let mut paths: Vec<&str> = pkg.files.paths().filter(|p| is_scanned(p)).collect();
    paths.sort_unstable();
    for path in paths {
        let Some(text) = pkg.files.text(path) else { continue };
        let Ok(doc) = Document::parse(text) else { continue };
        let scan = Scan { file: path, text, kind, target: &target, own_dir: path.starts_with(&format!("{}/", kind.dir())) };
        scan_table(doc.as_table(), &scan, "", &mut out);
    }
    out.sort_by(|a, b| (a.file.as_str(), a.line).cmp(&(b.file.as_str(), b.line)));
    out
}

/// The files a reference can sit in: the triggers, dialogs, maps and the records' own folders.
fn is_scanned(path: &str) -> bool {
    (path.starts_with("triggers/") || path.starts_with("dialogs/") || path.starts_with("units/") || path.starts_with("items/")) && path.ends_with(".toml")
        || (path.starts_with("maps/") && path.ends_with("/map.toml"))
}

struct Scan<'a> {
    file: &'a str,
    text: &'a str,
    kind: Kind,
    target: &'a Ref,
    /// The file is in the records' own folder (its `base` names a record of this kind).
    own_dir: bool,
}

fn scan_table(t: &Table, s: &Scan, what: &str, out: &mut Vec<Use>) {
    for (key, item) in t.iter() {
        match item {
            Item::Value(v) => scan_value(v, key, s, what, out),
            Item::Table(sub) => scan_table(sub, s, &join(what, key, None), out),
            Item::ArrayOfTables(a) => {
                for entry in a.iter() {
                    let id = entry.get("id").and_then(Item::as_str);
                    scan_table(entry, s, &join(what, key, id), out);
                }
            }
            Item::None => {}
        }
    }
}

/// What holds a reference: the entry's kind and id, after the enclosing context.
fn join(what: &str, key: &str, id: Option<&str>) -> String {
    let label = match id {
        Some(id) => format!("{key} `{id}`"),
        None => key.to_string(),
    };
    if what.is_empty() {
        label
    } else {
        format!("{what}, {label}")
    }
}

fn scan_value(v: &Value, key: &str, s: &Scan, what: &str, out: &mut Vec<Use>) {
    match v {
        Value::String(text) => {
            let named = Ref::parse(text.value().as_str()).ok().as_ref() == Some(s.target);
            if named && (s.kind.ref_keys().contains(&key) || (key == "base" && s.own_dir)) {
                push(s, v, what, out);
            }
        }
        Value::Array(a) => {
            // `troops = ["dt:…"]`: the elements of a list sit under the list's key.
            for x in a.iter() {
                scan_value(x, key, s, what, out);
            }
        }
        Value::InlineTable(t) => {
            for (k, x) in t.iter() {
                scan_value(x, k, s, what, out);
            }
        }
        _ => {}
    }
}

fn push(s: &Scan, v: &Value, what: &str, out: &mut Vec<Use>) {
    let at = v.span().map_or(0, |r| r.start);
    let what = if what.is_empty() { "the file".to_string() } else { what.to_string() };
    out.push(Use { file: s.file.to_string(), line: line_of(s.text, at), what });
}

/// The rename of record `old` to `new` in every file that names it, as `(path, new text)`. The
/// defining table's `id`, the references (the keys of [`Kind::ref_keys`], a `base` in the records'
/// folder), the name and description keys that carry the id (`unit.<old>.name` becomes
/// `unit.<new>.name`), and the texts' tables of the same name. `text_of` gives a file's current text.
/// Nothing changes when `new` is not an id, is taken, or `old` is not a record.
pub fn rename_plan(pkg: &Package, kind: Kind, old: &str, new: &str, text_of: &dyn Fn(&str) -> Option<String>) -> Result<Vec<(String, String)>, String> {
    if !valid_id(new) {
        return Err(format!("`{new}` is not an id: use lowercase letters, digits and _ only"));
    }
    if old == new {
        return Err("the new id is the same".into());
    }
    if defined_at(pkg, kind, old).is_none() {
        return Err(format!("no {} `{old}`", kind.key()));
    }
    if defined_at(pkg, kind, new).is_some() {
        return Err(format!("a {} `{new}` already exists", kind.key()));
    }
    let mut paths: Vec<&str> = pkg.files.paths().filter(|p| is_scanned(p) || (p.starts_with("texts/") && p.ends_with(".toml"))).collect();
    paths.sort_unstable();
    let mut plan = Vec::new();
    for path in paths {
        let Some(text) = text_of(path) else { continue };
        if let Some(next) = rename_in_text(&text, path, kind, old, new)? {
            plan.push((path.to_string(), next));
        }
    }
    Ok(plan)
}

/// The text of one file with `old` renamed to `new` (`None` when nothing in it changes).
fn rename_in_text(text: &str, path: &str, kind: Kind, old: &str, new: &str) -> Result<Option<String>, String> {
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let mut changed = false;
    let old_ref = Ref::Local { id: old.to_string() };
    let own_dir = path.starts_with(&format!("{}/", kind.dir()));
    let is_texts = path.starts_with("texts/");
    if is_texts {
        rename_text_keys(doc.as_table_mut(), kind.text_group(), old, new, &mut changed);
    } else {
        rename_refs(doc.as_table_mut(), kind, own_dir, &old_ref, old, new, &mut changed);
    }
    Ok(changed.then(|| doc.to_string()))
}

fn rename_refs(t: &mut Table, kind: Kind, own_dir: bool, old_ref: &Ref, old: &str, new: &str, changed: &mut bool) {
    // The defining table of this file: its `id` changes.
    if own_dir {
        if let Some(Item::ArrayOfTables(entries)) = t.get_mut(kind.key()) {
            for e in entries.iter_mut() {
                if e.get("id").and_then(Item::as_str) == Some(old) {
                    set_str(e, "id", new, changed);
                }
            }
        }
    }
    for (key, item) in t.iter_mut() {
        let key = key.get().to_string();
        match item {
            Item::Value(v) => rename_value(v, &key, kind, own_dir, old_ref, old, new, changed),
            Item::Table(sub) => rename_refs(sub, kind, own_dir, old_ref, old, new, changed),
            Item::ArrayOfTables(a) => {
                for sub in a.iter_mut() {
                    rename_refs(sub, kind, own_dir, old_ref, old, new, changed);
                }
            }
            Item::None => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rename_value(v: &mut Value, key: &str, kind: Kind, own_dir: bool, old_ref: &Ref, old: &str, new: &str, changed: &mut bool) {
    match v {
        Value::String(text) => {
            let s = text.value().clone();
            if (kind.ref_keys().contains(&key) || (key == "base" && own_dir)) && Ref::parse(s.as_str()).ok().as_ref() == Some(old_ref) {
                replace_value(v, new, changed);
            } else if own_dir && (key == "name" || key == "description") {
                if let Some(next) = respell(&s, kind.text_group(), old, new) {
                    replace_value(v, &next, changed);
                }
            }
        }
        Value::Array(a) => {
            for x in a.iter_mut() {
                rename_value(x, key, kind, own_dir, old_ref, old, new, changed);
            }
        }
        Value::InlineTable(t) => {
            for (k, x) in t.iter_mut() {
                let k = k.get().to_string();
                rename_value(x, &k, kind, own_dir, old_ref, old, new, changed);
            }
        }
        _ => {}
    }
}

/// The text key with its `group.<old>` part respelled (`marka.unit.ash_knight.name` with `unit`).
fn respell(key: &str, group: &str, old: &str, new: &str) -> Option<String> {
    let mut parts: Vec<&str> = key.split('.').collect();
    let mut hit = false;
    for i in 0..parts.len().saturating_sub(1) {
        if parts[i] == group && parts[i + 1] == old {
            parts[i + 1] = new;
            hit = true;
        }
    }
    hit.then(|| parts.join("."))
}

/// The texts' tables `group.<old>` are moved to `group.<new>`, in every table that holds the group.
fn rename_text_keys(t: &mut Table, group: &str, old: &str, new: &str, changed: &mut bool) {
    if let Some(Item::Table(g)) = t.get_mut(group) {
        if let Some(item) = g.remove(old) {
            g.insert(new, item);
            *changed = true;
        }
    }
    for (_, item) in t.iter_mut() {
        if let Item::Table(sub) = item {
            rename_text_keys(sub, group, old, new, changed);
        }
    }
}

fn set_str(t: &mut Table, key: &str, to: &str, changed: &mut bool) {
    if let Some(v) = t.get_mut(key).and_then(Item::as_value_mut) {
        replace_value(v, to, changed);
    }
}

fn replace_value(v: &mut Value, to: &str, changed: &mut bool) {
    let decor = v.decor().clone();
    *v = Value::from(to);
    *v.decor_mut() = decor;
    *changed = true;
}

/// A new record: its file (`units/<id>.toml`, `items/<id>.toml`), its text, and the texts of its name
/// in every language of the package (the base's name, as a start: the Texts screen edits it).
pub struct NewRecord {
    pub file: String,
    pub text: String,
    pub texts: Vec<(String, DocEdit)>,
}

/// The new record `id` of `kind` on the base `alias` (a `dt:` alias of the kind), its name text
/// `name` in every language. Refused when the id is taken or its file exists, or the alias is not
/// a base of the kind.
pub fn new_record(pkg: &Package, kind: Kind, id: &str, alias: &str, name: &str) -> Result<NewRecord, String> {
    if !valid_id(id) {
        return Err(format!("`{id}` is not an id: use lowercase letters, digits and _ only"));
    }
    if defined_at(pkg, kind, id).is_some() {
        return Err(format!("a {} `{id}` already exists", kind.key()));
    }
    let file = format!("{}/{id}.toml", kind.dir());
    if pkg.files.get(&file).is_some() {
        return Err(format!("{file} already exists"));
    }
    if aliases::resolve(kind.dt_kind(), alias).is_none() {
        return Err(format!("no base `{alias}` of this kind in the game"));
    }
    let base = match kind {
        Kind::Unit => format!("dt:{alias}"),
        Kind::Item => format!("dt:item/{alias}"),
    };
    let key = format!("{}.{id}.name", kind.text_group());
    let text = format!("[[{}]]\nid = \"{id}\"\nbase = \"{base}\"\nname = \"{key}\"\n", kind.key());
    let texts = pkg.texts.keys().map(|lang| texts::set_cell(lang, &key, name)).collect();
    Ok(NewRecord { file, text, texts })
}

/// The edit that sets record `id`'s field `key` to `value` (its file's table), or resets it to the
/// base (`None`: the override is removed).
pub fn set_edit(pkg: &Package, kind: Kind, id: &str, key: &str, value: Option<i64>) -> Result<(String, DocEdit), String> {
    let (file, _) = defined_at(pkg, kind, id).ok_or_else(|| format!("no {} `{id}`", kind.key()))?;
    let path = vec![entry_id(kind.key(), id)];
    let edit = match value {
        Some(v) => DocEdit::Set { path, key: key.to_string(), value: Value::from(v) },
        None => DocEdit::Remove { path, key: key.to_string() },
    };
    Ok((file, edit))
}

/// Why a record cannot be deleted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    NotFound,
    /// Named by these places, which the delete would leave dangling.
    Used(Vec<Use>),
}

/// The edit that deletes record `id` of `kind` from its file, refused while the package names it.
pub fn delete_edit(pkg: &Package, kind: Kind, id: &str) -> Result<(String, DocEdit), Refused> {
    let (file, _) = defined_at(pkg, kind, id).ok_or(Refused::NotFound)?;
    let refs = uses(pkg, kind, id);
    if !refs.is_empty() {
        return Err(Refused::Used(refs));
    }
    let edit = DocEdit::Drop { path: Vec::new(), key: kind.key().to_string(), pick: super::doc::Pick::Id(id.to_string()) };
    Ok((file, edit))
}

/// A base's value of field `key` for record `id`: a `dt:` base's, or a package base's effective value.
pub fn base_value(pkg: &Package, priced: &Priced, kind: Kind, id: &str, key: &str) -> Option<i64> {
    let base = match kind {
        Kind::Unit => pkg.units.iter().find(|u| u.value.id == id)?.value.base.clone(),
        Kind::Item => pkg.items.iter().find(|i| i.value.id == id)?.value.base.clone(),
    };
    match kind {
        Kind::Unit => priced.base_unit(&base).map(|d| def_value(&d, key)),
        Kind::Item => (key == "cost").then(|| priced.base_item(&base)).flatten().map(|a| a.cost as i64),
    }
}

/// The price of a unit or item that cannot be priced, or the skip that leaves it unpriced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemFail {
    Skip(PriceSkip),
    Unresolved(String),
}

/// The game's content with the package's units and items in it, built on the install, with the
/// prices asked for so far. Built once per rebuild (the screens keep it while the package is the same).
pub struct Priced {
    content: Content,
    units: BTreeMap<String, UnitDef>,
    items: BTreeMap<String, ArtefactDef>,
    unresolved: BTreeMap<(Kind, String), String>,
    unit_memo: BTreeMap<String, Result<UnitPrice, String>>,
    item_memo: BTreeMap<String, Result<ItemPrice, ItemFail>>,
}

impl Priced {
    /// The package's records as the game plays them on `install`, its texts in `lang`.
    pub fn new(install: &DtInstall, pkg: &Package, lang: &str) -> Priced {
        let picture = |_: u32| None::<usize>;
        let base = Base { units: &install.units, items: &install.artefacts, picture: &picture };
        let defs = compile::package_defs(pkg, &base, lang);
        let mut units = BTreeMap::new();
        let mut items = BTreeMap::new();
        let mut unresolved = BTreeMap::new();
        for (id, r) in defs.units {
            match r {
                Ok((d, _)) => {
                    units.insert(id, d);
                }
                Err(e) => {
                    unresolved.insert((Kind::Unit, id), e);
                }
            }
        }
        for (id, r) in defs.items {
            match r {
                Ok(a) => {
                    items.insert(id, a);
                }
                Err(e) => {
                    unresolved.insert((Kind::Item, id), e);
                }
            }
        }
        let content = Content::from_dt(install).with_package(units.values().cloned().collect(), items.values().cloned().collect());
        Priced { content, units, items, unresolved, unit_memo: BTreeMap::new(), item_memo: BTreeMap::new() }
    }

    /// The base game's unit of a `dt:` reference, or a package unit's effective definition.
    pub fn base_unit(&self, base: &Ref) -> Option<UnitDef> {
        match base {
            Ref::Dt { kind: DtKind::Unit, alias } => {
                let gi = aliases::resolve(DtKind::Unit, alias)?;
                self.content.units.iter().find(|u| u.id == gi).cloned()
            }
            Ref::Local { id } => self.units.get(id).cloned(),
            _ => None,
        }
    }

    /// The base game's item of a `dt:` reference, or a package item's effective definition.
    pub fn base_item(&self, base: &Ref) -> Option<ArtefactDef> {
        match base {
            Ref::Dt { kind: DtKind::Item, alias } => {
                let gi = aliases::resolve(DtKind::Item, alias)?;
                self.content.items.iter().find(|a| a.id == gi).cloned()
            }
            Ref::Local { id } => self.items.get(id).cloned(),
            _ => None,
        }
    }

    /// The price of unit `id` of the package (memoized).
    pub fn unit_price(&mut self, id: &str) -> Result<UnitPrice, String> {
        if let Some(r) = self.unit_memo.get(id) {
            return r.clone();
        }
        let r = match (self.unresolved.get(&(Kind::Unit, id.to_string())), self.units.get(id)) {
            (Some(e), _) => Err(e.clone()),
            (None, Some(d)) => Ok(cost::unit_price(&self.content, d)),
            (None, None) => Err(format!("no unit `{id}`")),
        };
        self.unit_memo.insert(id.to_string(), r.clone());
        r
    }

    /// The stats of unit `id` at `level` (0 is the base; level L is base + gain × L, the percent
    /// stats by their rule).
    pub fn level_stats(&self, id: &str, level: i32) -> Option<Stats> {
        Some(cost::level_stats(&self.content, self.units.get(id)?, level))
    }

    /// The window's level table of unit `id`: the tactical cost at levels 1 to 5, in percent of the
    /// base's tactical cost.
    pub fn level_percent(&self, id: &str) -> Option<[i64; 5]> {
        Some(cost::level_percent(&self.content, self.units.get(id)?))
    }

    /// The effective definition of unit `id` of the package.
    pub fn unit_def(&self, id: &str) -> Option<&UnitDef> {
        self.units.get(id)
    }

    /// The effective definition of item `id` of the package.
    pub fn item_def(&self, id: &str) -> Option<&ArtefactDef> {
        self.items.get(id)
    }

    /// The automatic price of item `id` of the package (memoized).
    pub fn item_price(&mut self, id: &str) -> Result<ItemPrice, ItemFail> {
        if let Some(r) = self.item_memo.get(id) {
            return r.clone();
        }
        let r = match (self.unresolved.get(&(Kind::Item, id.to_string())), self.items.get(id)) {
            (Some(e), _) => Err(ItemFail::Unresolved(e.clone())),
            (None, Some(a)) => cost::item_price(&self.content, a).map_err(ItemFail::Skip),
            (None, None) => Err(ItemFail::Unresolved(format!("no item `{id}`"))),
        };
        self.item_memo.insert(id.to_string(), r.clone());
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::files::Files;

    fn package(files: &[(&str, &str)]) -> Package {
        let mut f = Files::default();
        for (p, t) in files {
            f.insert(p, *t);
        }
        Package::parse(f).0
    }

    const PKG: &[(&str, &str)] = &[
        ("package.toml", "id = \"p\"\ntitle = \"p.title\"\nversion = \"1\"\n"),
        ("texts/en.toml", "[unit.knight2]\nname = \"K\"\n[item.relic]\nname = \"R\"\n[marka.unit.ash]\nname = \"A\"\n"),
        ("units/heroes.toml", "[[unit]]\nid = \"knight2\"\nbase = \"dt:knight\"\nname = \"unit.knight2.name\"\nhits = 12\n\n[[unit]]\nid = \"ash\"\nbase = \"knight2\"\nname = \"marka.unit.ash.name\"\n"),
        ("items/relics.toml", "[[item]]\nid = \"relic\"\nbase = \"dt:item/viking_sword\"\nname = \"item.relic.name\"\ncost = 50\n"),
        ("triggers/a.toml", "[[trigger]]\nid = \"t\"\nthen = [{ give_unit = \"knight2\", named = \"varga\" }, { give_item = \"relic\" }]\nif = [{ has_item = \"relic\" }]\n"),
        ("dialogs/d.toml", "[[dialog]]\nid = \"d\"\ntitle = \"x\"\ntext = \"x\"\nspeaker = \"knight2\"\n[[dialog.choice]]\nid = \"c\"\ntext = \"x\"\n"),
        ("maps/m/map.toml", "title = \"t\"\n[[army]]\nid = \"a\"\nleader = { unit = \"knight2\", level = 1 }\ntroops = [\"knight2\", \"dt:militiaman\"]\n"),
    ];

    #[test]
    fn uses_finds_units_and_items_where_they_are_named() {
        let pkg = package(PKG);
        let u = uses(&pkg, Kind::Unit, "knight2");
        let got: Vec<(&str, u32)> = u.iter().map(|x| (x.file.as_str(), x.line)).collect();
        assert!(got.contains(&("maps/m/map.toml", 4)) && got.contains(&("maps/m/map.toml", 5)), "the leader and the troops: {got:?}");
        assert!(got.contains(&("dialogs/d.toml", 5)), "the speaker: {got:?}");
        assert!(got.contains(&("triggers/a.toml", 3)), "the give: {got:?}");
        assert!(u.iter().any(|x| x.file == "units/heroes.toml" && x.what.contains("unit `ash`")), "a base in the units: {u:?}");
        let items = uses(&pkg, Kind::Item, "relic");
        assert_eq!(items.iter().map(|x| x.file.as_str()).collect::<Vec<_>>(), ["triggers/a.toml", "triggers/a.toml"], "{items:?}");
        assert!(uses(&pkg, Kind::Unit, "relic").is_empty());
    }

    #[test]
    fn a_delete_is_refused_while_named() {
        let pkg = package(PKG);
        assert!(matches!(delete_edit(&pkg, Kind::Unit, "knight2"), Err(Refused::Used(_))));
        assert!(matches!(delete_edit(&pkg, Kind::Unit, "nobody"), Err(Refused::NotFound)));
        let (file, edit) = delete_edit(&pkg, Kind::Unit, "ash").expect("nothing names ash");
        assert_eq!(file, "units/heroes.toml");
        assert!(matches!(edit, DocEdit::Drop { ref key, .. } if key == "unit"));
    }

    #[test]
    fn the_rename_changes_every_file_that_names_the_record() {
        let pkg = package(PKG);
        let texts = |f: &str| pkg.files.text(f).map(str::to_string);
        let plan = rename_plan(&pkg, Kind::Unit, "knight2", "vanguard", &texts).unwrap();
        let files: Vec<&str> = plan.iter().map(|(f, _)| f.as_str()).collect();
        for f in ["units/heroes.toml", "triggers/a.toml", "dialogs/d.toml", "maps/m/map.toml", "texts/en.toml"] {
            assert!(files.contains(&f), "{f} is in the plan: {files:?}");
        }
        let heroes = &plan.iter().find(|(f, _)| f == "units/heroes.toml").unwrap().1;
        assert!(heroes.contains("id = \"vanguard\"") && heroes.contains("unit.vanguard.name"), "{heroes}");
        assert!(heroes.contains("base = \"vanguard\""), "the other unit's base follows: {heroes}");
        assert!(!heroes.contains("knight2"), "{heroes}");
        let texts_en = &plan.iter().find(|(f, _)| f == "texts/en.toml").unwrap().1;
        assert!(texts_en.contains("[unit.vanguard]") && !texts_en.contains("knight2"), "{texts_en}");
        assert!(rename_plan(&pkg, Kind::Unit, "knight2", "ash", &texts).is_err(), "taken");
        assert!(rename_plan(&pkg, Kind::Unit, "knight2", "Bad", &texts).is_err(), "not an id");
    }

    #[test]
    fn a_new_record_has_its_file_and_its_name_in_every_language() {
        let pkg = package(PKG);
        let n = new_record(&pkg, Kind::Unit, "scout", "militiaman", "Scout").unwrap();
        assert_eq!(n.file, "units/scout.toml");
        assert!(n.text.contains("base = \"dt:militiaman\"") && n.text.contains("name = \"unit.scout.name\""), "{}", n.text);
        assert_eq!(n.texts.len(), 1);
        assert!(new_record(&pkg, Kind::Unit, "ash", "militiaman", "x").is_err(), "taken");
        assert!(new_record(&pkg, Kind::Unit, "scout", "nope_nothing", "x").is_err(), "no such base");
        let item = new_record(&pkg, Kind::Item, "charm", "viking_sword", "Charm").unwrap();
        assert!(item.text.contains("[[item]]") && item.text.contains("base = \"dt:item/viking_sword\""), "{}", item.text);
    }

    #[test]
    fn the_override_of_a_field_is_its_table_value() {
        let pkg = package(PKG);
        assert_eq!(override_of(&pkg, Kind::Unit, "knight2", "hits"), Some(12));
        assert_eq!(override_of(&pkg, Kind::Unit, "knight2", "cost"), None);
        assert_eq!(override_of(&pkg, Kind::Item, "relic", "cost"), Some(50));
        let (file, edit) = set_edit(&pkg, Kind::Unit, "knight2", "hits", Some(14)).unwrap();
        assert_eq!(file, "units/heroes.toml");
        assert!(matches!(edit, DocEdit::Set { ref key, .. } if key == "hits"));
        assert!(matches!(set_edit(&pkg, Kind::Unit, "knight2", "hits", None), Ok((_, DocEdit::Remove { .. }))));
    }
}
