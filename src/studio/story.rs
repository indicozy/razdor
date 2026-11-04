//! The story editor's edits (P4): the trigger and dialog forms and the flow graph's wiring, over
//! `triggers/*.toml` and `dialogs/*.toml`, written through `toml_edit` (`studio::doc`).
//!
//! The kinds come from the vocabulary (`pack::triggers::{EVENTS, CONDITIONS, ACTIONS}`). What the
//! forms need beyond the names is each kind's arguments: [`spec`] lists them (their keys and the
//! type of each value, [`Arg`]), and a test reads every kind back through the format's own parsers,
//! so a kind or key the vocabulary lacks cannot go unnoticed. A row (one `when`, `if` or `then`
//! entry) is an inline table: its kind key and its arguments. A row is valid when the format's
//! parser reads it ([`validate`]); the forms refuse what it refuses.
//!
//! A list of rows (`if`, `then`) is written as a whole array: its order is the order of the rows,
//! and the comments inside an inline array are not kept (a limit: the rest of the file is).

use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

use super::doc::{entry_id, DocEdit, Hop, Pick};
use super::edit::array_value;
use super::texts as tx;
use crate::pack::load::Package;
use crate::pack::triggers::{self as pt, Event, Problem};

/// The sections of the vocabulary: a trigger's event, a condition, an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Event,
    Cond,
    Action,
}

impl Section {
    /// The kinds of the section, in the vocabulary's order.
    pub fn kinds(self) -> &'static [&'static str] {
        match self {
            Section::Event => &pt::EVENTS,
            Section::Cond => &pt::CONDITIONS,
            Section::Action => &pt::ACTIONS,
        }
    }

    /// The key that holds the kind: `event` for a `when`, the kind's own name otherwise.
    pub fn kind_key(self, row: &InlineTable) -> Option<String> {
        match self {
            Section::Event => row.get("event").and_then(Value::as_str).map(str::to_string),
            _ => self.kinds().iter().find(|k| row.contains_key(k)).map(|k| k.to_string()),
        }
    }
}

/// The type of a value in a row: what the form shows for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    /// A whole number in `lo..=hi`.
    Int(i64, i64),
    Bool,
    /// The fixed `true` (`victory = true`, `army_goal … clear = true`).
    True,
    /// A cell `[x, y]`.
    Cell,
    /// A cell `[x, y]` or an area `[x, y, w, h]`.
    Place,
    /// A list of cells `[[x, y], …]` (a patrol).
    Cells,
    /// A variable of the package.
    Var,
    /// A variable's value: a whole number or `true`/`false`, as the variable is declared.
    VarValue,
    Army,
    Building,
    Dialog,
    /// A choice of the dialog that the row's own `dialog` key names.
    Choice,
    Item,
    Unit,
    /// A list of units, comma separated (`dt:` aliases or the package's own ids).
    Units,
    Spell,
    Map,
    /// A named character's id.
    Named,
    Faction,
    /// A side (player, ally, neighbour, enemy), as a relation's `towards`.
    Side,
    /// A comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`.
    Cmp,
    /// An army model's name (`bandits`, …).
    Model,
    /// An army or a building by its id: the row's `of` says which (`army` by default).
    Entity,
    /// `army` or `building`.
    EntityKind,
    /// A holder: `player`, `pack`, an army's id.
    Holder,
    /// The holder a row's positional value names: `player`, `pack`, an army's id, or `garrison`
    /// (then the row's `garrison` key names the building).
    PosHolder,
    /// A figure: `default`, a unit, or an army model (`{ model = … }`).
    Figure,
    /// A goal's target: `hero` or an army's id.
    Target,
    /// A text: an id, a word, a rule's name.
    Text,
    /// A text key of the package's texts.
    TextKey,
    /// An army's id that a `spawn_army` names (a new one, typed).
    NewArmy,
}

/// One argument of a kind: its key, its type, whether it may be left out, and the group of
/// alternatives it belongs to (one member of a group is in the row at a time; a group whose members
/// are optional may be empty).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgSpec {
    pub key: &'static str,
    pub arg: Arg,
    pub optional: bool,
    pub group: Option<&'static str>,
}

/// A kind's arguments, and the type of its positional value (the value under the kind's own key;
/// none for an event, whose kind is the value of `event`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindSpec {
    pub kind: &'static str,
    pub positional: Option<Arg>,
    pub args: &'static [ArgSpec],
}

const fn req(key: &'static str, arg: Arg) -> ArgSpec {
    ArgSpec { key, arg, optional: false, group: None }
}

const fn opt(key: &'static str, arg: Arg) -> ArgSpec {
    ArgSpec { key, arg, optional: true, group: None }
}

const fn grp(key: &'static str, arg: Arg, group: &'static str) -> ArgSpec {
    ArgSpec { key, arg, optional: false, group: Some(group) }
}

const fn grp_opt(key: &'static str, arg: Arg, group: &'static str) -> ArgSpec {
    ArgSpec { key, arg, optional: true, group: Some(group) }
}

const fn k(kind: &'static str, positional: Option<Arg>, args: &'static [ArgSpec]) -> KindSpec {
    KindSpec { kind, positional, args }
}

const HOLDER: &str = "holder";

static EVENT_SPECS: &[KindSpec] = &[
    k("game_start", None, &[]),
    k("day_start", None, &[]),
    k("week_start", None, &[]),
    k("new_moon", None, &[]),
    k("hero_enters", None, &[req("at", Arg::Place)]),
    k("hero_enters_building", None, &[req("building", Arg::Building)]),
    k("army_defeated", None, &[req("army", Arg::Army)]),
    k("building_captured", None, &[req("building", Arg::Building)]),
    k("building_lost", None, &[req("building", Arg::Building)]),
    k("sim_victory", None, &[]),
    k("dialog_choice", None, &[req("dialog", Arg::Dialog), req("choice", Arg::Choice)]),
    k("var_changed", None, &[req("var", Arg::Var)]),
    k("army_arrives", None, &[req("army", Arg::Army), grp_opt("at", Arg::Cell, "where"), grp_opt("building", Arg::Building, "where")]),
];

static COND_SPECS: &[KindSpec] = &[
    k("var", Some(Arg::Var), &[opt("op", Arg::Cmp), req("value", Arg::VarValue)]),
    k("random", Some(Arg::Int(0, 100)), &[]),
    k("hero_at", Some(Arg::Place), &[]),
    k("army_at", Some(Arg::Army), &[req("at", Arg::Place)]),
    k("has_named_unit", Some(Arg::Named), &[req("unit", Arg::Unit), grp(HOLDER_IN, Arg::Holder, HOLDER), grp("garrison", Arg::Building, HOLDER), opt("value", Arg::Bool)]),
    k("has_item", Some(Arg::Item), &[grp(HOLDER_IN, Arg::Holder, HOLDER), grp("garrison", Arg::Building, HOLDER)]),
    k("gold", Some(Arg::Int(-1_000_000, 1_000_000)), &[opt("op", Arg::Cmp)]),
    k("army_alive", Some(Arg::Army), &[opt("value", Arg::Bool)]),
    k("building_owner", Some(Arg::Building), &[req("faction", Arg::Faction)]),
    k("has_spell", Some(Arg::Spell), &[grp(HOLDER_IN, Arg::Holder, HOLDER), grp("garrison", Arg::Building, HOLDER), opt("all", Arg::Bool)]),
];

const HOLDER_IN: &str = "in";

static ACTION_SPECS: &[KindSpec] = &[
    k("set", Some(Arg::Var), &[req("value", Arg::VarValue)]),
    k("add", Some(Arg::Var), &[req("value", Arg::Int(-1_000_000, 1_000_000)), opt("min", Arg::Int(-1_000_000, 1_000_000)), opt("max", Arg::Int(-1_000_000, 1_000_000))]),
    k("random_var", Some(Arg::Var), &[req("min", Arg::Int(-1_000_000, 1_000_000)), req("max", Arg::Int(-1_000_000, 1_000_000))]),
    k("dialog", Some(Arg::Dialog), &[]),
    k("give_gold", Some(Arg::Int(-1_000_000, 1_000_000)), &[]),
    k("give_item", Some(Arg::Item), &[grp("to", Arg::Holder, HOLDER), grp("garrison", Arg::Building, HOLDER)]),
    k("give_unit", Some(Arg::Unit), &[opt("named", Arg::Named)]),
    k("give_xp", Some(Arg::Int(0, 1_000_000)), &[grp("to", Arg::Holder, HOLDER), grp("garrison", Arg::Building, HOLDER)]),
    k("spawn_army", Some(Arg::NewArmy), &[req("at", Arg::Cell), req("faction", Arg::Faction), opt("model", Arg::Int(0, 12)), req("troops", Arg::Units)]),
    k("remove_army", Some(Arg::Army), &[]),
    k("start_battle", Some(Arg::Army), &[req("rules", Arg::Text)]),
    k("teleport_hero", Some(Arg::Cell), &[]),
    k("teleport_army", Some(Arg::Army), &[grp("at", Arg::Cell, "dest"), grp("to", Arg::Building, "dest")]),
    k("set_faction", Some(Arg::Entity), &[req("faction", Arg::Faction), opt("of", Arg::EntityKind)]),
    k("set_relation", Some(Arg::Entity), &[req("towards", Arg::Side), req("value", Arg::Int(-3, 3)), opt("of", Arg::EntityKind)]),
    k("add_units", Some(Arg::Entity), &[req("troops", Arg::Units), opt("of", Arg::EntityKind), opt("garrison", Arg::Building)]),
    k("cast_on_army", Some(Arg::Spell), &[grp(HOLDER_IN, Arg::Holder, HOLDER), grp("garrison", Arg::Building, HOLDER), grp("minutes", Arg::Int(1, 1_000_000), "until"), grp("permanent", Arg::True, "until")]),
    k("remove_spells", Some(Arg::PosHolder), &[opt("garrison", Arg::Building)]),
    k("remove_spell_from_book", Some(Arg::Spell), &[]),
    k("victory", Some(Arg::True), &[opt("next", Arg::Map)]),
    k("defeat", Some(Arg::True), &[]),
    k(
        "army_goal",
        Some(Arg::Army),
        &[grp("go_to", Arg::Cell, "goal"), grp("go_to_building", Arg::Building, "goal"), grp("follow", Arg::Target, "goal"), grp("patrol", Arg::Cells, "goal"), grp("attack", Arg::Target, "goal"), grp("clear", Arg::True, "goal")],
    ),
    k("army_speed", Some(Arg::Army), &[req("percent", Arg::Int(10, 1000))]),
    k("hero_figure", Some(Arg::Figure), &[]),
    k("army_figure", Some(Arg::Army), &[grp("unit", Arg::Unit, "figure"), grp("model", Arg::Model, "figure")]),
    k("remove_unit", Some(Arg::Named), &[grp(HOLDER_IN, Arg::Holder, HOLDER), grp("garrison", Arg::Building, HOLDER)]),
    k("travel", Some(Arg::Map), &[opt("at", Arg::Text)]),
    k("ending", Some(Arg::Dialog), &[]),
    k("quest", Some(Arg::Text), &[req("title", Arg::TextKey), req("text", Arg::TextKey)]),
    k("complete_quest", Some(Arg::Text), &[]),
    k("restore_army", Some(Arg::Holder), &[]),
];

/// The arguments of a kind, from the vocabulary's section. `None` when the section has no such kind.
pub fn spec(section: Section, kind: &str) -> Option<&'static KindSpec> {
    let table = match section {
        Section::Event => EVENT_SPECS,
        Section::Cond => COND_SPECS,
        Section::Action => ACTION_SPECS,
    };
    table.iter().find(|s| s.kind == kind)
}

/// One row of a list (a `when`, an `if` or a `then`): an inline table.
pub type Row = InlineTable;

/// The row's value as the format's parser reads it. An error is the parser's message and hint.
pub fn validate(section: Section, row: &Row) -> Result<(), Problem> {
    let text = format!("x = {row}");
    let table: toml::Table = toml::from_str(&text).map_err(|e| Problem { message: e.to_string(), hint: String::new() })?;
    let Some(t) = table.get("x").and_then(|v| v.as_table()) else {
        return Err(Problem { message: "a row is a table".into(), hint: String::new() });
    };
    match section {
        Section::Event => pt::parse_event(t).map(|_| ()),
        Section::Cond => pt::parse_cond(t).map(|_| ()),
        Section::Action => pt::parse_action(t).map(|_| ()),
    }
}

/// A name a trigger or a dialog takes: lowercase letters, digits and `_` (the format's id rule).
pub fn is_name(s: &str) -> bool {
    crate::pack::refs::valid_id(s)
}

/// The names the forms offer: the package's variables, its armies (the map's and the spawned), the
/// map's buildings, the maps, the dialogs and their choices, the items, the units, the spells and the
/// named characters the story names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// The variables with their declared kind (`true` for a whole number).
    pub vars: Vec<(String, bool)>,
    pub armies: Vec<String>,
    pub buildings: Vec<String>,
    pub maps: Vec<String>,
    pub dialogs: Vec<String>,
    /// Each dialog's choice ids, by dialog.
    pub choices: Vec<(String, Vec<String>)>,
    pub items: Vec<String>,
    pub units: Vec<String>,
    pub spells: Vec<String>,
    pub named: Vec<String>,
}

impl Options {
    /// The options of a map of the package (`map` names the map whose armies and buildings are
    /// offered; the others are the package's).
    pub fn of(pkg: &Package, map: &str) -> Options {
        let mut vars: Vec<(String, bool)> = pkg.vars.iter().map(|v| (v.value.name.clone(), v.value.kind == pt::VarKind::Int)).collect();
        vars.sort();
        let mut o = Options { vars, ..Options::default() };
        if let Some(spec) = pkg.maps.get(map).and_then(|m| m.spec.as_ref()) {
            o.armies = spec.army.iter().map(|a| a.get_ref().id.clone()).collect();
            o.buildings = spec.building.iter().map(|b| b.get_ref().id.clone()).collect();
        }
        let mut spawned: Vec<&pt::Action> = pkg.triggers.iter().flat_map(|t| t.value.then.iter()).collect();
        spawned.extend(pkg.dialogs.iter().flat_map(|d| d.value.choices.iter()).flat_map(|c| c.then.iter()));
        for a in &spawned {
            if let pt::Action::SpawnArmy { army, .. } = a {
                o.armies.push(army.clone());
            }
        }
        let mut named: Vec<String> = Vec::new();
        for t in pkg.triggers.iter().map(|t| &t.value) {
            for c in &t.cond {
                if let pt::Cond::HasNamed { named: n, .. } = c {
                    named.push(n.clone());
                }
            }
            for a in &t.then {
                named_of_action(a, &mut named);
            }
        }
        for d in &pkg.dialogs {
            for c in &d.value.choices {
                for a in &c.then {
                    named_of_action(a, &mut named);
                }
                for cond in &c.cond {
                    if let pt::Cond::HasNamed { named: n, .. } = cond {
                        named.push(n.clone());
                    }
                }
            }
        }
        named.sort();
        named.dedup();
        o.named = named;
        o.armies.sort();
        o.armies.dedup();
        o.buildings.sort();
        o.maps = pkg.maps.keys().cloned().collect();
        o.dialogs = pkg.dialogs.iter().map(|d| d.value.id.clone()).collect();
        o.choices = pkg.dialogs.iter().map(|d| (d.value.id.clone(), d.value.choices.iter().map(|c| c.id.clone()).collect())).collect();
        o.items = pkg.items.iter().map(|i| i.value.id.clone()).chain(crate::pack::aliases::names(crate::pack::refs::DtKind::Item).map(|a| format!("dt:item/{a}"))).collect();
        o.units = pkg.units.iter().map(|u| u.value.id.clone()).chain(crate::pack::aliases::names(crate::pack::refs::DtKind::Unit).map(|a| format!("dt:{a}"))).collect();
        o.spells = crate::pack::aliases::names(crate::pack::refs::DtKind::Spell).map(|a| format!("dt:spell/{a}")).collect();
        o
    }

    /// The choices of a dialog (empty when it is not in the package).
    pub fn choices_of(&self, dialog: &str) -> Vec<String> {
        self.choices.iter().find(|(d, _)| d == dialog).map(|(_, c)| c.clone()).unwrap_or_default()
    }

    /// Whether a variable is declared as a whole number (`None` when it is not declared).
    pub fn var_is_int(&self, name: &str) -> Option<bool> {
        self.vars.iter().find(|(n, _)| n == name).map(|(_, int)| *int)
    }
}

fn named_of_action(a: &pt::Action, out: &mut Vec<String>) {
    match a {
        pt::Action::GiveUnit { named: Some(n), .. } => out.push(n.clone()),
        pt::Action::RemoveUnit { named, .. } => out.push(named.clone()),
        _ => {}
    }
}

/// The default value of an argument, as a new row gets it: the first of the options that fits,
/// else a neutral value (the row is then refused until the form sets it).
pub fn default_value(arg: Arg, key: &str, opts: &Options, dialog: Option<&str>) -> Value {
    let first = |list: &[String]| list.first().cloned().unwrap_or_default();
    match arg {
        Arg::Int(lo, hi) => Value::from(0_i64.clamp(lo, hi)),
        Arg::Bool => Value::from(matches!(key, "value" | "permanent" | "clear")),
        Arg::True => Value::from(true),
        Arg::Cell | Arg::Place => cell_value(&[0, 0]),
        Arg::Cells => {
            let mut outer = toml_edit::Array::new();
            outer.push(cell_value(&[0, 0]));
            outer.push(cell_value(&[1, 0]));
            Value::Array(outer)
        }
        Arg::Var => Value::from(first(&opts.vars.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>())),
        Arg::VarValue => Value::from(0_i64),
        Arg::Army | Arg::Entity => Value::from(first(&opts.armies)),
        Arg::Building => Value::from(first(&opts.buildings)),
        Arg::Dialog => Value::from(first(&opts.dialogs)),
        Arg::Choice => Value::from(first(&dialog.map(|d| opts.choices_of(d)).unwrap_or_default())),
        Arg::Item => Value::from(first(&opts.items)),
        Arg::Unit => Value::from(first(&opts.units)),
        Arg::Units => {
            let mut a = toml_edit::Array::new();
            if let Some(u) = opts.units.first() {
                a.push(u.as_str());
            }
            Value::Array(a)
        }
        Arg::Spell => Value::from(first(&opts.spells)),
        Arg::Map => Value::from(first(&opts.maps)),
        Arg::Named => Value::from(first(&opts.named)),
        Arg::Faction => Value::from("enemy"),
        Arg::Side => Value::from("enemy"),
        Arg::Cmp => Value::from(if key == "op" { "==" } else { ">=" }),
        Arg::Model => Value::from("bandits"),
        Arg::EntityKind => Value::from("army"),
        Arg::Holder | Arg::PosHolder => Value::from(if key == "to" { "pack" } else { "player" }),
        Arg::Figure => Value::from("default"),
        Arg::Target => Value::from("hero"),
        Arg::NewArmy => Value::from("army_new"),
        Arg::Text if key == "rules" => Value::from("dt"),
        Arg::Text if key == "quest" || key == "complete_quest" => Value::from("new_quest"),
        Arg::Text | Arg::TextKey => Value::from(""),
    }
}

/// A cell `[x, y]` as a TOML array.
pub fn cell_value(c: &[i64]) -> Value {
    let mut a = toml_edit::Array::new();
    for n in c {
        a.push(*n);
    }
    Value::Array(a)
}

/// A new row of a kind, its positional value and every required argument at its default. An
/// optional argument is not set, and a group takes its first member. A choice takes the choices of
/// the dialog the row names (`dialog`), when it names one before it.
pub fn new_row(section: Section, kind: &str, opts: &Options, dialog: Option<&str>) -> Option<Row> {
    let s = spec(section, kind)?;
    let mut row = InlineTable::new();
    match section {
        Section::Event => {
            row.insert("event", Value::from(kind));
        }
        _ => {
            let p = s.positional.unwrap_or(Arg::Bool);
            row.insert(kind, default_value(p, kind, opts, dialog));
        }
    }
    let mut groups_done: Vec<&str> = Vec::new();
    for a in s.args {
        let key = match a.group {
            Some(g) => {
                if groups_done.contains(&g) {
                    continue;
                }
                groups_done.push(g);
                // The group's first member is its default.
                s.args.iter().find(|m| m.group == Some(g)).copied().unwrap_or(*a)
            }
            None => *a,
        };
        if key.optional && key.group.is_none() {
            continue;
        }
        if key.group.is_some() && s.args.iter().any(|m| m.group == key.group && m.optional) {
            // An optional group starts empty.
            continue;
        }
        let dialog_now = row.get("dialog").and_then(Value::as_str).map(str::to_string);
        row.insert(key.key, default_value(key.arg, key.key, opts, dialog_now.as_deref().or(dialog)));
    }
    Some(row)
}

/// The keys that a positional `garrison` (`add_units = "garrison"`, `remove_spells = "garrison"`)
/// needs: the building in `garrison` when the positional is `garrison`, and no `garrison` key
/// otherwise. `first_building` fills the key when it is missing.
pub fn fix_garrison(section: Section, row: &mut Row, first_building: &str) {
    let Some(kind) = section.kind_key(row) else { return };
    if !matches!(kind.as_str(), "add_units" | "remove_spells") {
        return;
    }
    let positional_garrison = row.get(&kind).and_then(Value::as_str) == Some("garrison");
    if positional_garrison {
        if !row.contains_key("garrison") {
            row.insert("garrison", Value::from(first_building));
        }
        row.remove("of");
    } else if row.contains_key("garrison") {
        row.remove("garrison");
    }
}

/// The rows of an array value (an inline array of inline tables). Other values give none.
pub fn rows_of(v: Option<&Item>) -> Vec<Row> {
    v.and_then(Item::as_value)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_inline_table().cloned()).collect())
        .unwrap_or_default()
}

/// The rows of a list as the array value written to the file: one row to a line when there are
/// several (as the reference packages write them).
pub fn rows_value(rows: &[Row]) -> Value {
    array_value(rows.iter().map(|r| Value::InlineTable(r.clone())).collect(), rows.len() > 1)
}

/// A trigger as the text has it: its id, map, `when`, conditions, actions and the line of its table.
#[derive(Clone, Debug)]
pub struct TriggerDraft {
    pub id: String,
    pub map: Option<String>,
    pub when: Row,
    pub cond: Vec<Row>,
    pub then: Vec<Row>,
    pub once: Option<bool>,
    pub chance: Option<i64>,
    pub line: u32,
}

/// A choice as the text has it.
#[derive(Clone, Debug)]
pub struct ChoiceDraft {
    pub id: String,
    pub text: String,
    pub cond: Vec<Row>,
    pub then: Vec<Row>,
}

/// A dialog as the text has it: its text keys, speaker, and choices in order.
#[derive(Clone, Debug)]
pub struct DialogDraft {
    pub id: String,
    pub title: String,
    pub text: String,
    pub speaker: Option<String>,
    pub choices: Vec<ChoiceDraft>,
    pub line: u32,
}

/// The `[[trigger]]` with `id` in a file's text, read as the form shows it.
pub fn trigger_draft(text: &str, id: &str) -> Option<TriggerDraft> {
    let doc: DocumentMut = text.parse().ok()?;
    let entry = find_entry(doc.as_table(), "trigger", id)?;
    let when = entry.get("when")?.as_inline_table()?.clone();
    Some(TriggerDraft {
        id: id.to_string(),
        map: entry.get("map").and_then(Item::as_str).map(str::to_string),
        when,
        cond: rows_of(entry.get("if")),
        then: rows_of(entry.get("then")),
        once: entry.get("once").and_then(Item::as_bool),
        chance: entry.get("chance").and_then(Item::as_integer),
        line: entry.span().map_or(1, |s| crate::pack::load::line_of(text, s.start)),
    })
}

/// The `[[dialog]]` with `id` in a file's text, read as the form shows it.
pub fn dialog_draft(text: &str, id: &str) -> Option<DialogDraft> {
    let doc: DocumentMut = text.parse().ok()?;
    let entry = find_entry(doc.as_table(), "dialog", id)?;
    let choices = match entry.get("choice") {
        Some(Item::ArrayOfTables(a)) => a
            .iter()
            .map(|c| ChoiceDraft {
                id: c.get("id").and_then(Item::as_str).unwrap_or_default().to_string(),
                text: c.get("text").and_then(Item::as_str).unwrap_or_default().to_string(),
                cond: rows_of(c.get("if")),
                then: rows_of(c.get("then")),
            })
            .collect(),
        _ => Vec::new(),
    };
    Some(DialogDraft {
        id: id.to_string(),
        title: entry.get("title").and_then(Item::as_str).unwrap_or_default().to_string(),
        text: entry.get("text").and_then(Item::as_str).unwrap_or_default().to_string(),
        speaker: entry.get("speaker").and_then(Item::as_str).map(str::to_string),
        choices,
        line: entry.span().map_or(1, |s| crate::pack::load::line_of(text, s.start)),
    })
}

/// The entry of the array of tables `key` whose `id` is `id`.
fn find_entry<'a>(t: &'a Table, key: &str, id: &str) -> Option<&'a Table> {
    match t.get(key)? {
        Item::ArrayOfTables(a) => a.iter().find(|e| e.get("id").and_then(Item::as_str) == Some(id)),
        _ => None,
    }
}

/// The edits that set a trigger's `when` to `row`.
pub fn set_when(id: &str, row: &Row) -> DocEdit {
    DocEdit::Set { path: vec![entry_id("trigger", id)], key: "when".into(), value: Value::InlineTable(row.clone()) }
}

/// A trigger's list (`if` or `then`) set to `rows`; an empty list removes the key when the file
/// has it (`present`).
pub fn set_trigger_list(id: &str, key: &str, rows: &[Row], present: bool) -> Vec<DocEdit> {
    list_edit(vec![entry_id("trigger", id)], key, rows, present)
}

/// A choice's list (`if` or `then`) set to `rows`.
pub fn set_choice_list(dialog: &str, choice: &str, key: &str, rows: &[Row], present: bool) -> Vec<DocEdit> {
    list_edit(vec![entry_id("dialog", dialog), entry_id("choice", choice)], key, rows, present)
}

fn list_edit(path: Vec<Hop>, key: &str, rows: &[Row], present: bool) -> Vec<DocEdit> {
    if rows.is_empty() {
        if present {
            return vec![DocEdit::Remove { path, key: key.into() }];
        }
        return Vec::new();
    }
    vec![DocEdit::Set { path, key: key.into(), value: rows_value(rows) }]
}

/// A plain key of a trigger set to a value (`map`, `once`, `chance`, `id`), or removed (`None`).
pub fn set_trigger_key(id: &str, key: &str, value: Option<Value>) -> DocEdit {
    match value {
        Some(v) => DocEdit::Set { path: vec![entry_id("trigger", id)], key: key.into(), value: v },
        None => DocEdit::Remove { path: vec![entry_id("trigger", id)], key: key.into() },
    }
}

/// A new trigger appended to a file: its id, its map, its `when` (an event row) and its actions
/// (none: the key is left out).
pub fn new_trigger(id: &str, map: &str, when: &Row, then: &[Row]) -> DocEdit {
    let mut fields = vec![("id".into(), Value::from(id)), ("map".into(), Value::from(map)), ("when".into(), Value::InlineTable(when.clone()))];
    if !then.is_empty() {
        fields.push(("then".into(), rows_value(then)));
    }
    DocEdit::Push { path: vec![], key: "trigger".into(), fields }
}

/// Deletes a trigger (its entry, with its comments).
pub fn drop_trigger(id: &str) -> DocEdit {
    DocEdit::Drop { path: vec![], key: "trigger".into(), pick: Pick::Id(id.to_string()) }
}

/// A new dialog appended to a file, with its text keys and no choices.
pub fn new_dialog(id: &str, title: &str, text: &str) -> DocEdit {
    DocEdit::Push {
        path: vec![],
        key: "dialog".into(),
        fields: vec![("id".into(), Value::from(id)), ("title".into(), Value::from(title)), ("text".into(), Value::from(text))],
    }
}

/// A dialog's plain key (`title`, `text`, `speaker`), set or removed.
pub fn set_dialog_key(dialog: &str, key: &str, value: Option<Value>) -> DocEdit {
    match value {
        Some(v) => DocEdit::Set { path: vec![entry_id("dialog", dialog)], key: key.into(), value: v },
        None => DocEdit::Remove { path: vec![entry_id("dialog", dialog)], key: key.into() },
    }
}

/// A choice's new entry at the end of its dialog's list, with its text key.
pub fn add_choice(dialog: &str, choice: &str, text: &str) -> DocEdit {
    DocEdit::Push {
        path: vec![entry_id("dialog", dialog)],
        key: "choice".into(),
        fields: vec![("id".into(), Value::from(choice)), ("text".into(), Value::from(text))],
    }
}

/// Deletes a choice of a dialog.
pub fn drop_choice(dialog: &str, choice: &str) -> DocEdit {
    DocEdit::Drop { path: vec![entry_id("dialog", dialog)], key: "choice".into(), pick: Pick::Id(choice.to_string()) }
}

/// Moves a choice of a dialog from position `from` to `to` (the order shown).
pub fn move_choice(dialog: &str, from: usize, to: usize) -> DocEdit {
    DocEdit::Move { path: vec![entry_id("dialog", dialog)], key: "choice".into(), from, to }
}

/// A choice's plain key (`id`, `text`), set.
pub fn set_choice_key(dialog: &str, choice: &str, key: &str, value: Value) -> DocEdit {
    DocEdit::Set { path: vec![entry_id("dialog", dialog), entry_id("choice", choice)], key: key.into(), value }
}

/// The trigger a `dialog_choice` event wires a choice to: `<dialog>_<choice>_to_<dialog2>` (a free
/// id, numbered when the first is taken).
pub fn wire_id(taken: &[String], dialog: &str, choice: &str, target: &str) -> String {
    let base = format!("{dialog}_{choice}_to_{target}");
    if !taken.contains(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}_{n}")).find(|c| !taken.contains(c)).unwrap_or(base)
}

/// The `then` of a choice or a trigger read from the text, with a `dialog = target` action at the end
/// (a choice that no trigger listens to opens its target on its own).
pub fn with_dialog_action(then: &[Row], target: &str) -> Vec<Row> {
    let mut row = InlineTable::new();
    row.insert("dialog", Value::from(target));
    let mut out = then.to_vec();
    out.push(row);
    out
}

/// One part of an operation that is one undo step: a new file (its text), or edits of a file.
#[derive(Clone, Debug)]
pub enum Step {
    Create(String, String),
    Edit(String, Vec<DocEdit>),
}

/// The file that holds a map's triggers: the file of a trigger that names the map, else
/// `triggers/<map>.toml`. The step creates that file when the package has none yet.
pub fn trigger_file(pkg: &Package, map: &str) -> (String, Option<Step>) {
    if let Some(t) = pkg.triggers.iter().find(|t| t.value.map.as_deref() == Some(map)) {
        return (t.file.clone(), None);
    }
    let file = format!("triggers/{map}.toml");
    let create = (!has_file(pkg, &file)).then(|| Step::Create(file.clone(), format!("# The triggers of the map {map}: when the story moves, and what it does.\n")));
    (file, create)
}

fn has_file(pkg: &Package, file: &str) -> bool {
    pkg.files.paths().any(|p| p == file)
}

/// The steps that wire a dialog's choice to `target`. A choice that a `dialog_choice` trigger
/// answers gets the action on a new trigger of the map (`dialog_choice` → `dialog = target`); a
/// choice that none answers opens `target` itself, as the last of its actions. One undo step.
pub fn wire_choice(pkg: &Package, text_of: &dyn Fn(&str) -> Option<String>, map: &str, dialog: &str, choice: &str, target: &str) -> Result<Vec<Step>, String> {
    let d = pkg.dialogs.iter().find(|x| x.value.id == dialog).ok_or_else(|| format!("no dialog `{dialog}`"))?;
    if !pkg.dialogs.iter().any(|x| x.value.id == target) {
        return Err(format!("no dialog `{target}`"));
    }
    let listened = pkg.triggers.iter().any(|t| matches!(&t.value.when, Event::DialogChoice { dialog: dd, choice: cc } if dd == dialog && cc == choice));
    if listened {
        let (file, create) = trigger_file(pkg, map);
        let taken: Vec<String> = pkg.triggers.iter().map(|t| t.value.id.clone()).collect();
        let id = wire_id(&taken, dialog, choice, target);
        let when = event_row("dialog_choice", &[("dialog", dialog), ("choice", choice)]);
        let mut steps: Vec<Step> = create.into_iter().collect();
        steps.push(Step::Edit(file, vec![new_trigger(&id, map, &when, &[dialog_action(target)])]));
        return Ok(steps);
    }
    let text = text_of(&d.file).ok_or_else(|| format!("{} cannot be read", d.file))?;
    let draft = dialog_draft(&text, dialog).ok_or_else(|| format!("the dialog `{dialog}` does not parse"))?;
    let c = draft.choices.iter().find(|x| x.id == choice).ok_or_else(|| format!("no choice `{choice}` in `{dialog}`"))?;
    let mut rows = c.then.clone();
    rows.push(dialog_action(target));
    Ok(vec![Step::Edit(d.file.clone(), set_choice_list(dialog, choice, "then", &rows, false))])
}

/// The steps that wire a trigger to open `target` (its actions gain `dialog = target`). One undo step.
pub fn wire_trigger(pkg: &Package, text_of: &dyn Fn(&str) -> Option<String>, trigger: &str, target: &str) -> Result<Vec<Step>, String> {
    let t = pkg.triggers.iter().find(|x| x.value.id == trigger).ok_or_else(|| format!("no trigger `{trigger}`"))?;
    if !pkg.dialogs.iter().any(|x| x.value.id == target) {
        return Err(format!("no dialog `{target}`"));
    }
    let text = text_of(&t.file).ok_or_else(|| format!("{} cannot be read", t.file))?;
    let draft = trigger_draft(&text, trigger).ok_or_else(|| format!("the trigger `{trigger}` does not parse"))?;
    let mut rows = draft.then.clone();
    rows.push(dialog_action(target));
    Ok(vec![Step::Edit(t.file.clone(), set_trigger_list(trigger, "then", &rows, false))])
}

/// The steps of a new trigger on `map`: a `game_start` with no actions, under a free id (returned).
pub fn new_trigger_steps(pkg: &Package, map: &str) -> (String, Vec<Step>) {
    let taken: Vec<String> = pkg.triggers.iter().map(|t| t.value.id.clone()).collect();
    let id = free_name(&taken, "new_trigger");
    let when = event_row("game_start", &[]);
    let (file, create) = trigger_file(pkg, map);
    let mut steps: Vec<Step> = create.into_iter().collect();
    steps.push(Step::Edit(file, vec![new_trigger(&id, map, &when, &[])]));
    (id, steps)
}

/// The steps of a new dialog on `map` (`dialogs/<map>.toml`, created when missing): its title and
/// text keys, each with the id as its text in every language. The id is free.
pub fn new_dialog_steps(pkg: &Package, map: &str) -> (String, Vec<Step>) {
    let taken: Vec<String> = pkg.dialogs.iter().map(|d| d.value.id.clone()).collect();
    let id = free_name(&taken, "new_dialog");
    let (title, text) = (format!("{id}.title"), format!("{id}.text"));
    let file = format!("dialogs/{map}.toml");
    let mut steps: Vec<Step> = Vec::new();
    if !has_file(pkg, &file) {
        steps.push(Step::Create(file.clone(), format!("# The dialogs of the map {map}: windows the story opens, with their choices.\n")));
    }
    for lang in pkg.texts.keys() {
        for key in [&title, &text] {
            let (f, e) = tx::set_cell(lang, key, &id);
            steps.push(Step::Edit(f, vec![e]));
        }
    }
    steps.push(Step::Edit(file, vec![new_dialog(&id, &title, &text)]));
    (id, steps)
}

/// The steps that add a choice to a dialog (free id `choice`, `choice_2`, …) with its text key in
/// every language. Nothing when the dialog has five choices already (a window shows five).
pub fn add_choice_steps(pkg: &Package, dialog: &str) -> Result<(String, Vec<Step>), String> {
    let d = pkg.dialogs.iter().find(|x| x.value.id == dialog).ok_or_else(|| format!("no dialog `{dialog}`"))?;
    let taken: Vec<String> = d.value.choices.iter().map(|c| c.id.clone()).collect();
    if taken.len() >= pt::MAX_CHOICES {
        return Err(format!("a window shows at most {} choices", pt::MAX_CHOICES));
    }
    let cid = free_name(&taken, "choice");
    let key = format!("{dialog}.{cid}");
    let mut steps: Vec<Step> = Vec::new();
    for lang in pkg.texts.keys() {
        let (f, e) = tx::set_cell(lang, &key, &cid);
        steps.push(Step::Edit(f, vec![e]));
    }
    steps.push(Step::Edit(d.file.clone(), vec![add_choice(dialog, &cid, &key)]));
    Ok((cid, steps))
}

/// A free name: `base`, or `base_2`, `base_3`, … when it is taken.
pub fn free_name(taken: &[String], base: &str) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base}_{n}")).find(|c| !taken.iter().any(|t| t == c)).unwrap_or_else(|| base.to_string())
}

/// An event row: its kind and the string arguments.
pub fn event_row(kind: &str, args: &[(&str, &str)]) -> Row {
    let mut t = InlineTable::new();
    t.insert("event", Value::from(kind));
    for (k, v) in args {
        t.insert(*k, Value::from(*v));
    }
    t
}

/// An action row `{ dialog = target }`.
pub fn dialog_action(target: &str) -> Row {
    let mut t = InlineTable::new();
    t.insert("dialog", Value::from(target));
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_of_the_vocabulary_has_its_arguments_and_nothing_else() {
        for (section, specs) in [(Section::Event, EVENT_SPECS), (Section::Cond, COND_SPECS), (Section::Action, ACTION_SPECS)] {
            let kinds = section.kinds();
            for kind in kinds {
                assert!(spec(section, kind).is_some(), "no arguments for {kind}");
            }
            for s in specs {
                assert!(kinds.contains(&s.kind), "{} is not in the vocabulary", s.kind);
            }
        }
    }

    #[test]
    fn a_new_row_of_every_kind_is_read_back_by_the_parser() {
        // The defaults must be the format's own: a row made by `new_row` parses when every
        // required argument has an option to take. The options here give one of each name.
        let opts = Options {
            vars: vec![("dread".into(), true), ("ford_held".into(), false)],
            armies: vec!["toll".into()],
            buildings: vec!["dawnwatch".into()],
            maps: vec!["brod".into()],
            dialogs: vec!["ford".into()],
            choices: vec![("ford".into(), vec!["pay".into()])],
            items: vec!["dt:item/sword".into()],
            units: vec!["dt:swordsman".into()],
            spells: vec!["dt:spell/fireball".into()],
            named: vec!["ilm".into()],
        };
        for section in [Section::Event, Section::Cond, Section::Action] {
            for kind in section.kinds() {
                let row = new_row(section, kind, &opts, Some("ford")).expect("a row");
                if let Err(p) = validate(section, &row) {
                    panic!("{kind}: a new row is refused: {} ({})", p.message, p.hint);
                }
            }
        }
    }

    #[test]
    fn a_row_is_checked_by_the_format_parser() {
        let mut row = InlineTable::new();
        row.insert("var", Value::from("dread"));
        row.insert("op", Value::from(">="));
        row.insert("value", Value::from(3_i64));
        assert!(validate(Section::Cond, &row).is_ok());
        row.insert("colour", Value::from("red"));
        let err = validate(Section::Cond, &row).unwrap_err();
        assert!(err.message.contains("colour"), "{err:?}");
        let mut ev = InlineTable::new();
        ev.insert("event", Value::from("hero_enters"));
        assert!(validate(Section::Event, &ev).is_err(), "hero_enters needs at");
    }

    #[test]
    fn the_kind_is_found_by_its_key() {
        let mut row = InlineTable::new();
        row.insert("add", Value::from("dread"));
        row.insert("value", Value::from(1_i64));
        assert_eq!(Section::Action.kind_key(&row).as_deref(), Some("add"));
        let mut ev = InlineTable::new();
        ev.insert("event", Value::from("day_start"));
        assert_eq!(Section::Event.kind_key(&ev).as_deref(), Some("day_start"));
    }

    #[test]
    fn a_list_is_one_array_and_an_empty_list_removes_the_key() {
        let mut row = InlineTable::new();
        row.insert("dialog", Value::from("ford"));
        let edits = set_trigger_list("t", "then", std::slice::from_ref(&row), false);
        assert!(matches!(&edits[0], DocEdit::Set { key, .. } if key == "then"));
        assert!(set_trigger_list("t", "then", &[], false).is_empty(), "nothing to remove");
        assert!(matches!(&set_trigger_list("t", "then", &[], true)[0], DocEdit::Remove { key, .. } if key == "then"));
    }

    #[test]
    fn wire_ids_are_free() {
        assert_eq!(wire_id(&[], "a", "b", "c"), "a_b_to_c");
        assert_eq!(wire_id(&["a_b_to_c".into()], "a", "b", "c"), "a_b_to_c_2");
        assert_eq!(wire_id(&["a_b_to_c".into(), "a_b_to_c_2".into()], "a", "b", "c"), "a_b_to_c_3");
    }
}
