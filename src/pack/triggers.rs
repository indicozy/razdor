//! `triggers/*.toml` and `dialogs/*.toml`: a package's declarative story. A trigger says when
//! (an event), if (conditions, all must hold) and then (actions, in order). Dialogs are windows
//! with choices, opened by actions and answered by choosing one. The format is in
//! `docs/reference/rzpack-format.md` ("Triggers and dialogs").
//!
//! Reading turns the TOML tables into the typed forms below; every problem is a [`Problem`] with
//! a hint, reported at the line of its trigger or dialog. The runtime (`rules::triggers`) takes
//! these forms and resolves their references to engine ids.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use toml::{Spanned, Table, Value};

use super::aliases;
use super::diag::Report;
use super::load::{Located, Package};
use super::refs::{valid_id, DtKind, Ref};

/// The events a trigger can wait for (`when`).
pub const EVENTS: [&str; 13] = [
    "game_start",
    "day_start",
    "week_start",
    "new_moon",
    "hero_enters",
    "hero_enters_building",
    "army_defeated",
    "building_captured",
    "building_lost",
    "sim_victory",
    "dialog_choice",
    "var_changed",
    "army_arrives",
];
/// The conditions (`if`). Each is a table with one of these keys (its kind).
pub const CONDITIONS: [&str; 10] = ["var", "random", "hero_at", "army_at", "has_named_unit", "has_item", "gold", "army_alive", "building_owner", "has_spell"];
/// The actions (`then`), each a table with one of these keys.
pub const ACTIONS: [&str; 30] = [
    "set",
    "add",
    "random_var",
    "dialog",
    "give_gold",
    "give_item",
    "give_unit",
    "give_xp",
    "spawn_army",
    "remove_army",
    "start_battle",
    "teleport_hero",
    "teleport_army",
    "set_faction",
    "set_relation",
    "add_units",
    "cast_on_army",
    "remove_spells",
    "remove_spell_from_book",
    "victory",
    "defeat",
    "army_goal",
    "army_speed",
    "hero_figure",
    "army_figure",
    "remove_unit",
    "travel",
    "ending",
    "quest",
    "complete_quest",
];
/// The keys that are arguments, not kinds. A kind key is one of the lists above; a key here
/// never is.
const ARGUMENTS: [&str; 33] = [
    "value", "op", "at", "faction", "to", "of", "in", "unit", "named", "class", "spell", "all", "building", "army", "item", "min", "max", "minutes",
    "permanent", "towards", "troops", "model", "choice", "event", "garrison", "percent", "go_to", "go_to_building", "follow", "patrol", "attack", "clear", "next",
];

/// A problem with one table: what is wrong, and what to do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub message: String,
    pub hint: String,
}

type Res<T> = Result<T, Problem>;

fn bad<T>(message: impl Into<String>, hint: impl Into<String>) -> Res<T> {
    Err(Problem { message: message.into(), hint: hint.into() })
}

/// The name of a variable, an army, a building, a dialog or a trigger: `[a-z0-9_]+`.
fn valid_name(s: &str) -> bool {
    valid_id(s)
}

/// The closest of `options` to `word` (edit distance at most 3), if any.
pub fn closest<'a>(word: &str, options: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    options
        .into_iter()
        .map(|o| (edit_distance(word, o), o))
        .filter(|(d, _)| *d <= 3)
        .min_by_key(|(d, _)| *d)
        .map(|(_, o)| o)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// The kind of a table: its one key that names a kind of `kinds`.
fn kind_of(t: &Table, kinds: &[&'static str], what: &str) -> Res<&'static str> {
    let found: Vec<&str> = t.keys().map(String::as_str).filter(|k| kinds.contains(k)).collect();
    match found.as_slice() {
        [one] => Ok(kinds.iter().copied().find(|k| k == one).unwrap_or_default()),
        [] => {
            let stray: Vec<&str> = t.keys().map(String::as_str).filter(|k| !ARGUMENTS.contains(k)).collect();
            let word = stray.first().copied().unwrap_or("");
            let hint = match closest(word, kinds.iter().copied()) {
                Some(near) => format!("did you mean `{near}`?"),
                None => format!("the {what}s are: {}", kinds.join(", ")),
            };
            bad(format!("unknown {what} `{word}`"), hint)
        }
        _ => bad(format!("a {what} names one kind, not {}", found.join(" and ")), format!("split it into one table per {what}")),
    }
}

/// Keys of `t` that are not `kind` or one of `allowed`: an error with the closest argument name.
fn check_args(t: &Table, kind: &str, allowed: &[&str]) -> Res<()> {
    for key in t.keys() {
        if key != kind && !allowed.contains(&key.as_str()) {
            let hint = match closest(key, allowed.iter().copied()) {
                Some(near) => format!("did you mean `{near}`?"),
                None if allowed.is_empty() => format!("`{kind}` takes no other keys"),
                None => format!("`{kind}` takes: {}", allowed.join(", ")),
            };
            return bad(format!("`{key}` is not a key of `{kind}`"), hint);
        }
    }
    Ok(())
}

/// The `value` of a condition that is a presence (`has_named_unit`, `army_alive`): true when it is
/// not given, `false` for the absence (`value = false`).
fn flag_of(t: &Table, kind: &str) -> Res<bool> {
    match arg(t, "value") {
        None => Ok(true),
        Some(v) => v.as_bool().map_or_else(|| bad(format!("`{kind}`'s `value` is true or false"), "`value = false` for the absence"), Ok),
    }
}

fn arg<'a>(t: &'a Table, key: &str) -> Option<&'a Value> {
    t.get(key)
}

fn need<'a>(t: &'a Table, kind: &str, key: &str) -> Res<&'a Value> {
    arg(t, key).map_or_else(|| bad(format!("`{kind}` needs `{key}`"), format!("add `{key} = …` to the table")), Ok)
}

fn int_of(v: &Value, what: &str) -> Res<i64> {
    v.as_integer().map_or_else(|| bad(format!("{what} must be a whole number"), format!("write e.g. `{what} = 3`")), Ok)
}

fn str_of<'a>(v: &'a Value, what: &str) -> Res<&'a str> {
    v.as_str().map_or_else(|| bad(format!("{what} must be text in quotes"), "write it as a string"), Ok)
}

fn name_of(v: &Value, what: &str) -> Res<String> {
    let s = str_of(v, what)?;
    if valid_name(s) {
        Ok(s.to_string())
    } else {
        bad(format!("`{s}` is not a valid {what} name"), "use lowercase letters, digits and _")
    }
}

/// A cell `[x, y]` or an area `[x, y, w, h]` (w and h at least 1; x and y not negative).
fn place_of(v: &Value) -> Res<Place> {
    let Some(a) = v.as_array() else {
        return bad("a place is [x, y] or [x, y, w, h]", "e.g. `at = [12, 7]`");
    };
    let nums: Vec<i64> = a.iter().map(|n| int_of(n, "a coordinate")).collect::<Res<_>>()?;
    match nums.as_slice() {
        [x, y] if *x >= 0 && *y >= 0 => Ok(Place::Cell(*x as i32, *y as i32)),
        [x, y, w, h] if *x >= 0 && *y >= 0 && *w >= 1 && *h >= 1 => Ok(Place::Area(*x as i32, *y as i32, *w as i32, *h as i32)),
        _ => bad("a place is [x, y] (not negative) or [x, y, w, h] (w and h at least 1)", "e.g. `at = [12, 7]` or `at = [10, 5, 4, 3]`"),
    }
}

/// The place under `at`, required.
fn at_of(t: &Table, kind: &str) -> Res<Place> {
    place_of(need(t, kind, "at")?)
}

/// A reference to a unit (`dt:` alias, or a local or package id).
fn unit_ref(v: &Value) -> Res<Ref> {
    let r = Ref::parse(str_of(v, "a unit")?).map_err(|m| Problem { message: m, hint: "a unit is dt:<alias> or a package's unit id".into() })?;
    match &r {
        Ref::Dt { kind: DtKind::Item | DtKind::Spell, .. } => bad(format!("`{r}` is not a unit"), "use dt:<alias> for a unit"),
        _ => Ok(r),
    }
}

fn item_ref(v: &Value) -> Res<Ref> {
    let r = Ref::parse(str_of(v, "an item")?).map_err(|m| Problem { message: m, hint: "an item is dt:item/<alias> or a package's item id".into() })?;
    match &r {
        Ref::Dt { kind: DtKind::Unit | DtKind::Spell, .. } => bad(format!("`{r}` is not an item"), "use dt:item/<alias> for an item"),
        _ => Ok(r),
    }
}

fn spell_ref(v: &Value) -> Res<Ref> {
    let r = Ref::parse(str_of(v, "a spell")?).map_err(|m| Problem { message: m, hint: "a spell is dt:spell/<alias>".into() })?;
    match &r {
        Ref::Dt { kind: DtKind::Spell, .. } => Ok(r),
        _ => bad(format!("`{r}` is not a base spell"), "spells of a package are not in format 1: use dt:spell/<alias>"),
    }
}

fn units_of(v: &Value) -> Res<Vec<Ref>> {
    let Some(a) = v.as_array() else { return bad("`troops` is a list of units", "e.g. `troops = [\"dt:swordsman\", \"dt:archer\"]`") };
    a.iter().map(unit_ref).collect()
}

/// A faction by name or number: player 1, ally 2, neighbour 3, enemy 4.
fn faction_of(v: &Value) -> Res<u8> {
    const NAMES: [&str; 4] = ["player", "ally", "neighbour", "enemy"];
    if let Some(s) = v.as_str() {
        return NAMES.iter().position(|n| *n == s).map(|i| i as u8 + 1).map_or_else(
            || bad(format!("`{s}` is not a faction"), format!("use one of {}", NAMES.join(", "))),
            Ok,
        );
    }
    match int_of(v, "a faction")? {
        n @ 1..=4 => Ok(n as u8),
        _ => bad("a faction is 1 to 4", format!("use one of {}", NAMES.join(", "))),
    }
}

/// Towards whom a relation is: player 0, ally 1, neighbour 2, enemy 3.
fn towards_of(v: &Value) -> Res<u8> {
    const NAMES: [&str; 4] = ["player", "ally", "neighbour", "enemy"];
    let s = str_of(v, "`towards`")?;
    NAMES.iter().position(|n| *n == s).map(|i| i as u8).map_or_else(|| bad(format!("`{s}` is not a side"), format!("use one of {}", NAMES.join(", "))), Ok)
}

fn cmp_of(s: &str) -> Res<Cmp> {
    Ok(match s {
        "==" => Cmp::Eq,
        "!=" => Cmp::Ne,
        "<" => Cmp::Lt,
        "<=" => Cmp::Le,
        ">" => Cmp::Gt,
        ">=" => Cmp::Ge,
        other => return bad(format!("`{other}` is not a comparison"), "use ==, !=, <, <=, >, or >="),
    })
}

/// A holder of units, items or spells: the hero's army, the pack, a scenario army (by id), a
/// named character of the player's army (`named:<id>`), or a garrison (`garrison = "<building>"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Holder {
    Player,
    Pack,
    Army(String),
    Named(String),
    Garrison(String),
}

fn holder_of(v: &Value, what: &str) -> Res<Holder> {
    let s = str_of(v, what)?;
    Ok(match s {
        "player" => Holder::Player,
        "pack" => Holder::Pack,
        _ => match s.strip_prefix("named:") {
            Some(n) => Holder::Named(name_of(&Value::String(n.into()), "named character")?),
            None => Holder::Army(name_of(v, "army")?),
        },
    })
}

/// The holder a table gives under `key` (`in` or `to`), or the garrison its `garrison` key names
/// (the two are not both given). Neither: `default`.
fn holder_key(t: &Table, kind: &str, key: &str, default: Holder) -> Res<Holder> {
    match (arg(t, key), arg(t, "garrison")) {
        (Some(_), Some(_)) => bad(format!("`{kind}` takes `{key}` or `garrison`, not both"), "pick one holder"),
        (None, Some(g)) => Ok(Holder::Garrison(name_of(g, "building")?)),
        (Some(h), None) => holder_of(h, &format!("`{key}`")),
        (None, None) => Ok(default),
    }
}

/// A holder written as the action's own value: `remove_spells = "garrison"` with the building in
/// `garrison = "<id>"`; otherwise the value is the holder itself.
fn positional_holder(t: &Table, kind: &str, v: &Value) -> Res<Holder> {
    match arg(t, "garrison") {
        None if v.as_str() == Some("garrison") => bad(format!("`{kind} = \"garrison\"` needs `garrison = \"<building>\"`"), "name the building in `garrison = \"<id>\"`"),
        None => holder_of(v, &format!("`{kind}`")),
        Some(g) if v.as_str() == Some("garrison") => Ok(Holder::Garrison(name_of(g, "building")?)),
        Some(_) => bad(format!("`{kind}` with `garrison` is written `{kind} = \"garrison\"`"), "the building goes in `garrison = \"<id>\"`"),
    }
}

/// A goal's target: `"hero"`, or an army's id.
fn target_of(v: &Value) -> Res<Target> {
    match v.as_str() {
        Some("hero") => Ok(Target::Hero),
        _ => Ok(Target::Army(name_of(v, "army")?)),
    }
}

/// A trigger's event (`when`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    GameStart,
    DayStart,
    WeekStart,
    NewMoon,
    HeroEnters(Place),
    HeroEntersBuilding(String),
    ArmyDefeated(String),
    BuildingCaptured(String),
    /// A building the player owned changes owner (`building_lost`).
    BuildingLost(String),
    /// Raised by `razdor-pack sim --auto-win-after` before a map's victory is taken (a developer
    /// moment: the game never raises it). A story plays its last dialog then, so the sim walks
    /// the endings.
    SimVictory,
    DialogChoice { dialog: String, choice: String },
    VarChanged(String),
    /// An army with a goal reaches it: at a cell (`at`), or in a building (`building`); none:
    /// any arrival of the army at its goal.
    ArmyArrives { army: String, at: Option<(i32, i32)>, building: Option<String> },
}

/// A cell or an area of the map: `[x, y]` or `[x, y, w, h]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Cell(i32, i32),
    Area(i32, i32, i32, i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    Int,
    Bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VarValue {
    Int(i64),
    Bool(bool),
}

/// A declared variable (`[vars]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Var {
    pub name: String,
    pub kind: VarKind,
    pub default: VarValue,
}

/// What a building or an army is, for `set_faction`, `set_relation`, `add_units`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entity {
    Army(String),
    Building(String),
}

/// Where an army is teleported to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dest {
    Cell(i32, i32),
    Building(String),
}

/// How long a spell lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Until {
    Minutes(u64),
    Permanent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cond {
    Var { name: String, cmp: Cmp, value: VarValue },
    Random(u8),
    HeroAt(Place),
    ArmyAt { army: String, at: Place },
    /// `present`: true (the default) when the named character is there; `value = false` when he is not.
    HasNamed { named: String, unit: Ref, holder: Holder, present: bool },
    HasItem { item: Ref, holder: Holder },
    Gold { amount: i64, cmp: Cmp },
    /// `alive`: true (the default) when the army stands; `value = false` when it is gone.
    ArmyAlive { army: String, alive: bool },
    BuildingOwner { building: String, faction: u8 },
    HasSpell { spell: Ref, holder: Holder, all: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Set { name: String, value: VarValue },
    /// `value` added to a number variable; with `min` and/or `max` the result is clamped to them.
    Add { name: String, value: i64, min: Option<i64>, max: Option<i64> },
    RandomVar { name: String, min: i64, max: i64 },
    Dialog(String),
    GiveGold(i64),
    GiveItem { item: Ref, to: Holder },
    GiveUnit { unit: Ref, named: Option<String> },
    GiveXp { amount: i64, to: Holder },
    SpawnArmy { army: String, at: (i32, i32), faction: u8, model: u8, troops: Vec<Ref> },
    RemoveArmy(String),
    /// The army meets the hero now, in a battle run by `rules` (`dt`, or `lua:<script>`: a field battle).
    StartBattle { army: String, rules: String },
    TeleportHero((i32, i32)),
    TeleportArmy { army: String, to: Dest },
    SetFaction { of: Entity, faction: u8 },
    SetRelation { of: Entity, towards: u8, value: i64 },
    AddUnits { of: Entity, troops: Vec<Ref> },
    Cast { spell: Ref, holder: Holder, until: Until },
    RemoveSpells(Holder),
    ForgetSpell(Ref),
    Victory,
    Defeat,
    ArmyGoal { army: String, goal: GoalSpec },
    ArmySpeed { army: String, percent: u32 },
    /// `None`: the hero's own figure (`hero_figure = "default"`).
    HeroFigure(Option<Figure>),
    ArmyFigure { army: String, figure: Figure },
    /// `remove_unit = "<named>"`: the named character leaves the holder (the hero's army by default).
    RemoveUnit { named: String, holder: Holder },
    /// `victory = true, next = "<map>"`: a victory whose campaign goes to that map (the override
    /// of the campaign's graph, `campaign.toml`).
    VictoryTo(String),
    /// `travel = "<map>"`, with `at = "<point>"` where the hero comes in: the campaign leaves this
    /// map for another one, without a victory.
    Travel { map: String, at: Option<String> },
    /// `ending = "<dialog>"`: the dialog shows, then the campaign ends.
    Ending(String),
    /// `quest = "<id>", title = "<key>", text = "<key>"`: an active diary entry, or its texts
    /// updated when the id is already active. The texts are text keys, shown in the game's language.
    Quest { id: String, title: String, text: String },
    /// `complete_quest = "<id>"`: the active diary entry of that id is completed (a no-op otherwise).
    CompleteQuest(String),
}

/// A map figure: a unit's (its kind's picture, `dt:<alias>`) or an army model's (`model = "bandits"`).
/// A model is its [`ArmyModel`](crate::dt::dtm::ArmyModel) code (1 to 12): see [`model_code`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Figure {
    Unit(Ref),
    Model(u8),
}

/// The army models a figure may take, by their names in a package: the army models of the
/// base game that are drawn as a figure (`ArmyModel` 1 to 6, 10 to 12).
pub const MODELS: [(&str, u8); 9] = [
    ("knight", 1),
    ("archmage", 2),
    ("ranger", 3),
    ("feudal", 4),
    ("bandits", 5),
    ("peasants", 6),
    ("necromancer", 10),
    ("ghosts", 11),
    ("zombies", 12),
];

/// The code of an army model by its name (see [`MODELS`]).
pub fn model_code(name: &str) -> Res<u8> {
    MODELS.iter().find(|(n, _)| *n == name).map(|(_, code)| *code).map_or_else(
        || {
            let names: Vec<&str> = MODELS.iter().map(|(n, _)| *n).collect();
            bad(format!("`{name}` is not an army model"), format!("use one of {}", names.join(", ")))
        },
        Ok,
    )
}

/// What an `army_goal` asks for (exactly one key of the action).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoalSpec {
    GoTo((i32, i32)),
    GoToBuilding(String),
    Follow(Target),
    Patrol(Vec<(i32, i32)>),
    Attack(Target),
    /// `clear = true`: the army goes back to the original AI.
    Clear,
}

/// A goal's target: the hero, or a scenario or package army by its id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Hero,
    Army(String),
}

/// A trigger, as read. `map` scopes it to one map of the package; none applies it to every map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trigger {
    pub id: String,
    pub map: Option<String>,
    pub when: Event,
    pub cond: Vec<Cond>,
    pub then: Vec<Action>,
    pub once: bool,
    pub chance: u8,
}

/// One choice of a dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    /// A text key.
    pub text: String,
    pub cond: Vec<Cond>,
    pub then: Vec<Action>,
}

/// A dialog, as read. `title`, `text` and the choices' texts are text keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub id: String,
    pub title: String,
    pub text: String,
    pub speaker: Option<Ref>,
    pub choices: Vec<Choice>,
}

/// Raw forms, as serde reads them. The `if`, `then` and `when` tables are read by the rules
/// below, so their problems carry hints.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerFile {
    #[serde(default)]
    pub vars: Table,
    #[serde(default)]
    pub trigger: Vec<Spanned<RawTrigger>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTrigger {
    pub id: String,
    pub when: Table,
    #[serde(rename = "if", default)]
    pub cond: Vec<Table>,
    #[serde(default)]
    pub then: Vec<Table>,
    #[serde(default = "yes")]
    pub once: bool,
    #[serde(default)]
    pub chance: Option<i64>,
    /// The map this trigger applies to (its id); none: every map of the package.
    #[serde(default)]
    pub map: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialogFile {
    #[serde(default)]
    pub dialog: Vec<Spanned<RawDialog>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDialog {
    pub id: String,
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub choice: Vec<Spanned<RawChoice>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawChoice {
    pub id: String,
    pub text: String,
    #[serde(rename = "if", default)]
    pub cond: Vec<Table>,
    #[serde(default)]
    pub then: Vec<Table>,
}

/// The most choices a dialog window shows (its buttons stack under the text).
pub const MAX_CHOICES: usize = 5;

/// Reads a `when` table.
pub fn parse_event(t: &Table) -> Res<Event> {
    let kind = match t.get("event") {
        Some(v) => str_of(v, "`event`")?.to_string(),
        None => return bad("a trigger needs `when = { event = \"…\" }`", format!("events are: {}", EVENTS.join(", "))),
    };
    let rest = |allowed: &[&str]| check_args(t, "event", allowed);
    match kind.as_str() {
        "game_start" => rest(&[]).map(|()| Event::GameStart),
        "day_start" => rest(&[]).map(|()| Event::DayStart),
        "week_start" => rest(&[]).map(|()| Event::WeekStart),
        "new_moon" => rest(&[]).map(|()| Event::NewMoon),
        "hero_enters" => {
            rest(&["at"])?;
            Ok(Event::HeroEnters(at_of(t, "hero_enters")?))
        }
        "hero_enters_building" => {
            rest(&["building"])?;
            Ok(Event::HeroEntersBuilding(name_of(need(t, "hero_enters_building", "building")?, "building")?))
        }
        "army_defeated" => {
            rest(&["army"])?;
            Ok(Event::ArmyDefeated(name_of(need(t, "army_defeated", "army")?, "army")?))
        }
        "building_captured" => {
            rest(&["building"])?;
            Ok(Event::BuildingCaptured(name_of(need(t, "building_captured", "building")?, "building")?))
        }
        "sim_victory" => rest(&[]).map(|()| Event::SimVictory),
        "building_lost" => {
            rest(&["building"])?;
            Ok(Event::BuildingLost(name_of(need(t, "building_lost", "building")?, "building")?))
        }
        "dialog_choice" => {
            rest(&["dialog", "choice"])?;
            Ok(Event::DialogChoice {
                dialog: name_of(need(t, "dialog_choice", "dialog")?, "dialog")?,
                choice: name_of(need(t, "dialog_choice", "choice")?, "choice")?,
            })
        }
        "var_changed" => {
            rest(&["var"])?;
            Ok(Event::VarChanged(name_of(need(t, "var_changed", "var")?, "variable")?))
        }
        "army_arrives" => {
            rest(&["army", "at", "building"])?;
            let army = name_of(need(t, "army_arrives", "army")?, "army")?;
            let at = match arg(t, "at") {
                Some(v) => match place_of(v)? {
                    Place::Cell(x, y) => Some((x, y)),
                    Place::Area(..) => return bad("an arrival is at one cell", "give `at = [x, y]`"),
                },
                None => None,
            };
            let building = match arg(t, "building") {
                Some(b) => Some(name_of(b, "building")?),
                None => None,
            };
            if at.is_some() && building.is_some() {
                return bad("`army_arrives` takes `at` or `building`, not both", "a cell, or the building the goal names");
            }
            Ok(Event::ArmyArrives { army, at, building })
        }
        other => bad(format!("unknown event `{other}`"), closest(other, EVENTS).map_or_else(|| format!("events are: {}", EVENTS.join(", ")), |n| format!("did you mean `{n}`?"))),
    }
}

/// Reads one condition (an `if` entry).
pub fn parse_cond(t: &Table) -> Res<Cond> {
    let kind = kind_of(t, &CONDITIONS, "condition")?;
    let v = &t[kind];
    match kind {
        "var" => {
            check_args(t, kind, &["op", "value"])?;
            let op = match arg(t, "op") {
                Some(o) => cmp_of(str_of(o, "`op`")?)?,
                None => Cmp::Eq,
            };
            Ok(Cond::Var { name: name_of(v, "variable")?, cmp: op, value: value2_of(need(t, kind, "value")?)? })
        }
        "random" => {
            check_args(t, kind, &[])?;
            match int_of(v, "`random`")? {
                n @ 0..=100 => Ok(Cond::Random(n as u8)),
                _ => bad("`random` is a percent, 0 to 100", "e.g. `random = 30`"),
            }
        }
        "hero_at" => {
            check_args(t, kind, &[])?;
            Ok(Cond::HeroAt(place_of(v)?))
        }
        "army_at" => {
            check_args(t, kind, &["at"])?;
            Ok(Cond::ArmyAt { army: name_of(v, "army")?, at: at_of(t, kind)? })
        }
        "has_named_unit" => {
            check_args(t, kind, &["unit", "in", "garrison", "value"])?;
            let holder = holder_key(t, kind, "in", Holder::Player)?;
            if matches!(holder, Holder::Pack | Holder::Named(_)) {
                return bad("`has_named_unit` looks in the player's army, an army or a garrison", "use `in = \"<army id>\"`, `garrison = \"<building>\"` or leave both out");
            }
            let present = flag_of(t, kind)?;
            Ok(Cond::HasNamed { named: name_of(v, "named character")?, unit: unit_ref(need(t, kind, "unit")?)?, holder, present })
        }
        "has_item" => {
            check_args(t, kind, &["in", "garrison"])?;
            let holder = holder_key(t, kind, "in", Holder::Player)?;
            if matches!(holder, Holder::Named(_)) {
                return bad("`has_item` looks in the player's pack and army, an army or a garrison", "use `in = \"player\"`, an army id or `garrison = \"<building>\"`");
            }
            Ok(Cond::HasItem { item: item_ref(v)?, holder })
        }
        "gold" => {
            check_args(t, kind, &["op"])?;
            let cmp = match arg(t, "op") {
                Some(o) => cmp_of(str_of(o, "`op`")?)?,
                None => Cmp::Ge,
            };
            Ok(Cond::Gold { amount: int_of(v, "`gold`")?, cmp })
        }
        "army_alive" => {
            check_args(t, kind, &["value"])?;
            Ok(Cond::ArmyAlive { army: name_of(v, "army")?, alive: flag_of(t, kind)? })
        }
        "building_owner" => {
            check_args(t, kind, &["faction"])?;
            Ok(Cond::BuildingOwner { building: name_of(v, "building")?, faction: faction_of(need(t, kind, "faction")?)? })
        }
        "has_spell" => {
            check_args(t, kind, &["in", "all", "garrison"])?;
            let holder = holder_key(t, kind, "in", Holder::Player)?;
            let all = match arg(t, "all") {
                Some(a) => a.as_bool().ok_or_else(|| Problem { message: "`all` is true or false".into(), hint: "e.g. `all = true`".into() })?,
                None => false,
            };
            Ok(Cond::HasSpell { spell: spell_ref(v)?, holder, all })
        }
        _ => unreachable!("kind_of returns a condition"),
    }
}

fn value2_of(v: &Value) -> Res<VarValue> {
    match v {
        Value::Integer(n) => Ok(VarValue::Int(*n)),
        Value::Boolean(b) => Ok(VarValue::Bool(*b)),
        _ => bad("a variable's value is a whole number or true/false", "e.g. `value = 3` or `value = true`"),
    }
}

/// Reads one action (a `then` entry).
pub fn parse_action(t: &Table) -> Res<Action> {
    let kind = kind_of(t, &ACTIONS, "action")?;
    let v = &t[kind];
    match kind {
        "set" => {
            check_args(t, kind, &["value"])?;
            Ok(Action::Set { name: name_of(v, "variable")?, value: value2_of(need(t, kind, "value")?)? })
        }
        "add" => {
            check_args(t, kind, &["value", "min", "max"])?;
            let min = arg(t, "min").map(|m| int_of(m, "`min`")).transpose()?;
            let max = arg(t, "max").map(|m| int_of(m, "`max`")).transpose()?;
            if let (Some(lo), Some(hi)) = (min, max) {
                if lo > hi {
                    return bad("`min` is above `max`", "swap them");
                }
            }
            Ok(Action::Add { name: name_of(v, "variable")?, value: int_of(need(t, kind, "value")?, "`value`")?, min, max })
        }
        "random_var" => {
            check_args(t, kind, &["min", "max"])?;
            let min = int_of(need(t, kind, "min")?, "`min`")?;
            let max = int_of(need(t, kind, "max")?, "`max`")?;
            if min > max {
                return bad("`min` is above `max`", "swap them");
            }
            Ok(Action::RandomVar { name: name_of(v, "variable")?, min, max })
        }
        "dialog" => {
            check_args(t, kind, &[])?;
            Ok(Action::Dialog(name_of(v, "dialog")?))
        }
        "give_gold" => {
            check_args(t, kind, &[])?;
            Ok(Action::GiveGold(int_of(v, "`give_gold`")?))
        }
        "give_item" => {
            check_args(t, kind, &["to", "garrison"])?;
            let to = holder_key(t, kind, "to", Holder::Pack)?;
            Ok(Action::GiveItem { item: item_ref(v)?, to })
        }
        "give_unit" => {
            check_args(t, kind, &["named"])?;
            let named = match arg(t, "named") {
                Some(n) => Some(name_of(n, "named character")?),
                None => None,
            };
            Ok(Action::GiveUnit { unit: unit_ref(v)?, named })
        }
        "give_xp" => {
            check_args(t, kind, &["to", "garrison"])?;
            let to = holder_key(t, kind, "to", Holder::Player)?;
            Ok(Action::GiveXp { amount: int_of(v, "`give_xp`")?, to })
        }
        "spawn_army" => {
            check_args(t, kind, &["at", "faction", "model", "troops"])?;
            let model = match arg(t, "model") {
                Some(m) => match int_of(m, "`model`")? {
                    n @ 0..=12 => n as u8,
                    _ => return bad("`model` is a figure, 0 to 12", "the figure a map army shows, e.g. `model = 5`"),
                },
                None => 5,
            };
            Ok(Action::SpawnArmy {
                army: name_of(v, "army")?,
                at: match at_of(t, kind)? {
                    Place::Cell(x, y) => (x, y),
                    Place::Area(..) => return bad("a spawned army stands on one cell", "give `at = [x, y]`"),
                },
                faction: faction_of(need(t, kind, "faction")?)?,
                model,
                troops: units_of(need(t, kind, "troops")?)?,
            })
        }
        "remove_army" => {
            check_args(t, kind, &[])?;
            Ok(Action::RemoveArmy(name_of(v, "army")?))
        }
        "start_battle" => {
            check_args(t, kind, &["rules"])?;
            let rules = need(t, kind, "rules")?.as_str().unwrap_or_default().to_string();
            let script = rules.strip_prefix("lua:");
            let ok = rules == "dt" || script.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'));
            if !ok {
                return bad("`rules` is `dt` or `lua:<script>` (a script of the package, in `scripts/`)", "e.g. `rules = \"lua:siege\"`");
            }
            Ok(Action::StartBattle { army: name_of(v, "army")?, rules })
        }
        "teleport_hero" => {
            check_args(t, kind, &[])?;
            match place_of(v)? {
                Place::Cell(x, y) => Ok(Action::TeleportHero((x, y))),
                Place::Area(..) => bad("the hero stands on one cell", "give `teleport_hero = [x, y]`"),
            }
        }
        "teleport_army" => {
            check_args(t, kind, &["at", "to"])?;
            let army = name_of(v, "army")?;
            let to = match (arg(t, "at"), arg(t, "to")) {
                (Some(_), None) => match at_of(t, kind)? {
                    Place::Cell(x, y) => Dest::Cell(x, y),
                    Place::Area(..) => return bad("an army stands on one cell", "give `at = [x, y]`"),
                },
                (None, Some(b)) => Dest::Building(name_of(b, "building")?),
                _ => return bad("`teleport_army` needs `at = [x, y]` or `to = \"<building>\"`, not both", "pick one destination"),
            };
            Ok(Action::TeleportArmy { army, to })
        }
        "set_faction" => {
            check_args(t, kind, &["faction", "of"])?;
            Ok(Action::SetFaction { of: entity_of(t, kind, v)?, faction: faction_of(need(t, kind, "faction")?)? })
        }
        "set_relation" => {
            check_args(t, kind, &["towards", "value", "of"])?;
            let value = int_of(need(t, kind, "value")?, "`value`")?;
            if !(-3..=3).contains(&value) {
                return bad("a relation is -3 to 3", "e.g. `value = -3`");
            }
            Ok(Action::SetRelation { of: entity_of(t, kind, v)?, towards: towards_of(need(t, kind, "towards")?)?, value })
        }
        "add_units" => {
            check_args(t, kind, &["troops", "of", "garrison"])?;
            let of = match arg(t, "garrison") {
                None if v.as_str() == Some("garrison") => return bad("`add_units = \"garrison\"` needs `garrison = \"<building>\"`", "name the building in `garrison = \"<id>\"`"),
                None => entity_of(t, kind, v)?,
                Some(_) if arg(t, "of").is_some() => return bad("`add_units` takes `of` or `garrison`, not both", "a garrison is `add_units = \"garrison\", garrison = \"<building>\"`"),
                Some(g) if v.as_str() == Some("garrison") => Entity::Building(name_of(g, "building")?),
                Some(_) => return bad("`add_units` with `garrison` is written `add_units = \"garrison\"`", "the building goes in `garrison = \"<id>\"`"),
            };
            Ok(Action::AddUnits { of, troops: units_of(need(t, kind, "troops")?)? })
        }
        "cast_on_army" => {
            check_args(t, kind, &["in", "minutes", "permanent", "garrison"])?;
            let holder = holder_key(t, kind, "in", Holder::Player)?;
            let permanent = match arg(t, "permanent") {
                Some(p) => p.as_bool().ok_or_else(|| Problem { message: "`permanent` is true or false".into(), hint: "e.g. `permanent = true`".into() })?,
                None => false,
            };
            let until = match (arg(t, "minutes"), permanent) {
                (None, true) => Until::Permanent,
                (Some(m), false) => match int_of(m, "`minutes`")? {
                    n if n >= 1 => Until::Minutes(n as u64),
                    _ => return bad("`minutes` is at least 1", "e.g. `minutes = 120`"),
                },
                _ => return bad("`cast_on_army` needs `minutes = n` or `permanent = true`", "pick how long the spell lasts"),
            };
            Ok(Action::Cast { spell: spell_ref(v)?, holder, until })
        }
        "remove_spells" => {
            check_args(t, kind, &["garrison"])?;
            Ok(Action::RemoveSpells(positional_holder(t, kind, v)?))
        }
        "remove_spell_from_book" => {
            check_args(t, kind, &[])?;
            Ok(Action::ForgetSpell(spell_ref(v)?))
        }
        "army_goal" => {
            const GOALS: [&str; 6] = ["go_to", "go_to_building", "follow", "patrol", "attack", "clear"];
            check_args(t, kind, &GOALS)?;
            let army = name_of(v, "army")?;
            let given: Vec<&str> = GOALS.into_iter().filter(|k| arg(t, k).is_some()).collect();
            let goal = match given.as_slice() {
                [] => return bad("`army_goal` needs one goal", format!("one of: {}", GOALS.join(", "))),
                [one] => goal_of(t, one)?,
                _ => return bad(format!("`army_goal` takes one goal, not {}", given.join(" and ")), "split it into one action per goal"),
            };
            Ok(Action::ArmyGoal { army, goal })
        }
        "army_speed" => {
            check_args(t, kind, &["percent"])?;
            let percent = int_of(need(t, kind, "percent")?, "`percent`")?;
            if !(10..=1000).contains(&percent) {
                return bad("`percent` is 10 to 1000 (100 is the normal pace)", "e.g. `percent = 150`");
            }
            Ok(Action::ArmySpeed { army: name_of(v, "army")?, percent: percent as u32 })
        }
        "hero_figure" => {
            check_args(t, kind, &[])?;
            match v {
                Value::String(s) if s == "default" => Ok(Action::HeroFigure(None)),
                Value::Table(m) => {
                    if let Some(k) = m.keys().find(|k| k.as_str() != "model") {
                        return bad(format!("`{k}` is not a key of `hero_figure`"), "a figure table takes only `model`, e.g. `hero_figure = { model = \"bandits\" }`");
                    }
                    Ok(Action::HeroFigure(Some(Figure::Model(model_code(str_of(need(m, "model", "model")?, "`model`")?)?))))
                }
                _ => Ok(Action::HeroFigure(Some(Figure::Unit(unit_ref(v)?)))),
            }
        }
        "army_figure" => {
            check_args(t, kind, &["unit", "model"])?;
            let figure = match (arg(t, "unit"), arg(t, "model")) {
                (Some(u), None) => Figure::Unit(unit_ref(u)?),
                (None, Some(m)) => Figure::Model(model_code(str_of(m, "`model`")?)?),
                (Some(_), Some(_)) => return bad("`army_figure` takes `unit` or `model`, not both", "pick one: a unit's picture or an army model"),
                (None, None) => return bad("`army_figure` needs `unit` or `model`", "e.g. `unit = \"dt:bandit\"` or `model = \"bandits\"`"),
            };
            Ok(Action::ArmyFigure { army: name_of(v, "army")?, figure })
        }
        "remove_unit" => {
            check_args(t, kind, &["in", "garrison"])?;
            let holder = holder_key(t, kind, "in", Holder::Player)?;
            if matches!(holder, Holder::Pack | Holder::Named(_)) {
                return bad("`remove_unit` takes the player's army, an army or a garrison", "use `in = \"<army id>\"`, `garrison = \"<building>\"` or leave both out");
            }
            Ok(Action::RemoveUnit { named: name_of(v, "named character")?, holder })
        }
        "victory" | "defeat" => {
            let allowed: &[&str] = if kind == "victory" { &["next"] } else { &[] };
            check_args(t, kind, allowed)?;
            match v.as_bool() {
                Some(true) if kind == "victory" => match arg(t, "next") {
                    Some(next) => Ok(Action::VictoryTo(str_of(next, "`next`")?.to_string())),
                    None => Ok(Action::Victory),
                },
                Some(true) => Ok(Action::Defeat),
                _ => bad(format!("`{kind}` is written `{kind} = true`"), "the scenario ends with it"),
            }
        }
        "travel" => {
            check_args(t, kind, &["at"])?;
            let at = match arg(t, "at") {
                Some(a) => Some(str_of(a, "`at`")?.to_string()),
                None => None,
            };
            Ok(Action::Travel { map: str_of(v, "`travel`")?.to_string(), at })
        }
        "ending" => {
            check_args(t, kind, &[])?;
            Ok(Action::Ending(name_of(v, "dialog")?))
        }
        "quest" => {
            check_args(t, kind, &["title", "text"])?;
            Ok(Action::Quest {
                id: name_of(v, "quest")?,
                title: str_of(need(t, kind, "title")?, "`title`")?.to_string(),
                text: str_of(need(t, kind, "text")?, "`text`")?.to_string(),
            })
        }
        "complete_quest" => {
            check_args(t, kind, &[])?;
            Ok(Action::CompleteQuest(name_of(v, "quest")?))
        }
        _ => unreachable!("kind_of returns an action"),
    }
}

/// The goal an `army_goal` action's key names (`key` is the one given).
fn goal_of(t: &Table, key: &str) -> Res<GoalSpec> {
    let v = &t[key];
    Ok(match key {
        "go_to" => match place_of(v)? {
            Place::Cell(x, y) => GoalSpec::GoTo((x, y)),
            Place::Area(..) => return bad("`go_to` is one cell", "give `go_to = [x, y]`"),
        },
        "go_to_building" => GoalSpec::GoToBuilding(name_of(v, "building")?),
        "follow" => GoalSpec::Follow(target_of(v)?),
        "attack" => GoalSpec::Attack(target_of(v)?),
        "patrol" => {
            let Some(a) = v.as_array() else {
                return bad("`patrol` is a list of cells", "e.g. `patrol = [[4, 5], [9, 5]]`");
            };
            let cells = a
                .iter()
                .map(|c| match place_of(c)? {
                    Place::Cell(x, y) => Ok((x, y)),
                    Place::Area(..) => bad("a patrol point is one cell", "e.g. `patrol = [[4, 5], [9, 5]]`"),
                })
                .collect::<Res<Vec<_>>>()?;
            if cells.len() < 2 {
                return bad("a patrol has at least two cells", "e.g. `patrol = [[4, 5], [9, 5]]`");
            }
            GoalSpec::Patrol(cells)
        }
        _ => match v.as_bool() {
            Some(true) => GoalSpec::Clear,
            _ => return bad("`clear` is written `clear = true`", "it sends the army back to the original AI"),
        },
    })
}

/// `of = "building"` makes the entity a building; the default is an army.
fn entity_of(t: &Table, kind: &str, v: &Value) -> Res<Entity> {
    let name = name_of(v, kind)?;
    match arg(t, "of") {
        None => Ok(Entity::Army(name)),
        Some(o) => match str_of(o, "`of`")? {
            "army" => Ok(Entity::Army(name)),
            "building" => Ok(Entity::Building(name)),
            other => bad(format!("`of` is army or building, not `{other}`"), "e.g. `of = \"building\"`"),
        },
    }
}

/// A variable's value as a declared default: `dread = 0` (int) or `held = false` (bool).
pub fn parse_vars(t: &Table) -> Res<Vec<Var>> {
    let mut out = Vec::new();
    for (name, v) in t {
        if !valid_name(name) {
            return bad(format!("`{name}` is not a valid variable name"), "use lowercase letters, digits and _");
        }
        let (kind, default) = match v {
            Value::Integer(n) => (VarKind::Int, VarValue::Int(*n)),
            Value::Boolean(b) => (VarKind::Bool, VarValue::Bool(*b)),
            _ => return bad(format!("variable `{name}` is a whole number or true/false"), "e.g. `dread = 0` or `ford_held = false`"),
        };
        out.push(Var { name: name.clone(), kind, default });
    }
    Ok(out)
}

/// Reads one `[[trigger]]`.
pub fn parse_trigger(raw: &RawTrigger) -> Res<Trigger> {
    if !valid_name(&raw.id) {
        return bad(format!("trigger id `{}` is not valid", raw.id), "use lowercase letters, digits and _");
    }
    let chance = match raw.chance {
        None => 100,
        Some(n @ 0..=100) => n as u8,
        Some(_) => return bad("`chance` is a percent, 0 to 100", "e.g. `chance = 50`"),
    };
    if let Some(m) = &raw.map {
        if m.is_empty() || !m.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return bad(format!("map `{m}` is not a map id"), "a map id is the name of its folder: maps/<id>/, lowercase letters, digits and -");
        }
    }
    Ok(Trigger {
        id: raw.id.clone(),
        map: raw.map.clone(),
        when: parse_event(&raw.when)?,
        cond: raw.cond.iter().map(parse_cond).collect::<Res<_>>()?,
        then: raw.then.iter().map(parse_action).collect::<Res<_>>()?,
        once: raw.once,
        chance,
    })
}

/// Reads one `[[dialog]]` with its choices; the choices' ids are not checked here.
pub fn parse_dialog(raw: &RawDialog) -> Res<Dialog> {
    if !valid_name(&raw.id) {
        return bad(format!("dialog id `{}` is not valid", raw.id), "use lowercase letters, digits and _");
    }
    let speaker = match &raw.speaker {
        Some(s) => Some(Ref::parse(s).map_err(|m| Problem { message: m, hint: "a speaker is a unit: dt:<alias>".into() })?),
        None => None,
    };
    let choices = raw
        .choice
        .iter()
        .map(|c| {
            let c = c.get_ref();
            if !valid_name(&c.id) {
                return bad(format!("choice id `{}` is not valid", c.id), "use lowercase letters, digits and _");
            }
            Ok(Choice {
                id: c.id.clone(),
                text: c.text.clone(),
                cond: c.cond.iter().map(parse_cond).collect::<Res<_>>()?,
                then: c.then.iter().map(parse_action).collect::<Res<_>>()?,
            })
        })
        .collect::<Res<Vec<_>>>()?;
    Ok(Dialog { id: raw.id.clone(), title: raw.title.clone(), text: raw.text.clone(), speaker, choices })
}

/// The story's rules across the whole package (run by `check`): every variable a trigger
/// names is declared, with a type that fits; every dialog a trigger or choice opens exists, and
/// every choice a `dialog_choice` event names; no dialog is unreachable; the choices stay within
/// [`MAX_CHOICES`]; references to units, items and spells resolve.
/// Every action of the package's triggers and of its dialog choices, with the file and line of its
/// table (the checks and the text keys walk them).
pub fn actions(pkg: &Package) -> Vec<(&str, u32, &Action)> {
    let mut out = Vec::new();
    for t in &pkg.triggers {
        out.extend(t.value.then.iter().map(|a| (t.file.as_str(), t.line, a)));
    }
    for d in &pkg.dialogs {
        out.extend(d.value.choices.iter().flat_map(|c| c.then.iter()).map(|a| (d.file.as_str(), d.line, a)));
    }
    out
}

pub fn check(pkg: &Package, r: &mut Report) {
    let mut declared = BTreeSet::new();
    for v in &pkg.vars {
        if !declared.insert(v.value.name.as_str()) {
            r.error_hint(&v.file, Some(v.line), format!("variable `{}` is declared twice", v.value.name), "declare each variable once, in one triggers/*.toml file");
        }
    }
    let vars: BTreeMap<&str, VarKind> = pkg.vars.iter().map(|v| (v.value.name.as_str(), v.value.kind)).collect();
    let dialogs: BTreeMap<&str, &Located<Dialog>> = pkg.dialogs.iter().map(|d| (d.value.id.as_str(), d)).collect();
    let mut opened: BTreeSet<String> = BTreeSet::new();
    let ctx = Ctx { vars: &vars, dialogs: &dialogs, pkg };
    let mut seen = BTreeSet::new();
    for t in &pkg.triggers {
        if !seen.insert(t.value.id.as_str()) {
            r.error_hint(&t.file, Some(t.line), format!("duplicate trigger id `{}`", t.value.id), "trigger ids are unique in the package");
        }
        if let Some(m) = &t.value.map {
            if !pkg.maps.contains_key(m) {
                r.error_hint(&t.file, Some(t.line), format!("no map `{m}` in the package"), format!("its maps are: {}", pkg.maps.keys().cloned().collect::<Vec<_>>().join(", ")));
            }
        }
        if let Event::DialogChoice { dialog, choice } = &t.value.when {
            match dialogs.get(dialog.as_str()) {
                None => r.error_hint(&t.file, Some(t.line), format!("no dialog `{dialog}`"), dialog_hint(dialog, dialogs.keys().copied())),
                Some(d) if !d.value.choices.iter().any(|c| &c.id == choice) => r.error_hint(
                    &t.file,
                    Some(t.line),
                    format!("dialog `{dialog}` has no choice `{choice}`"),
                    format!("its choices are: {}", d.value.choices.iter().map(|c| c.id.as_str()).collect::<Vec<_>>().join(", ")),
                ),
                Some(_) => {}
            }
        }
        if let Event::VarChanged(name) = &t.value.when {
            ctx.var(r, &t.file, t.line, name);
        }
        for c in &t.value.cond {
            ctx.cond(r, &t.file, t.line, c);
        }
        for a in &t.value.then {
            ctx.action(r, &t.file, t.line, a, &mut opened);
        }
    }
    let mut dialog_ids = BTreeSet::new();
    for d in &pkg.dialogs {
        if !dialog_ids.insert(d.value.id.as_str()) {
            r.error_hint(&d.file, Some(d.line), format!("duplicate dialog id `{}`", d.value.id), "dialog ids are unique in the package");
        }
        if d.value.choices.len() > MAX_CHOICES {
            r.error_hint(&d.file, Some(d.line), format!("dialog `{}` has {} choices", d.value.id, d.value.choices.len()), format!("a window shows at most {MAX_CHOICES}: split the dialog"));
        }
        let mut ids = BTreeSet::new();
        for c in &d.value.choices {
            if !ids.insert(c.id.as_str()) {
                r.error_hint(&d.file, Some(d.line), format!("duplicate choice `{}` in dialog `{}`", c.id, d.value.id), "choice ids are unique within a dialog");
            }
            for cond in &c.cond {
                ctx.cond(r, &d.file, d.line, cond);
            }
            for a in &c.then {
                ctx.action(r, &d.file, d.line, a, &mut opened);
            }
        }
        if let Some(s) = &d.value.speaker {
            ctx.unit(r, &d.file, d.line, s);
        }
    }
    // A `complete_quest` names a quest some `quest` action adds (a typo is an error).
    let acts = actions(pkg);
    let added: BTreeSet<&str> = acts.iter().filter_map(|(_, _, a)| if let Action::Quest { id, .. } = a { Some(id.as_str()) } else { None }).collect();
    for (file, line, a) in &acts {
        if let Action::CompleteQuest(id) = a {
            if !added.contains(id.as_str()) {
                let hint = match closest(id, added.iter().copied()) {
                    Some(near) => format!("did you mean `{near}`?"),
                    None => "a `quest` action with this id adds it: check the spelling".to_string(),
                };
                r.error_hint(file, Some(*line), format!("`complete_quest` of quest `{id}`, which no `quest` action adds"), hint);
            }
        }
    }
    for d in &pkg.dialogs {
        if !opened.contains(&d.value.id) {
            r.warning_hint(
                &d.file,
                Some(d.line),
                format!("dialog `{}` is never opened", d.value.id),
                "no trigger or choice has `dialog = \"…\"` for it: it can never show",
            );
        }
    }
}

/// The map references of the story, checked against one map (`map_check` calls this for each map,
/// with its size and its armies and buildings by id). The triggers that apply to the map are those
/// without `map` and those naming it; the dialogs they open, and the dialogs those open, are
/// checked too. Spawned armies are the package's own, so every map knows them.
pub fn check_map_refs(pkg: &Package, map: &str, size: (u32, u32), armies: &BTreeSet<String>, buildings: &BTreeSet<String>, r: &mut Report) {
    let spawned: BTreeSet<String> = pkg
        .triggers
        .iter()
        .flat_map(|t| t.value.then.iter())
        .chain(pkg.dialogs.iter().flat_map(|d| d.value.choices.iter()).flat_map(|c| c.then.iter()))
        .filter_map(|a| if let Action::SpawnArmy { army, .. } = a { Some(army.clone()) } else { None })
        .collect();
    let mut scope = Scope { map, size, armies, buildings, spawned: &spawned, opened: Vec::new(), r };
    for t in pkg.triggers.iter().filter(|t| t.value.map.as_deref().is_none_or(|m| m == map)) {
        scope.trigger(&t.file, t.line, &t.value);
    }
    let mut done = BTreeSet::new();
    while let Some(id) = scope.opened.pop() {
        if !done.insert(id.clone()) {
            continue;
        }
        let Some(d) = pkg.dialogs.iter().find(|d| d.value.id == id) else { continue };
        for c in &d.value.choices {
            for cond in &c.cond {
                scope.cond(&d.file, d.line, cond);
            }
            for a in &c.then {
                scope.action(&d.file, d.line, a);
            }
        }
    }
}

/// The references of one map's story, reported at the line of the trigger or dialog naming them.
struct Scope<'a, 'r> {
    map: &'a str,
    size: (u32, u32),
    armies: &'a BTreeSet<String>,
    buildings: &'a BTreeSet<String>,
    spawned: &'a BTreeSet<String>,
    /// The dialogs the checked actions open, to be checked in turn.
    opened: Vec<String>,
    r: &'r mut Report,
}

impl Scope<'_, '_> {
    fn cell(&mut self, file: &str, line: u32, what: &str, x: i32, y: i32) {
        let (w, h) = (self.size.0 as i32, self.size.1 as i32);
        if x < 0 || y < 0 || x >= w || y >= h {
            self.r.error_hint(file, Some(line), format!("{what} at ({x}, {y}) is outside map `{}` ({w}×{h})", self.map), "cells count from 0 at the top left: x to the right, y down");
        }
    }

    fn place(&mut self, file: &str, line: u32, what: &str, p: &Place) {
        match *p {
            Place::Cell(x, y) => self.cell(file, line, what, x, y),
            Place::Area(_, _, w, h) if w < 1 || h < 1 => {
                self.r.error_hint(file, Some(line), format!("{what}: an area is at least 1 by 1 cells"), "give `at = [x, y, w, h]` with w and h of 1 or more");
            }
            Place::Area(x, y, w, h) => {
                self.cell(file, line, what, x, y);
                self.cell(file, line, what, x + w - 1, y + h - 1);
            }
        }
    }

    fn army(&mut self, file: &str, line: u32, name: &str) {
        if self.armies.contains(name) || self.spawned.contains(name) {
            return;
        }
        let hint = match closest(name, self.armies.iter().map(String::as_str)) {
            Some(near) => format!("did you mean `{near}`?"),
            None if self.armies.is_empty() => format!("map `{}` has no armies: name a spawn_army of the package", self.map),
            None => format!("the armies of map `{}` are: {}", self.map, self.armies.iter().cloned().collect::<Vec<_>>().join(", ")),
        };
        self.r.error_hint(file, Some(line), format!("no army `{name}` in map `{}`", self.map), hint);
    }

    fn building(&mut self, file: &str, line: u32, name: &str) {
        if self.buildings.contains(name) {
            return;
        }
        let hint = match closest(name, self.buildings.iter().map(String::as_str)) {
            Some(near) => format!("did you mean `{near}`?"),
            None => format!("the buildings of map `{}` are: {}", self.map, self.buildings.iter().cloned().collect::<Vec<_>>().join(", ")),
        };
        self.r.error_hint(file, Some(line), format!("no building `{name}` in map `{}`", self.map), hint);
    }

    fn holder(&mut self, file: &str, line: u32, h: &Holder) {
        match h {
            Holder::Army(a) => self.army(file, line, a),
            Holder::Garrison(b) => self.building(file, line, b),
            _ => {}
        }
    }

    fn entity(&mut self, file: &str, line: u32, e: &Entity) {
        match e {
            Entity::Army(a) => self.army(file, line, a),
            Entity::Building(b) => self.building(file, line, b),
        }
    }

    fn trigger(&mut self, file: &str, line: u32, t: &Trigger) {
        match &t.when {
            Event::HeroEnters(p) => self.place(file, line, "hero_enters", p),
            Event::HeroEntersBuilding(b) | Event::BuildingCaptured(b) | Event::BuildingLost(b) => self.building(file, line, b),
            Event::ArmyDefeated(a) => self.army(file, line, a),
            Event::ArmyArrives { army, at, building } => {
                self.army(file, line, army);
                if let Some((x, y)) = at {
                    self.cell(file, line, "army_arrives", *x, *y);
                }
                if let Some(b) = building {
                    self.building(file, line, b);
                }
            }
            _ => {}
        }
        for c in &t.cond {
            self.cond(file, line, c);
        }
        for a in &t.then {
            self.action(file, line, a);
        }
    }

    fn cond(&mut self, file: &str, line: u32, c: &Cond) {
        match c {
            Cond::HeroAt(p) => self.place(file, line, "hero_at", p),
            Cond::ArmyAt { army, at } => {
                self.army(file, line, army);
                self.place(file, line, "army_at", at);
            }
            Cond::ArmyAlive { army, .. } => self.army(file, line, army),
            Cond::BuildingOwner { building, .. } => self.building(file, line, building),
            Cond::HasItem { holder, .. } | Cond::HasSpell { holder, .. } | Cond::HasNamed { holder, .. } => self.holder(file, line, holder),
            _ => {}
        }
    }

    fn action(&mut self, file: &str, line: u32, a: &Action) {
        match a {
            Action::Dialog(d) | Action::Ending(d) => self.opened.push(d.clone()),
            Action::GiveItem { to, .. } | Action::GiveXp { to, .. } => self.holder(file, line, to),
            Action::Cast { holder, .. } | Action::RemoveSpells(holder) => self.holder(file, line, holder),
            Action::SpawnArmy { at, .. } => self.cell(file, line, "spawn_army", at.0, at.1),
            Action::RemoveArmy(army) | Action::StartBattle { army, .. } => self.army(file, line, army),
            Action::TeleportHero(t) => self.cell(file, line, "teleport_hero", t.0, t.1),
            Action::TeleportArmy { army, to } => {
                self.army(file, line, army);
                match to {
                    Dest::Cell(x, y) => self.cell(file, line, "teleport_army", *x, *y),
                    Dest::Building(b) => self.building(file, line, b),
                }
            }
            Action::SetFaction { of, .. } | Action::SetRelation { of, .. } | Action::AddUnits { of, .. } => self.entity(file, line, of),
            Action::ArmyGoal { army, goal } => {
                self.army(file, line, army);
                match goal {
                    GoalSpec::GoTo((x, y)) => self.cell(file, line, "army_goal go_to", *x, *y),
                    GoalSpec::GoToBuilding(b) => self.building(file, line, b),
                    GoalSpec::Follow(target) | GoalSpec::Attack(target) => {
                        if let Target::Army(a) = target {
                            self.army(file, line, a);
                        }
                    }
                    GoalSpec::Patrol(cells) => {
                        for (x, y) in cells {
                            self.cell(file, line, "army_goal patrol", *x, *y);
                        }
                    }
                    GoalSpec::Clear => {}
                }
            }
            Action::ArmySpeed { army, .. } | Action::ArmyFigure { army, .. } => self.army(file, line, army),
            Action::RemoveUnit { holder, .. } => self.holder(file, line, holder),
            _ => {}
        }
    }
}

struct Ctx<'a> {
    vars: &'a BTreeMap<&'a str, VarKind>,
    dialogs: &'a BTreeMap<&'a str, &'a Located<Dialog>>,
    pkg: &'a Package,
}

impl Ctx<'_> {
    fn var(&self, r: &mut Report, file: &str, line: u32, name: &str) -> Option<VarKind> {
        let kind = self.vars.get(name).copied();
        if kind.is_none() {
            let hint = closest(name, self.vars.keys().copied()).map_or_else(
                || "declare it under [vars] in a triggers/*.toml file, e.g. `dread = 0`".to_string(),
                |near| format!("did you mean `{near}`?"),
            );
            r.error_hint(file, Some(line), format!("variable `{name}` is not declared"), hint);
        }
        kind
    }

    fn cond(&self, r: &mut Report, file: &str, line: u32, c: &Cond) {
        match c {
            Cond::Var { name, cmp, value } => {
                if let Some(kind) = self.var(r, file, line, name) {
                    let ok = matches!((kind, value), (VarKind::Int, VarValue::Int(_)) | (VarKind::Bool, VarValue::Bool(_)));
                    if !ok {
                        r.error_hint(file, Some(line), format!("`{name}` is {}, not {}", kind_word(kind), value_word(value)), "compare with a value of the variable's type");
                    } else if kind == VarKind::Bool && !matches!(cmp, Cmp::Eq | Cmp::Ne) {
                        r.error_hint(file, Some(line), format!("`{name}` is true or false: only == and != compare it"), "use == or !=");
                    }
                }
            }
            Cond::HasNamed { unit, .. } => self.unit(r, file, line, unit),
            Cond::HasItem { item, .. } => self.item(r, file, line, item),
            Cond::HasSpell { spell, .. } => self.spell(r, file, line, spell),
            _ => {}
        }
    }

    fn action(&self, r: &mut Report, file: &str, line: u32, a: &Action, opened: &mut BTreeSet<String>) {
        match a {
            Action::Set { name, value } => {
                if let Some(kind) = self.var(r, file, line, name) {
                    let ok = matches!((kind, value), (VarKind::Int, VarValue::Int(_)) | (VarKind::Bool, VarValue::Bool(_)));
                    if !ok {
                        r.error_hint(file, Some(line), format!("`{name}` is {}, not {}", kind_word(kind), value_word(value)), "set it to a value of its type");
                    }
                }
            }
            Action::Add { name, min, max, .. } => {
                if let Some(kind) = self.var(r, file, line, name) {
                    if kind == VarKind::Bool {
                        r.error_hint(file, Some(line), format!("`{name}` is true or false: it cannot be added to or randomised"), "use set = … with true or false");
                    }
                }
                if let (Some(lo), Some(hi)) = (min, max) {
                    if lo > hi {
                        r.error_hint(file, Some(line), "`min` is above `max`", "swap them");
                    }
                }
            }
            Action::RandomVar { name, .. } => {
                if let Some(kind) = self.var(r, file, line, name) {
                    if kind == VarKind::Bool {
                        r.error_hint(file, Some(line), format!("`{name}` is true or false: it cannot be added to or randomised"), "use set = … with true or false");
                    }
                }
            }
            Action::Dialog(d) | Action::Ending(d) => {
                if self.dialogs.contains_key(d.as_str()) {
                    opened.insert(d.clone());
                } else {
                    r.error_hint(file, Some(line), format!("no dialog `{d}`"), dialog_hint(d, self.dialogs.keys().copied()));
                }
            }
            Action::GiveItem { item, .. } => self.item(r, file, line, item),
            Action::GiveUnit { unit, .. } => self.unit(r, file, line, unit),
            Action::HeroFigure(Some(Figure::Unit(unit))) | Action::ArmyFigure { figure: Figure::Unit(unit), .. } => self.unit(r, file, line, unit),
            Action::SpawnArmy { troops, .. } | Action::AddUnits { troops, .. } => {
                for t in troops {
                    self.unit(r, file, line, t);
                }
            }
            Action::Cast { spell, .. } | Action::ForgetSpell(spell) => self.spell(r, file, line, spell),
            _ => {}
        }
    }

    /// A unit reference: `dt:` resolves in the alias table, a local id is a unit of the package.
    fn unit(&self, r: &mut Report, file: &str, line: u32, u: &Ref) {
        match u {
            Ref::Dt { kind, alias } => {
                if aliases::resolve(*kind, alias).is_none() {
                    r.error_hint(file, Some(line), format!("unknown unit `{u}`"), suggest_alias(*kind, alias));
                }
            }
            Ref::Local { id } => {
                if !self.pkg.units.iter().any(|x| &x.value.id == id) {
                    r.error_hint(file, Some(line), format!("no unit `{id}` in the package"), "define it in units/*.toml, or use a dt: unit");
                }
            }
            Ref::Package { package, .. } => self.foreign(r, file, line, package),
        }
    }

    fn item(&self, r: &mut Report, file: &str, line: u32, i: &Ref) {
        match i {
            Ref::Dt { kind, alias } => {
                if aliases::resolve(*kind, alias).is_none() {
                    r.error_hint(file, Some(line), format!("unknown item `{i}`"), suggest_alias(*kind, alias));
                }
            }
            Ref::Local { id } => {
                if !self.pkg.items.iter().any(|x| &x.value.id == id) {
                    r.error_hint(file, Some(line), format!("no item `{id}` in the package"), "define it in items/*.toml, or use a dt: item");
                }
            }
            Ref::Package { package, .. } => self.foreign(r, file, line, package),
        }
    }

    fn spell(&self, r: &mut Report, file: &str, line: u32, s: &Ref) {
        if let Ref::Dt { kind, alias } = s {
            if aliases::resolve(*kind, alias).is_none() {
                r.error_hint(file, Some(line), format!("unknown spell `{s}`"), suggest_alias(*kind, alias));
            }
        }
    }

    fn foreign(&self, r: &mut Report, file: &str, line: u32, package: &str) {
        let listed = self.pkg.manifest.as_ref().is_some_and(|m| m.requires.iter().any(|q| q.split('@').next() == Some(package)));
        if !listed {
            r.error_hint(file, Some(line), format!("package `{package}` is not in requires"), format!("add \"{package}@^1.0\" to requires in package.toml"));
        }
    }
}

fn kind_word(k: VarKind) -> &'static str {
    match k {
        VarKind::Int => "a whole number",
        VarKind::Bool => "true or false",
    }
}

fn value_word(v: &VarValue) -> &'static str {
    match v {
        VarValue::Int(_) => "a whole number",
        VarValue::Bool(_) => "true or false",
    }
}

fn dialog_hint<'a>(name: &str, known: impl Iterator<Item = &'a str>) -> String {
    let known: Vec<&str> = known.collect();
    match closest(name, known.iter().copied()) {
        Some(near) => format!("did you mean `{near}`?"),
        None if known.is_empty() => "the package has no dialogs: write them in dialogs/*.toml".into(),
        None => format!("the dialogs are: {}", known.join(", ")),
    }
}

fn suggest_alias(kind: DtKind, alias: &str) -> String {
    let near = aliases::suggest(kind, alias);
    if near.is_empty() {
        "no alias is close; the table is data/pack/dt-aliases.toml".into()
    } else {
        format!("closest: {}", near.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> Table {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn the_new_forms_read_their_arguments_and_refuse_the_bad_ones() {
        assert_eq!(parse_action(&table("hero_figure = { model = \"bandits\" }")), Ok(Action::HeroFigure(Some(Figure::Model(5)))));
        assert!(parse_action(&table("hero_figure = { model = \"dragons\" }")).unwrap_err().message.contains("not an army model"));
        assert_eq!(
            parse_action(&table("army_figure = \"e\"\nmodel = \"necromancer\"")),
            Ok(Action::ArmyFigure { army: "e".into(), figure: Figure::Model(10) })
        );
        assert!(parse_action(&table("army_figure = \"e\"\nmodel = \"ghosts\"\nunit = \"dt:mage\"")).unwrap_err().message.contains("not both"));
        assert_eq!(
            parse_action(&table("remove_unit = \"varga\"\nin = \"guard\"")),
            Ok(Action::RemoveUnit { named: "varga".into(), holder: Holder::Army("guard".into()) })
        );
        assert_eq!(
            parse_action(&table("add = \"crown\"\nvalue = 1\nmin = -5\nmax = 5")),
            Ok(Action::Add { name: "crown".into(), value: 1, min: Some(-5), max: Some(5) })
        );
        assert!(parse_action(&table("add = \"crown\"\nvalue = 1\nmin = 5\nmax = -5")).unwrap_err().message.contains("above"));
        assert_eq!(parse_event(&table("event = \"building_lost\"\nbuilding = \"abbey\"")), Ok(Event::BuildingLost("abbey".into())));
        assert_eq!(parse_event(&table("event = \"sim_victory\"")), Ok(Event::SimVictory));
        assert_eq!(
            parse_cond(&table("has_named_unit = \"varga\"\nunit = \"dt:halberdier\"\nvalue = false")),
            Ok(Cond::HasNamed { named: "varga".into(), unit: Ref::parse("dt:halberdier").unwrap(), holder: Holder::Player, present: false })
        );
        assert_eq!(parse_cond(&table("army_alive = \"caravan\"\nvalue = false")), Ok(Cond::ArmyAlive { army: "caravan".into(), alive: false }));
        assert!(parse_cond(&table("army_alive = \"caravan\"\nvalue = 1")).unwrap_err().message.contains("true or false"));
    }

    #[test]
    fn events_read_their_arguments() {
        assert_eq!(parse_event(&table("event = \"day_start\"")), Ok(Event::DayStart));
        assert_eq!(parse_event(&table("event = \"hero_enters\"\nat = [12, 7]")), Ok(Event::HeroEnters(Place::Cell(12, 7))));
        assert_eq!(
            parse_event(&table("event = \"hero_enters\"\nat = [10, 5, 4, 3]")),
            Ok(Event::HeroEnters(Place::Area(10, 5, 4, 3)))
        );
        assert_eq!(
            parse_event(&table("event = \"dialog_choice\"\ndialog = \"gate\"\nchoice = \"pay\"")),
            Ok(Event::DialogChoice { dialog: "gate".into(), choice: "pay".into() })
        );
    }

    #[test]
    fn an_unknown_event_hints_the_closest_name() {
        let e = parse_event(&table("event = \"day_strat\"")).unwrap_err();
        assert!(e.message.contains("unknown event `day_strat`"), "{e:?}");
        assert_eq!(e.hint, "did you mean `day_start`?");
    }

    #[test]
    fn an_unknown_key_hints_the_argument_name() {
        let e = parse_cond(&table("var = \"dread\"\nvalu = 3")).unwrap_err();
        assert!(e.message.contains("`valu` is not a key of `var`"), "{e:?}");
        assert_eq!(e.hint, "did you mean `value`?");
    }

    #[test]
    fn conditions_read() {
        assert_eq!(
            parse_cond(&table("var = \"dread\"\nop = \">=\"\nvalue = 3")),
            Ok(Cond::Var { name: "dread".into(), cmp: Cmp::Ge, value: VarValue::Int(3) })
        );
        assert_eq!(parse_cond(&table("random = 30")), Ok(Cond::Random(30)));
        assert_eq!(parse_cond(&table("hero_at = [3, 4, 2, 2]")), Ok(Cond::HeroAt(Place::Area(3, 4, 2, 2))));
        assert_eq!(parse_cond(&table("gold = 100")), Ok(Cond::Gold { amount: 100, cmp: Cmp::Ge }));
        assert_eq!(
            parse_cond(&table("building_owner = \"castle\"\nfaction = \"player\"")),
            Ok(Cond::BuildingOwner { building: "castle".into(), faction: 1 })
        );
        assert_eq!(
            parse_cond(&table("has_spell = \"dt:spell/fireball\"\nin = \"rebels\"\nall = true")),
            Ok(Cond::HasSpell { spell: Ref::parse("dt:spell/fireball").unwrap(), holder: Holder::Army("rebels".into()), all: true })
        );
    }

    #[test]
    fn a_condition_with_two_kinds_or_none_is_an_error() {
        assert!(parse_cond(&table("var = \"a\"\nrandom = 3\nvalue = 1")).unwrap_err().message.contains("names one kind"));
        let e = parse_cond(&table("randm = 3")).unwrap_err();
        assert_eq!(e.hint, "did you mean `random`?");
    }

    #[test]
    fn actions_read_and_reject_bad_shapes() {
        assert_eq!(parse_action(&table("set = \"ford_held\"\nvalue = true")), Ok(Action::Set { name: "ford_held".into(), value: VarValue::Bool(true) }));
        assert_eq!(parse_action(&table("give_gold = 50")), Ok(Action::GiveGold(50)));
        assert_eq!(parse_action(&table("victory = true")), Ok(Action::Victory));
        assert!(parse_action(&table("victory = false")).is_err());
        assert!(parse_action(&table("random_var = \"d\"\nmin = 5\nmax = 1")).unwrap_err().message.contains("above"));
        assert!(parse_action(&table("set_relation = \"rebels\"\ntowards = \"enemy\"\nvalue = 9")).unwrap_err().message.contains("-3 to 3"));
        assert!(parse_action(&table("spawn_army = \"r\"\nat = [1, 2]\nfaction = \"enemy\"\ntroops = [\"dt:item/viking_sword\"]")).is_err());
    }

    #[test]
    fn spawn_and_teleport_read() {
        let a = parse_action(&table("spawn_army = \"rebels\"\nat = [5, 6]\nfaction = \"enemy\"\ntroops = [\"dt:swordsman\"]")).unwrap();
        assert_eq!(
            a,
            Action::SpawnArmy { army: "rebels".into(), at: (5, 6), faction: 4, model: 5, troops: vec![Ref::parse("dt:swordsman").unwrap()] }
        );
        assert_eq!(
            parse_action(&table("teleport_army = \"rebels\"\nto = \"castle\"")),
            Ok(Action::TeleportArmy { army: "rebels".into(), to: Dest::Building("castle".into()) })
        );
    }

    #[test]
    fn a_variable_must_be_int_or_bool() {
        let v = parse_vars(&table("dread = 0\nford_held = false")).unwrap();
        assert_eq!(v.len(), 2);
        assert!(parse_vars(&table("name = \"x\"")).is_err());
    }

    #[test]
    fn a_trigger_and_a_dialog_read_whole() {
        let file: TriggerFile = toml::from_str("[[trigger]]\nid = \"t\"\nwhen = { event = \"game_start\" }\nthen = [ { dialog = \"gate\" } ]\n").unwrap();
        let t = parse_trigger(file.trigger[0].get_ref()).unwrap();
        assert!(t.once);
        assert_eq!(t.chance, 100);
        assert_eq!(t.then, vec![Action::Dialog("gate".into())]);
        let raw: DialogFile = toml::from_str(
            "[[dialog]]\nid = \"gate\"\ntitle = \"gate.title\"\ntext = \"gate.text\"\nspeaker = \"dt:swordsman\"\n\n[[dialog.choice]]\nid = \"pay\"\ntext = \"gate.pay\"\nthen = [ { give_gold = -10 } ]\n",
        )
        .unwrap();
        let d = parse_dialog(raw.dialog[0].get_ref()).unwrap();
        assert_eq!(d.choices.len(), 1);
        assert_eq!(d.choices[0].then, vec![Action::GiveGold(-10)]);
        assert_eq!(d.speaker, Some(Ref::parse("dt:swordsman").unwrap()));
    }

    #[test]
    fn closest_finds_a_near_name_only() {
        assert_eq!(closest("hero_at", CONDITIONS), Some("hero_at"));
        assert_eq!(closest("xyzzy", CONDITIONS), None);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }
}

#[cfg(test)]
mod wish_reader_tests {
    use super::*;

    fn table(text: &str) -> Table {
        toml::from_str(text).unwrap()
    }

    fn goal(text: &str) -> Res<Action> {
        parse_action(&table(text))
    }

    #[test]
    fn army_goal_reads_each_goal() {
        assert_eq!(goal("army_goal = \"ghost\"\ngo_to = [4, 5]"), Ok(Action::ArmyGoal { army: "ghost".into(), goal: GoalSpec::GoTo((4, 5)) }));
        assert_eq!(goal("army_goal = \"ghost\"\ngo_to_building = \"tower\""), Ok(Action::ArmyGoal { army: "ghost".into(), goal: GoalSpec::GoToBuilding("tower".into()) }));
        assert_eq!(goal("army_goal = \"ghost\"\nfollow = \"hero\""), Ok(Action::ArmyGoal { army: "ghost".into(), goal: GoalSpec::Follow(Target::Hero) }));
        assert_eq!(goal("army_goal = \"ghost\"\nfollow = \"guard\""), Ok(Action::ArmyGoal { army: "ghost".into(), goal: GoalSpec::Follow(Target::Army("guard".into())) }));
        assert_eq!(
            goal("army_goal = \"ghost\"\npatrol = [[4, 5], [9, 5]]"),
            Ok(Action::ArmyGoal { army: "ghost".into(), goal: GoalSpec::Patrol(vec![(4, 5), (9, 5)]) })
        );
        assert_eq!(goal("army_goal = \"ghost\"\nattack = \"hero\""), Ok(Action::ArmyGoal { army: "ghost".into(), goal: GoalSpec::Attack(Target::Hero) }));
        assert_eq!(goal("army_goal = \"ghost\"\nclear = true"), Ok(Action::ArmyGoal { army: "ghost".into(), goal: GoalSpec::Clear }));
    }

    #[test]
    fn army_goal_takes_exactly_one_goal_of_a_known_shape() {
        assert!(goal("army_goal = \"ghost\"").unwrap_err().message.contains("needs one goal"));
        let two = goal("army_goal = \"ghost\"\ngo_to = [1, 1]\nclear = true").unwrap_err();
        assert!(two.message.contains("takes one goal"), "{two:?}");
        assert!(goal("army_goal = \"ghost\"\npatrol = [[4, 5]]").unwrap_err().message.contains("at least two"));
        assert!(goal("army_goal = \"ghost\"\ngo_to = [1, 1, 2, 2]").unwrap_err().message.contains("one cell"));
        assert!(goal("army_goal = \"ghost\"\nclear = false").unwrap_err().message.contains("clear = true"));
        let typo = goal("army_goal = \"ghost\"\ngo_too = [1, 1]").unwrap_err();
        assert_eq!(typo.hint, "did you mean `go_to`?", "{typo:?}");
    }

    #[test]
    fn army_speed_and_the_figures_read() {
        assert_eq!(parse_action(&table("army_speed = \"ghost\"\npercent = 150")), Ok(Action::ArmySpeed { army: "ghost".into(), percent: 150 }));
        assert!(parse_action(&table("army_speed = \"ghost\"\npercent = 5")).unwrap_err().message.contains("10 to 1000"));
        assert_eq!(parse_action(&table("hero_figure = \"default\"")), Ok(Action::HeroFigure(None)));
        assert_eq!(parse_action(&table("hero_figure = \"dt:bandit\"")), Ok(Action::HeroFigure(Some(Figure::Unit(Ref::parse("dt:bandit").unwrap())))));
        assert_eq!(
            parse_action(&table("army_figure = \"ghost\"\nunit = \"dt:mage\"")),
            Ok(Action::ArmyFigure { army: "ghost".into(), figure: Figure::Unit(Ref::parse("dt:mage").unwrap()) })
        );
        assert!(parse_action(&table("army_figure = \"ghost\"")).unwrap_err().message.contains("needs `unit`"));
    }

    #[test]
    fn army_arrives_reads_a_cell_or_a_building() {
        assert_eq!(
            parse_event(&table("event = \"army_arrives\"\narmy = \"ghost\"\nat = [4, 5]")),
            Ok(Event::ArmyArrives { army: "ghost".into(), at: Some((4, 5)), building: None })
        );
        assert_eq!(
            parse_event(&table("event = \"army_arrives\"\narmy = \"ghost\"\nbuilding = \"tower\"")),
            Ok(Event::ArmyArrives { army: "ghost".into(), at: None, building: Some("tower".into()) })
        );
        assert_eq!(parse_event(&table("event = \"army_arrives\"\narmy = \"ghost\"")), Ok(Event::ArmyArrives { army: "ghost".into(), at: None, building: None }));
        assert!(parse_event(&table("event = \"army_arrives\"\narmy = \"g\"\nat = [1, 1]\nbuilding = \"t\"")).unwrap_err().message.contains("not both"));
    }

    #[test]
    fn a_garrison_is_a_holder_of_spells_items_units_and_named_characters() {
        assert_eq!(
            parse_cond(&table("has_spell = \"dt:spell/fireball\"\ngarrison = \"tower\"")),
            Ok(Cond::HasSpell { spell: Ref::parse("dt:spell/fireball").unwrap(), holder: Holder::Garrison("tower".into()), all: false })
        );
        assert_eq!(
            parse_action(&table("cast_on_army = \"dt:spell/fireball\"\ngarrison = \"tower\"\nminutes = 60")),
            Ok(Action::Cast { spell: Ref::parse("dt:spell/fireball").unwrap(), holder: Holder::Garrison("tower".into()), until: Until::Minutes(60) })
        );
        assert_eq!(
            parse_action(&table("give_item = \"dt:item/viking_sword\"\ngarrison = \"tower\"")),
            Ok(Action::GiveItem { item: Ref::parse("dt:item/viking_sword").unwrap(), to: Holder::Garrison("tower".into()) })
        );
        assert_eq!(
            parse_action(&table("give_xp = 40\ngarrison = \"tower\"")),
            Ok(Action::GiveXp { amount: 40, to: Holder::Garrison("tower".into()) })
        );
        assert_eq!(
            parse_action(&table("add_units = \"garrison\"\ngarrison = \"tower\"\ntroops = [\"dt:swordsman\"]")),
            Ok(Action::AddUnits { of: Entity::Building("tower".into()), troops: vec![Ref::parse("dt:swordsman").unwrap()] })
        );
        assert_eq!(parse_action(&table("remove_spells = \"garrison\"\ngarrison = \"tower\"")), Ok(Action::RemoveSpells(Holder::Garrison("tower".into()))));
        assert_eq!(
            parse_cond(&table("has_named_unit = \"ivo\"\nunit = \"dt:knight\"\ngarrison = \"tower\"")),
            Ok(Cond::HasNamed { named: "ivo".into(), unit: Ref::parse("dt:knight").unwrap(), holder: Holder::Garrison("tower".into()), present: true })
        );
        assert_eq!(
            parse_cond(&table("has_named_unit = \"ivo\"\nunit = \"dt:knight\"\nin = \"guard\"")),
            Ok(Cond::HasNamed { named: "ivo".into(), unit: Ref::parse("dt:knight").unwrap(), holder: Holder::Army("guard".into()), present: true })
        );
    }

    #[test]
    fn a_holder_is_given_once_and_a_positional_garrison_needs_its_key() {
        let both = parse_cond(&table("has_spell = \"dt:spell/fireball\"\nin = \"guard\"\ngarrison = \"tower\"")).unwrap_err();
        assert!(both.message.contains("not both"), "{both:?}");
        let no_key = parse_action(&table("remove_spells = \"garrison\"")).unwrap_err();
        assert!(no_key.message.contains("holder") || no_key.message.contains("garrison"), "{no_key:?}");
        assert!(parse_cond(&table("has_item = \"dt:item/viking_sword\"\nin = \"named:ivo\"")).is_err(), "a named character holds no item lookup");
        assert!(parse_cond(&table("has_named_unit = \"ivo\"\nunit = \"dt:knight\"\nin = \"pack\"")).is_err());
    }
}

#[cfg(test)]
mod check_tests {
    use super::*;
    use crate::pack::check::check as check_package;
    use crate::pack::diag::Severity;
    use crate::pack::files::Files;

    const MANIFEST: &str = "id = \"demo\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"story.title\"\nauthor = \"Razdor\"\nlicense = \"MIT\"\n";
    const TEXTS: &str = "[story]\ntitle = \"Story\"\n\n[gate]\ntitle = \"The gate\"\ntext = \"Who goes there?\"\npay = \"Pay\"\nrefuse = \"Refuse\"\n";

    /// A package with one triggers file and one dialogs file.
    fn report(triggers: &str, dialogs: &str) -> Report {
        let mut f = Files::default();
        f.insert("package.toml", MANIFEST.as_bytes());
        f.insert("texts/en.toml", TEXTS.as_bytes());
        f.insert("triggers/story.toml", triggers.as_bytes());
        f.insert("dialogs/gate.toml", dialogs.as_bytes());
        check_package(&Package::parse(f).0)
    }

    const VARS: &str = "[vars]\ndread = 0\nheld = false\n\n";
    const GATE: &str = "[[dialog]]\nid = \"gate\"\ntitle = \"gate.title\"\ntext = \"gate.text\"\n\n[[dialog.choice]]\nid = \"pay\"\ntext = \"gate.pay\"\nthen = [ { give_gold = -5 } ]\n\n[[dialog.choice]]\nid = \"refuse\"\ntext = \"gate.refuse\"\n";

    fn messages(r: &Report, severity: Severity) -> Vec<String> {
        r.diagnostics.iter().filter(|d| d.severity == severity).map(|d| d.message.clone()).collect()
    }

    #[test]
    fn a_consistent_story_checks_clean() {
        let triggers = format!("{VARS}[[trigger]]\nid = \"start\"\nwhen = {{ event = \"game_start\" }}\nthen = [ {{ dialog = \"gate\" }} ]\n\n[[trigger]]\nid = \"on_pay\"\nwhen = {{ event = \"dialog_choice\", dialog = \"gate\", choice = \"pay\" }}\nif = [ {{ var = \"dread\", op = \">=\", value = 0 }} ]\nthen = [ {{ set = \"held\", value = true }}, {{ add = \"dread\", value = 1 }} ]\n");
        let r = report(&triggers, GATE);
        assert!(!r.has_errors() && r.warnings() == 0, "{}", r.to_text());
    }

    #[test]
    fn an_undeclared_variable_is_an_error_with_a_near_name() {
        let triggers = format!("{VARS}[[trigger]]\nid = \"t\"\nwhen = {{ event = \"game_start\" }}\nthen = [ {{ add = \"dreed\", value = 1 }} ]\n");
        let r = report(&triggers, GATE);
        let d = r.diagnostics.iter().find(|d| d.message.contains("variable `dreed` is not declared")).unwrap_or_else(|| panic!("{}", r.to_text()));
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.hint.as_deref(), Some("did you mean `dread`?"));
        assert_eq!(d.file, "triggers/story.toml");
        assert_eq!(d.line, Some(5), "reported at its trigger's table");
    }

    #[test]
    fn a_value_of_the_wrong_type_is_an_error() {
        let triggers = format!("{VARS}[[trigger]]\nid = \"t\"\nwhen = {{ event = \"game_start\" }}\nthen = [ {{ set = \"held\", value = 3 }} ]\n\n[[trigger]]\nid = \"u\"\nwhen = {{ event = \"game_start\" }}\nif = [ {{ var = \"held\", op = \"<\", value = true }} ]\nthen = [ {{ add = \"held\", value = 1 }} ]\n");
        let r = report(&triggers, GATE);
        let errs = messages(&r, Severity::Error);
        assert!(errs.iter().any(|m| m == "`held` is true or false, not a whole number"), "{}", r.to_text());
        assert!(errs.iter().any(|m| m == "`held` is true or false: only == and != compare it"), "{}", r.to_text());
        assert!(errs.iter().any(|m| m.contains("`held` is true or false: it cannot be added to")), "{}", r.to_text());
    }

    #[test]
    fn an_unknown_dialog_or_choice_is_an_error() {
        let triggers = format!("{VARS}[[trigger]]\nid = \"t\"\nwhen = {{ event = \"game_start\" }}\nthen = [ {{ dialog = \"gatee\" }} ]\n\n[[trigger]]\nid = \"u\"\nwhen = {{ event = \"dialog_choice\", dialog = \"gate\", choice = \"run\" }}\nthen = [ {{ give_gold = 1 }} ]\n");
        let r = report(&triggers, GATE);
        let errs = r.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect::<Vec<_>>();
        let near = errs.iter().find(|d| d.message == "no dialog `gatee`").unwrap_or_else(|| panic!("{}", r.to_text()));
        assert_eq!(near.hint.as_deref(), Some("did you mean `gate`?"));
        assert!(errs.iter().any(|d| d.message == "dialog `gate` has no choice `run`" && d.hint.as_deref() == Some("its choices are: pay, refuse")), "{}", r.to_text());
    }

    #[test]
    fn a_dialog_nothing_opens_is_a_warning() {
        let triggers = format!("{VARS}[[trigger]]\nid = \"t\"\nwhen = {{ event = \"game_start\" }}\nthen = [ {{ give_gold = 1 }} ]\n");
        let r = report(&triggers, GATE);
        assert!(!r.has_errors(), "{}", r.to_text());
        let warned = messages(&r, Severity::Warning);
        assert_eq!(warned, vec!["dialog `gate` is never opened".to_string()], "{}", r.to_text());
    }

    #[test]
    fn a_choice_can_open_a_dialog_too() {
        let dialogs = format!("{GATE}\n[[dialog]]\nid = \"after\"\ntitle = \"gate.title\"\ntext = \"gate.text\"\n");
        let dialogs = dialogs.replace("then = [ { give_gold = -5 } ]", "then = [ { dialog = \"after\" } ]");
        let triggers = format!("{VARS}[[trigger]]\nid = \"t\"\nwhen = {{ event = \"game_start\" }}\nthen = [ {{ dialog = \"gate\" }} ]\n");
        let r = report(&triggers, &dialogs);
        assert!(!r.has_errors() && r.warnings() == 0, "{}", r.to_text());
    }

    #[test]
    fn a_missing_text_key_is_an_error() {
        let dialogs = GATE.replace("title = \"gate.title\"", "title = \"gate.nope\"");
        let triggers = format!("{VARS}[[trigger]]\nid = \"t\"\nwhen = {{ event = \"game_start\" }}\nthen = [ {{ dialog = \"gate\" }} ]\n");
        let r = report(&triggers, &dialogs);
        assert!(messages(&r, Severity::Error).iter().any(|m| m.contains("text key `gate.nope` has no text")), "{}", r.to_text());
    }

    #[test]
    fn more_than_five_choices_is_an_error() {
        let mut dialogs = String::from("[[dialog]]\nid = \"gate\"\ntitle = \"gate.title\"\ntext = \"gate.text\"\n");
        for k in 0..6 {
            dialogs.push_str(&format!("\n[[dialog.choice]]\nid = \"c{k}\"\ntext = \"gate.pay\"\n"));
        }
        let triggers = format!("{VARS}[[trigger]]\nid = \"t\"\nwhen = {{ event = \"game_start\" }}\nthen = [ {{ dialog = \"gate\" }} ]\n");
        let r = report(&triggers, &dialogs);
        assert!(messages(&r, Severity::Error).iter().any(|m| m == "dialog `gate` has 6 choices"), "{}", r.to_text());
    }

    #[test]
    fn an_unknown_unit_or_spell_is_an_error() {
        let triggers = format!("{VARS}[[trigger]]\nid = \"t\"\nwhen = {{ event = \"game_start\" }}\nthen = [ {{ give_unit = \"dt:knigth\" }} ]\n\n[[trigger]]\nid = \"u\"\nwhen = {{ event = \"game_start\" }}\nif = [ {{ has_spell = \"dt:spell/nofire\" }} ]\nthen = [ {{ give_gold = 1 }} ]\n");
        let r = report(&triggers, GATE);
        let errs = messages(&r, Severity::Error);
        assert!(errs.iter().any(|m| m.starts_with("unknown unit `dt:knigth`")), "{}", r.to_text());
        assert!(errs.iter().any(|m| m.starts_with("unknown spell `dt:spell/nofire`")), "{}", r.to_text());
    }

    #[test]
    fn a_variable_declared_twice_is_an_error() {
        let mut f = Files::default();
        f.insert("package.toml", MANIFEST.as_bytes());
        f.insert("texts/en.toml", TEXTS.as_bytes());
        f.insert("triggers/a.toml", b"[vars]\ndread = 0\n");
        f.insert("triggers/b.toml", b"[vars]\ndread = 2\n");
        let r = check_package(&Package::parse(f).0);
        assert!(messages(&r, Severity::Error).iter().any(|m| m == "variable `dread` is declared twice"), "{}", r.to_text());
    }

    #[test]
    fn bad_shapes_are_reported_with_their_line_and_a_hint() {
        let triggers = format!("{VARS}[[trigger]]\nid = \"t\"\nwhen = {{ event = \"day_strat\" }}\nthen = [ {{ give_gold = 1 }} ]\n\n[[trigger]]\nid = \"u\"\nwhen = {{ event = \"game_start\" }}\nthen = [ {{ give_gld = 1 }} ]\n");
        let r = report(&triggers, GATE);
        let errs: Vec<&crate::pack::diag::Diagnostic> = r.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
        let e1 = errs.iter().find(|d| d.message.contains("unknown event `day_strat`")).unwrap_or_else(|| panic!("{}", r.to_text()));
        assert_eq!((e1.line, e1.hint.as_deref()), (Some(5), Some("did you mean `day_start`?")));
        let e2 = errs.iter().find(|d| d.message.contains("unknown action `give_gld`")).unwrap_or_else(|| panic!("{}", r.to_text()));
        assert_eq!((e2.line, e2.hint.as_deref()), (Some(10), Some("did you mean `give_gold`?")));
    }
}

#[cfg(test)]
mod map_refs_tests {
    use super::*;
    use crate::pack::check::check;
    use crate::pack::files::Files;
    use crate::pack::terrain::{encode_rgb, DEFAULT_PALETTE};

    /// The map `m`: a 50×50 meadow, a knight hero, an enemy army `e` at (30, 30) and an enemy
    /// building `t` at (7, 7).
    const MAP: &str = "title = \"t.x\"\nstart_date = \"1300-04-01\"\nrelations = [[3,3,0,-3],[3,3,0,-3],[0,0,3,-3],[-3,-3,-3,3]]\n\n[hero]\ncell = [5, 5]\nclasses = [\"knight\"]\ngold = 100\ntroops = [{ unit = \"dt:militiaman\", level = 1, count = 3 }]\n\n[[army]]\nid = \"e\"\nname = \"t.x\"\ncell = [30, 30]\nfaction = \"enemy\"\nmodel = \"bandits\"\ntroops = [{ unit = \"dt:bandit\", level = 1, count = 2 }]\n\n[[building]]\nid = \"t\"\ntype = \"town\"\nname = \"t.x\"\ncell = [18, 18]\nfaction = \"enemy\"\n\n[victory]\nkind = \"reach\"\ncell = [45, 45]\nmessage = \"t.x\"\n";

    /// A package with the map `m` and the story (`triggers/main.toml`'s text after its `[vars]`).
    fn package(story: &str, dialogs: &str) -> Package {
        let mut f = Files::default();
        f.insert("package.toml", "id = \"demo\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"t.x\"\nauthor = \"A\"\nlicense = \"MIT\"\n");
        f.insert("texts/en.toml", "[t]\nx = \"x\"\n");
        f.insert("maps/m/map.toml", MAP);
        f.insert("maps/m/terrain.png", encode_rgb(50, 50, |_, _| DEFAULT_PALETTE[6].1));
        f.insert("triggers/main.toml", format!("[vars]\nn = 0\n\n{story}"));
        f.insert("dialogs/d.toml", dialogs);
        Package::parse(f).0
    }

    /// The report's errors as text (the checker's lines).
    fn errors(story: &str, dialogs: &str) -> String {
        let r = check(&package(story, dialogs));
        r.diagnostics.iter().filter(|d| d.severity == crate::pack::Severity::Error).map(|d| format!("{}:{}: {} ({})", d.file, d.line.unwrap_or(0), d.message, d.hint.as_deref().unwrap_or(""))).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_clean_story_on_a_map_checks_clean() {
        let story = "[[trigger]]\nid = \"a\"\nwhen = { event = \"hero_enters\", at = [10, 10, 5, 5] }\nif = [ { army_at = \"e\", at = [30, 30] } ]\nthen = [ { spawn_army = \"host\", at = [20, 20], faction = \"enemy\", troops = [\"dt:knight\"] }, { dialog = \"d\" } ]\n\n[[trigger]]\nid = \"b\"\nwhen = { event = \"army_defeated\", army = \"host\" }\nthen = [ { victory = true } ]\n";
        let dialogs = "[[dialog]]\nid = \"d\"\ntitle = \"t.x\"\ntext = \"t.x\"\n\n[[dialog.choice]]\nid = \"c\"\ntext = \"t.x\"\nthen = [ { teleport_army = \"e\", to = \"t\" } ]\n";
        assert_eq!(errors(story, dialogs), "", "a story whose names are on the map");
    }

    #[test]
    fn an_army_the_map_lacks_is_an_error_naming_the_map() {
        let story = "[[trigger]]\nid = \"a\"\nwhen = { event = \"army_defeated\", army = \"ee\" }\nthen = [ { give_gold = 1 } ]\n";
        let e = errors(story, "");
        assert!(e.contains("no army `ee` in map `m`"), "{e}");
        assert!(e.contains("did you mean `e`?"), "the closest army is named: {e}");
    }

    #[test]
    fn a_building_the_map_lacks_is_an_error() {
        let story = "[[trigger]]\nid = \"a\"\nwhen = { event = \"building_captured\", building = \"keep\" }\nthen = [ { give_gold = 1 } ]\n";
        assert!(errors(story, "").contains("no building `keep` in map `m`"));
    }

    #[test]
    fn a_cell_outside_the_map_is_an_error_with_the_size() {
        let story = "[[trigger]]\nid = \"a\"\nwhen = { event = \"hero_enters\", at = [50, 3] }\nthen = [ { give_gold = 1 } ]\n\n[[trigger]]\nid = \"b\"\nwhen = { event = \"day_start\" }\nthen = [ { teleport_hero = [3, 60] } ]\n";
        let e = errors(story, "");
        assert!(e.contains("hero_enters at (50, 3) is outside map `m` (50×50)"), "{e}");
        assert!(e.contains("teleport_hero at (3, 60) is outside"), "{e}");
    }

    #[test]
    fn a_dialog_is_checked_on_the_maps_that_open_it() {
        let story = "[[trigger]]\nid = \"a\"\nwhen = { event = \"game_start\" }\nthen = [ { dialog = \"d\" } ]\n";
        let dialogs = "[[dialog]]\nid = \"d\"\ntitle = \"t.x\"\ntext = \"t.x\"\n\n[[dialog.choice]]\nid = \"c\"\ntext = \"t.x\"\nthen = [ { remove_army = \"ghost\" } ]\n";
        let e = errors(story, dialogs);
        assert!(e.contains("no army `ghost` in map `m`") && e.contains("dialogs/d.toml:"), "{e}");
    }

    #[test]
    fn a_map_scope_must_name_a_map_of_the_package() {
        let story = "[[trigger]]\nid = \"a\"\nmap = \"nowhere\"\nwhen = { event = \"game_start\" }\nthen = [ { give_gold = 1 } ]\n";
        assert!(errors(story, "").contains("no map `nowhere` in the package"));
    }

    #[test]
    fn spawned_armies_are_known_on_every_map() {
        let story = "[[trigger]]\nid = \"a\"\nwhen = { event = \"week_start\" }\nthen = [ { spawn_army = \"host\", at = [20, 20], faction = \"enemy\", troops = [\"dt:knight\"] } ]\n\n[[trigger]]\nid = \"b\"\nwhen = { event = \"army_defeated\", army = \"host\" }\nthen = [ { victory = true } ]\n";
        assert_eq!(errors(story, ""), "");
    }

    #[test]
    fn army_goals_figures_speed_and_arrivals_on_the_map_check_clean() {
        let story = "[[trigger]]\nid = \"walk\"\nwhen = { event = \"game_start\" }\nthen = [ { army_goal = \"e\", go_to = [12, 12] }, { army_speed = \"e\", percent = 150 }, { hero_figure = \"dt:bandit\" }, { army_figure = \"e\", unit = \"dt:bandit\" } ]\n\n[[trigger]]\nid = \"in\"\nwhen = { event = \"army_arrives\", army = \"e\", building = \"t\" }\nthen = [ { army_goal = \"e\", patrol = [[10, 10], [20, 20]] }, { add_units = \"garrison\", garrison = \"t\", troops = [\"dt:swordsman\"] } ]\n\n[[trigger]]\nid = \"stop\"\nwhen = { event = \"army_arrives\", army = \"e\", at = [12, 12] }\nthen = [ { army_goal = \"e\", clear = true }, { give_item = \"dt:item/viking_sword\", garrison = \"t\" } ]\n\n[[trigger]]\nid = \"chase\"\nwhen = { event = \"day_start\" }\nif = [ { has_spell = \"dt:spell/healing\", garrison = \"t\" } ]\nthen = [ { army_goal = \"e\", follow = \"hero\" }, { army_goal = \"e\", go_to_building = \"t\" }, { army_goal = \"e\", attack = \"e\" } ]\n";
        assert_eq!(errors(story, ""), "", "every goal, arrival, figure and garrison name is on the map");
    }

    #[test]
    fn a_goal_names_a_cell_or_building_the_map_lacks() {
        let story = "[[trigger]]\nid = \"a\"\nwhen = { event = \"game_start\" }\nthen = [ { army_goal = \"e\", go_to = [60, 3] } ]\n\n[[trigger]]\nid = \"b\"\nwhen = { event = \"day_start\" }\nthen = [ { army_goal = \"e\", patrol = [[1, 1], [3, 99]] } ]\n\n[[trigger]]\nid = \"c\"\nwhen = { event = \"day_start\" }\nthen = [ { army_goal = \"e\", go_to_building = \"keep\" } ]\n";
        let e = errors(story, "");
        assert!(e.contains("army_goal go_to at (60, 3) is outside map `m`"), "{e}");
        assert!(e.contains("army_goal patrol at (3, 99) is outside map `m`"), "{e}");
        assert!(e.contains("no building `keep` in map `m`"), "{e}");
    }

    #[test]
    fn a_goal_or_figure_names_an_army_the_map_lacks() {
        let story = "[[trigger]]\nid = \"a\"\nwhen = { event = \"game_start\" }\nthen = [ { army_goal = \"ghost\", go_to = [4, 4] }, { army_figure = \"ghost\", unit = \"dt:knight\" } ]\n\n[[trigger]]\nid = \"b\"\nwhen = { event = \"army_arrives\", army = \"ghost\" }\nthen = [ { army_speed = \"ghost\", percent = 50 } ]\n";
        let e = errors(story, "");
        assert!(e.contains("no army `ghost` in map `m`"), "{e}");
        assert!(e.matches("no army `ghost`").count() >= 4, "every use is named: {e}");
    }

    #[test]
    fn a_garrison_names_a_building_of_the_map() {
        let story = "[[trigger]]\nid = \"a\"\nwhen = { event = \"game_start\" }\nthen = [ { cast_on_army = \"dt:spell/healing\", garrison = \"castle\", minutes = 5 } ]\n";
        assert!(errors(story, "").contains("no building `castle` in map `m`"));
    }

    #[test]
    fn a_figure_names_a_unit_the_package_lacks() {
        let story = "[[trigger]]\nid = \"a\"\nwhen = { event = \"game_start\" }\nthen = [ { hero_figure = \"local:nobody\" } ]\n";
        assert!(!errors(story, "").is_empty());
    }

    // The diary's actions: `quest` and `complete_quest` read, and their checks.

    fn diary_table(text: &str) -> Table {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn quest_and_complete_quest_read() {
        assert_eq!(
            parse_action(&diary_table("quest = \"ford\"\ntitle = \"t.a\"\ntext = \"t.b\"")),
            Ok(Action::Quest { id: "ford".into(), title: "t.a".into(), text: "t.b".into() })
        );
        assert_eq!(parse_action(&diary_table("complete_quest = \"ford\"")), Ok(Action::CompleteQuest("ford".into())));
    }

    #[test]
    fn a_quest_needs_its_title_and_text_and_hints_a_typo() {
        assert!(parse_action(&diary_table("quest = \"ford\"\ntitle = \"t.a\"")).is_err(), "no text");
        let e = parse_action(&diary_table("quest = \"ford\"\ntitle = \"t.a\"\ntexy = \"t.b\"")).unwrap_err();
        assert_eq!(e.hint, "did you mean `text`?", "{e:?}");
    }

    #[test]
    fn a_complete_quest_must_name_a_quest_some_action_adds() {
        let story = "[[trigger]]\nid = \"a\"\nwhen = { event = \"game_start\" }\nthen = [ { quest = \"ford\", title = \"t.x\", text = \"t.x\" } ]\n\n[[trigger]]\nid = \"b\"\nwhen = { event = \"day_start\" }\nthen = [ { complete_quest = \"fotd\" } ]\n";
        let errs = errors(story, "");
        assert!(errs.contains("`complete_quest` of quest `fotd`, which no `quest` action adds"), "{errs}");
        assert!(errs.contains("did you mean `ford`?"), "{errs}");
    }

    #[test]
    fn a_quest_added_in_a_choice_completes_and_a_text_key_is_checked() {
        let dialogs = "[[dialog]]\nid = \"d\"\ntitle = \"t.x\"\ntext = \"t.x\"\n\n[[dialog.choice]]\nid = \"take\"\ntext = \"t.x\"\nthen = [ { quest = \"ford\", title = \"t.x\", text = \"t.x\" } ]\n";
        let story = "[[trigger]]\nid = \"a\"\nwhen = { event = \"game_start\" }\nthen = [ { dialog = \"d\" } ]\n\n[[trigger]]\nid = \"b\"\nwhen = { event = \"dialog_choice\", dialog = \"d\", choice = \"take\" }\nthen = [ { complete_quest = \"ford\" } ]\n";
        assert_eq!(errors(story, dialogs), "", "a choice's quest is one a trigger may complete");
        let missing = "[[trigger]]\nid = \"a\"\nwhen = { event = \"game_start\" }\nthen = [ { quest = \"ford\", title = \"t.nope\", text = \"t.x\" } ]\n";
        assert!(errors(missing, "").contains("text key `t.nope` has no text in any language"), "a quest's title is a text key");
    }
}
