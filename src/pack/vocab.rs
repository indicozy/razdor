//! The vocabulary of trigger files: every event (`when`), condition (`if`) and action (`then`) that
//! [`super::triggers`] reads, with its keys, one sentence and one TOML example. It is the source of
//! `razdor-pack schema` (text and `--json`), of the studio's `schema` tool and of the JSON Schema
//! that editors read for trigger and dialog files. The parser's name lists (`triggers::EVENTS`,
//! `CONDITIONS`, `ACTIONS`) and its argument names are checked against this table by the tests
//! below, so the two cannot drift: a name the parser accepts has an entry, and every entry parses.
//!
//! `aliases` are names agents invented for an action that exists (`heal_army` for `restore_army`):
//! they are never accepted by the parser, and `check` suggests their canonical name.

use serde_json::{json, Map, Value};

use super::triggers::MODELS;

/// The three tables of a trigger file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Event,
    Condition,
    Action,
}

impl Section {
    /// The heading of the section in the text output.
    fn heading(self) -> &'static str {
        match self {
            Section::Event => "Events (when = { event = … })",
            Section::Condition => "Conditions (if = [ { … } ])",
            Section::Action => "Actions (then = [ { … } ])",
        }
    }

    /// The TOML key that holds the section's tables in a trigger or dialog.
    fn toml_key(self) -> &'static str {
        match self {
            Section::Event => "when",
            Section::Condition => "if",
            Section::Action => "then",
        }
    }
}

/// The type of a key's value, as the reader takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    /// A name of a package thing (`[a-z0-9_]+`); the noun says which (`variable`, `army`).
    Id(&'static str),
    /// A whole number.
    Int,
    /// A whole number from the first to the second.
    Range(i64, i64),
    Bool,
    /// Text: a text key of the package's texts, or a title.
    Text,
    /// A cell `[x, y]`.
    Cell,
    /// A cell `[x, y]` or an area `[x, y, w, h]`.
    Place,
    /// A whole number, or true or false (a variable's value).
    Value,
    /// A holder: `player`, `pack`, an army id, or `named:<id>`.
    Holder,
    /// A unit: `dt:<alias>` or the package's unit id.
    Unit,
    /// An item: `dt:item/<alias>` or the package's item id.
    Item,
    /// A spell: `dt:spell/<alias>`.
    Spell,
    /// A list of units.
    Units,
    /// A faction by name (`player`, `ally`, `neighbour`, `enemy`) or number (1 to 4).
    Faction,
    /// One of these words.
    Choice(&'static [&'static str]),
    /// An army model by its name (see `triggers::MODELS`).
    Model,
    /// A list of at least two cells.
    Patrol,
    /// `dt`, or `lua:<script>`.
    Rules,
    /// `hero`, or an army id.
    Target,
    /// `"default"`, a unit, or `{ model = "<army model>" }`.
    Figure,
}

impl Ty {
    /// The type in words, for the text output.
    pub fn words(self) -> String {
        match self {
            Ty::Id(noun) => noun.to_string(),
            Ty::Int => "whole number".into(),
            Ty::Range(lo, hi) => format!("whole number {lo} to {hi}"),
            Ty::Bool => "true or false".into(),
            Ty::Text => "text".into(),
            Ty::Cell => "cell [x, y]".into(),
            Ty::Place => "cell [x, y] or area [x, y, w, h]".into(),
            Ty::Value => "whole number, or true or false".into(),
            Ty::Holder => "player, pack, an army id or named:<id>".into(),
            Ty::Unit => "unit: dt:<alias> or a package unit id".into(),
            Ty::Item => "item: dt:item/<alias> or a package item id".into(),
            Ty::Spell => "spell: dt:spell/<alias>".into(),
            Ty::Units => "list of units".into(),
            Ty::Faction => "player, ally, neighbour, enemy, or 1 to 4".into(),
            Ty::Choice(words) => words.join(" or "),
            Ty::Model => format!("army model: {}", MODELS.map(|(n, _)| n).join(", ")),
            Ty::Patrol => "list of at least two cells".into(),
            Ty::Rules => "dt, or lua:<script>".into(),
            Ty::Target => "hero, or an army id".into(),
            Ty::Figure => "default, a unit, or { model = … }".into(),
        }
    }

    /// The type as a JSON Schema (draft 2020-12) for the value.
    pub fn json_schema(self) -> Value {
        const NAME: &str = "^[a-z0-9_]+$";
        match self {
            Ty::Id(_) => json!({ "type": "string", "pattern": NAME }),
            Ty::Int => json!({ "type": "integer" }),
            Ty::Range(lo, hi) => json!({ "type": "integer", "minimum": lo, "maximum": hi }),
            Ty::Bool => json!({ "type": "boolean" }),
            Ty::Text | Ty::Holder | Ty::Unit | Ty::Item | Ty::Spell | Ty::Rules | Ty::Target => json!({ "type": "string" }),
            Ty::Cell => cell_schema(),
            Ty::Place => json!({ "type": "array", "items": { "type": "integer", "minimum": 0 }, "minItems": 2, "maxItems": 4 }),
            Ty::Value => json!({ "oneOf": [{ "type": "integer" }, { "type": "boolean" }] }),
            Ty::Units => json!({ "type": "array", "items": { "type": "string" } }),
            Ty::Faction => json!({ "oneOf": [{ "enum": ["player", "ally", "neighbour", "enemy"] }, { "type": "integer", "minimum": 1, "maximum": 4 }] }),
            Ty::Choice(words) => json!({ "enum": words }),
            Ty::Model => json!({ "enum": MODELS.map(|(n, _)| n) }),
            Ty::Patrol => json!({ "type": "array", "items": cell_schema(), "minItems": 2 }),
            Ty::Figure => json!({
                "oneOf": [
                    { "const": "default" },
                    { "type": "string" },
                    { "type": "object", "properties": { "model": { "enum": MODELS.map(|(n, _)| n) } }, "required": ["model"], "additionalProperties": false }
                ]
            }),
        }
    }
}

fn cell_schema() -> Value {
    json!({ "type": "array", "items": { "type": "integer", "minimum": 0 }, "minItems": 2, "maxItems": 2 })
}

/// A key of a table: its name, its type, whether it must be given, and its default when not.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub name: &'static str,
    pub ty: Ty,
    pub required: bool,
    pub default: Option<&'static str>,
}

/// One event, condition or action.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub section: Section,
    /// The name the parser accepts: the event's name (its `event` key's value), or the kind key.
    pub name: &'static str,
    /// What the kind key holds (a condition's or an action's value); none for an event.
    pub value: Option<Ty>,
    /// The other keys, the table's arguments.
    pub keys: &'static [Key],
    /// Names agents tried for this entry; `check` suggests `name` for them.
    pub aliases: &'static [&'static str],
    /// One sentence: what it does or when it fires.
    pub about: &'static str,
    /// One TOML inline table that the parser reads as this entry.
    pub example: &'static str,
}

impl Entry {
    /// The example as the line a trigger or dialog writes it in: `when = …`, `if = [ … ]`, `then = [ … ]`.
    pub fn example_line(&self) -> String {
        match self.section {
            Section::Event => format!("when = {}", self.example),
            _ => format!("{} = [ {} ]", self.section.toml_key(), self.example),
        }
    }

    /// The heading of the entry in the text output: the kind key and its value.
    fn signature(&self) -> String {
        match (self.section, self.value) {
            (Section::Event, _) => format!("event = \"{}\"", self.name),
            (_, Some(v)) => format!("{} = <{}>", self.name, v.words()),
            (_, None) => self.name.to_string(),
        }
    }
}

const fn req(name: &'static str, ty: Ty) -> Key {
    Key { name, ty, required: true, default: None }
}

const fn opt(name: &'static str, ty: Ty) -> Key {
    Key { name, ty, required: false, default: None }
}

const fn dflt(name: &'static str, ty: Ty, default: &'static str) -> Key {
    Key { name, ty, required: false, default: Some(default) }
}

const CMP: &[&str] = &["==", "!=", "<", "<=", ">", ">="];
const SIDES: &[&str] = &["player", "ally", "neighbour", "enemy"];
const OF: &[&str] = &["army", "building"];

/// Every event, condition and action of a trigger file, in the order of the parser's lists.
pub const VOCAB: &[Entry] = &[
    // Events (`when`).
    Entry {
        section: Section::Event,
        name: "game_start",
        value: None,
        keys: &[],
        aliases: &[],
        about: "The story starts, and the opening dialogs wait for the map's first moment.",
        example: r#"{ event = "game_start" }"#,
    },
    Entry {
        section: Section::Event,
        name: "day_start",
        value: None,
        keys: &[],
        aliases: &[],
        about: "Each midnight.",
        example: r#"{ event = "day_start" }"#,
    },
    Entry {
        section: Section::Event,
        name: "week_start",
        value: None,
        keys: &[],
        aliases: &[],
        about: "Each midnight of a day that is a multiple of 7 from year 0 (the clock has no weeks).",
        example: r#"{ event = "week_start" }"#,
    },
    Entry {
        section: Section::Event,
        name: "new_moon",
        value: None,
        keys: &[],
        aliases: &[],
        about: "Each midnight of a day that is a multiple of 28 from year 0 (no moon is modelled).",
        example: r#"{ event = "new_moon" }"#,
    },
    Entry {
        section: Section::Event,
        name: "hero_enters",
        value: None,
        keys: &[req("at", Ty::Place)],
        aliases: &[],
        about: "The hero steps onto a cell, or into an area.",
        example: r#"{ event = "hero_enters", at = [12, 7] }"#,
    },
    Entry {
        section: Section::Event,
        name: "hero_enters_building",
        value: None,
        keys: &[req("building", Ty::Id("building"))],
        aliases: &[],
        about: "The hero enters a building.",
        example: r#"{ event = "hero_enters_building", building = "castle" }"#,
    },
    Entry {
        section: Section::Event,
        name: "army_defeated",
        value: None,
        keys: &[req("army", Ty::Id("army"))],
        aliases: &[],
        about: "The player beats that army in a battle.",
        example: r#"{ event = "army_defeated", army = "guard" }"#,
    },
    Entry {
        section: Section::Event,
        name: "building_captured",
        value: None,
        keys: &[req("building", Ty::Id("building"))],
        aliases: &[],
        about: "The hero takes that building, by walking in or by beating its garrison.",
        example: r#"{ event = "building_captured", building = "castle" }"#,
    },
    Entry {
        section: Section::Event,
        name: "building_lost",
        value: None,
        keys: &[req("building", Ty::Id("building"))],
        aliases: &[],
        about: "A building the player owned changes owner, by the AI or by a set_faction.",
        example: r#"{ event = "building_lost", building = "mill" }"#,
    },
    Entry {
        section: Section::Event,
        name: "sim_victory",
        value: None,
        keys: &[],
        aliases: &[],
        about: "Developer only: razdor-pack sim --auto-win-after raises it before a map's victory, the game never does.",
        example: r#"{ event = "sim_victory" }"#,
    },
    Entry {
        section: Section::Event,
        name: "dialog_choice",
        value: None,
        keys: &[req("dialog", Ty::Id("dialog")), req("choice", Ty::Id("choice"))],
        aliases: &[],
        about: "The player picks that choice of that dialog.",
        example: r#"{ event = "dialog_choice", dialog = "gate", choice = "pay" }"#,
    },
    Entry {
        section: Section::Event,
        name: "var_changed",
        value: None,
        keys: &[req("var", Ty::Id("variable"))],
        aliases: &[],
        about: "That variable changes its value.",
        example: r#"{ event = "var_changed", var = "dread" }"#,
    },
    Entry {
        section: Section::Event,
        name: "army_arrives",
        value: None,
        keys: &[req("army", Ty::Id("army")), opt("at", Ty::Cell), opt("building", Ty::Id("building"))],
        aliases: &[],
        about: "An army with an army_goal reaches its cell or walks into its building (at or building, or neither for any arrival).",
        example: r#"{ event = "army_arrives", army = "guard", at = [4, 5] }"#,
    },
    Entry {
        section: Section::Event,
        name: "hero_meets_army",
        value: None,
        keys: &[req("army", Ty::Id("army"))],
        aliases: &[],
        about: "The hero steps onto that army or it greets him: its triggers run before any battle, and one that fires means no battle.",
        example: r#"{ event = "hero_meets_army", army = "patrol" }"#,
    },
    // Conditions (`if`).
    Entry {
        section: Section::Condition,
        name: "var",
        value: Some(Ty::Id("variable")),
        keys: &[dflt("op", Ty::Choice(CMP), "=="), req("value", Ty::Value)],
        aliases: &[],
        about: "A variable compares with a value (true or false variables take only == and !=).",
        example: r#"{ var = "dread", op = ">=", value = 3 }"#,
    },
    Entry {
        section: Section::Condition,
        name: "random",
        value: Some(Ty::Range(0, 100)),
        keys: &[],
        aliases: &[],
        about: "True with that percent, drawn from the package's own generator.",
        example: r#"{ random = 30 }"#,
    },
    Entry {
        section: Section::Condition,
        name: "hero_at",
        value: Some(Ty::Place),
        keys: &[],
        aliases: &[],
        about: "The hero stands on that cell, or in that area.",
        example: r#"{ hero_at = [3, 4, 2, 2] }"#,
    },
    Entry {
        section: Section::Condition,
        name: "army_at",
        value: Some(Ty::Id("army")),
        keys: &[req("at", Ty::Cell)],
        aliases: &[],
        about: "The army is on the map at that cell.",
        example: r#"{ army_at = "guard", at = [5, 6] }"#,
    },
    Entry {
        section: Section::Condition,
        name: "has_named_unit",
        value: Some(Ty::Id("named character")),
        keys: &[req("unit", Ty::Unit), dflt("in", Ty::Holder, "player"), opt("garrison", Ty::Id("building")), dflt("value", Ty::Bool, "true")],
        aliases: &[],
        about: "A named character of that unit type is alive in that army or garrison (value = false: he is not there).",
        example: r#"{ has_named_unit = "kord", unit = "dt:swordsman", in = "player" }"#,
    },
    Entry {
        section: Section::Condition,
        name: "has_item",
        value: Some(Ty::Item),
        keys: &[dflt("in", Ty::Holder, "player"), opt("garrison", Ty::Id("building"))],
        aliases: &[],
        about: "The item is in the pack, or worn by that holder's units (a garrison's pack too).",
        example: r#"{ has_item = "dt:item/viking_sword", in = "player" }"#,
    },
    Entry {
        section: Section::Condition,
        name: "gold",
        value: Some(Ty::Int),
        keys: &[dflt("op", Ty::Choice(CMP), ">=")],
        aliases: &[],
        about: "The hero's gold compares with that number.",
        example: r#"{ gold = 100, op = ">=" }"#,
    },
    Entry {
        section: Section::Condition,
        name: "army_alive",
        value: Some(Ty::Id("army")),
        keys: &[dflt("value", Ty::Bool, "true")],
        aliases: &[],
        about: "The army stands on the map and is not beaten (value = false: it is gone).",
        example: r#"{ army_alive = "guard", value = false }"#,
    },
    Entry {
        section: Section::Condition,
        name: "building_owner",
        value: Some(Ty::Id("building")),
        keys: &[req("faction", Ty::Faction)],
        aliases: &[],
        about: "The building belongs to that faction.",
        example: r#"{ building_owner = "castle", faction = "player" }"#,
    },
    Entry {
        section: Section::Condition,
        name: "has_spell",
        value: Some(Ty::Spell),
        keys: &[dflt("in", Ty::Holder, "player"), opt("garrison", Ty::Id("building")), dflt("all", Ty::Bool, "false")],
        aliases: &[],
        about: "A unit holds the spell in a slot that has not run out (all = true: every unit does).",
        example: r#"{ has_spell = "dt:spell/fireball", in = "rebels", all = true }"#,
    },
    // Actions (`then`).
    Entry {
        section: Section::Action,
        name: "set",
        value: Some(Ty::Id("variable")),
        keys: &[req("value", Ty::Value)],
        aliases: &[],
        about: "The variable gets that value, and its change fires var_changed.",
        example: r#"{ set = "held", value = true }"#,
    },
    Entry {
        section: Section::Action,
        name: "add",
        value: Some(Ty::Id("variable")),
        keys: &[req("value", Ty::Int), opt("min", Ty::Int), opt("max", Ty::Int)],
        aliases: &[],
        about: "A number variable grows by value, clamped to min and max when they are given.",
        example: r#"{ add = "crown", value = 1, min = -5, max = 5 }"#,
    },
    Entry {
        section: Section::Action,
        name: "random_var",
        value: Some(Ty::Id("variable")),
        keys: &[req("min", Ty::Int), req("max", Ty::Int)],
        aliases: &[],
        about: "A number variable gets a random value from min to max.",
        example: r#"{ random_var = "dice", min = 1, max = 6 }"#,
    },
    Entry {
        section: Section::Action,
        name: "dialog",
        value: Some(Ty::Id("dialog")),
        keys: &[],
        aliases: &[],
        about: "Opens the dialog, its choices shown at once.",
        example: r#"{ dialog = "gate" }"#,
    },
    Entry {
        section: Section::Action,
        name: "give_gold",
        value: Some(Ty::Int),
        keys: &[],
        aliases: &[],
        about: "The hero's gold changes: a negative number takes gold, never below 0.",
        example: r#"{ give_gold = 50 }"#,
    },
    Entry {
        section: Section::Action,
        name: "give_item",
        value: Some(Ty::Item),
        keys: &[dflt("to", Ty::Holder, "pack"), opt("garrison", Ty::Id("building"))],
        aliases: &[],
        about: "The item goes to a holder (the pack by default), or with garrison to a building's troops, else its pack.",
        example: r#"{ give_item = "dt:item/viking_sword", to = "named:kord" }"#,
    },
    Entry {
        section: Section::Action,
        name: "give_unit",
        value: Some(Ty::Unit),
        keys: &[opt("named", Ty::Id("named character"))],
        aliases: &[],
        about: "A unit joins the hero's army, which drops its weakest unit when full, as in DT.",
        example: r#"{ give_unit = "dt:swordsman", named = "kord" }"#,
    },
    Entry {
        section: Section::Action,
        name: "give_xp",
        value: Some(Ty::Int),
        keys: &[dflt("to", Ty::Holder, "player"), opt("garrison", Ty::Id("building"))],
        aliases: &[],
        about: "Experience goes to a holder's units (the hero's army by default), or with garrison to a building's troops.",
        example: r#"{ give_xp = 50, to = "rebels" }"#,
    },
    Entry {
        section: Section::Action,
        name: "spawn_army",
        value: Some(Ty::Id("army")),
        keys: &[req("at", Ty::Cell), req("faction", Ty::Faction), dflt("model", Ty::Range(0, 12), "5"), req("troops", Ty::Units)],
        aliases: &[],
        about: "A new AI army appears on the map, steered by army_goal; an id already in use spawns nothing.",
        example: r#"{ spawn_army = "rebels", at = [5, 6], faction = "enemy", troops = ["dt:swordsman"] }"#,
    },
    Entry {
        section: Section::Action,
        name: "remove_army",
        value: Some(Ty::Id("army")),
        keys: &[],
        aliases: &[],
        about: "The army leaves the map (an event can bring it back).",
        example: r#"{ remove_army = "rebels" }"#,
    },
    Entry {
        section: Section::Action,
        name: "start_battle",
        value: Some(Ty::Id("army")),
        keys: &[req("rules", Ty::Rules)],
        aliases: &[],
        about: "The army meets the hero now, in that battle: dt, or a field battle run by a script.",
        example: r#"{ start_battle = "guard", rules = "dt" }"#,
    },
    Entry {
        section: Section::Action,
        name: "teleport_hero",
        value: Some(Ty::Cell),
        keys: &[],
        aliases: &[],
        about: "The hero's army moves to that cell.",
        example: r#"{ teleport_hero = [10, 5] }"#,
    },
    Entry {
        section: Section::Action,
        name: "teleport_army",
        value: Some(Ty::Id("army")),
        keys: &[opt("at", Ty::Cell), opt("to", Ty::Id("building"))],
        aliases: &[],
        about: "The army moves to a cell (at) or into a building (to); give one of them.",
        example: r#"{ teleport_army = "rebels", to = "castle" }"#,
    },
    Entry {
        section: Section::Action,
        name: "set_faction",
        value: Some(Ty::Id("army or building")),
        keys: &[dflt("of", Ty::Choice(OF), "army"), req("faction", Ty::Faction)],
        aliases: &[],
        about: "The army (or the building, with of = \"building\") changes faction.",
        example: r#"{ set_faction = "castle", of = "building", faction = "enemy" }"#,
    },
    Entry {
        section: Section::Action,
        name: "set_relation",
        value: Some(Ty::Id("army or building")),
        keys: &[dflt("of", Ty::Choice(OF), "army"), req("towards", Ty::Choice(SIDES)), req("value", Ty::Range(-3, 3))],
        aliases: &[],
        about: "The army (or building) has that relation towards a side, from -3 to 3.",
        example: r#"{ set_relation = "rebels", towards = "player", value = -3 }"#,
    },
    Entry {
        section: Section::Action,
        name: "add_units",
        value: Some(Ty::Id("army, or \"garrison\"")),
        keys: &[req("troops", Ty::Units), dflt("of", Ty::Choice(OF), "army"), opt("garrison", Ty::Id("building"))],
        aliases: &[],
        about: "Units join an army, or a building's troops with add_units = \"garrison\" and garrison = \"<building>\".",
        example: r#"{ add_units = "rebels", troops = ["dt:archer"] }"#,
    },
    Entry {
        section: Section::Action,
        name: "cast_on_army",
        value: Some(Ty::Spell),
        keys: &[dflt("in", Ty::Holder, "player"), opt("garrison", Ty::Id("building")), opt("minutes", Ty::Range(1, 100_000)), dflt("permanent", Ty::Bool, "false")],
        aliases: &[],
        about: "The spell goes into each unit's slot for minutes, or for good with permanent = true.",
        example: r#"{ cast_on_army = "dt:spell/fireball", in = "rebels", minutes = 120 }"#,
    },
    Entry {
        section: Section::Action,
        name: "remove_spells",
        value: Some(Ty::Holder),
        keys: &[opt("garrison", Ty::Id("building"))],
        aliases: &[],
        about: "Every spell of the holder's units goes; with garrison and the value \"garrison\", those of a building's troops.",
        example: r#"{ remove_spells = "rebels" }"#,
    },
    Entry {
        section: Section::Action,
        name: "remove_spell_from_book",
        value: Some(Ty::Spell),
        keys: &[],
        aliases: &[],
        about: "The hero's spell book drops that spell.",
        example: r#"{ remove_spell_from_book = "dt:spell/fireball" }"#,
    },
    Entry {
        section: Section::Action,
        name: "army_goal",
        value: Some(Ty::Id("army")),
        keys: &[
            opt("go_to", Ty::Cell),
            opt("go_to_building", Ty::Id("building")),
            opt("follow", Ty::Target),
            opt("patrol", Ty::Patrol),
            opt("attack", Ty::Target),
            opt("clear", Ty::Bool),
        ],
        aliases: &[],
        about: "The army walks to one goal (give exactly one key) instead of the AI's own choice; clear = true gives it back to the AI.",
        example: r#"{ army_goal = "ghost", patrol = [[4, 5], [9, 5]] }"#,
    },
    Entry {
        section: Section::Action,
        name: "army_speed",
        value: Some(Ty::Id("army")),
        keys: &[req("percent", Ty::Range(10, 1000))],
        aliases: &[],
        about: "The army's steps take 100 divided by percent of their time (100 is the normal pace).",
        example: r#"{ army_speed = "ghost", percent = 150 }"#,
    },
    Entry {
        section: Section::Action,
        name: "hero_figure",
        value: Some(Ty::Figure),
        keys: &[],
        aliases: &[],
        about: "The hero is drawn with a unit's map figure, or an army model's, and that stays saved.",
        example: r#"{ hero_figure = { model = "bandits" } }"#,
    },
    Entry {
        section: Section::Action,
        name: "army_figure",
        value: Some(Ty::Id("army")),
        keys: &[opt("unit", Ty::Unit), opt("model", Ty::Model)],
        aliases: &[],
        about: "The army is drawn with a unit's map figure or an army model's (give one of unit and model).",
        example: r#"{ army_figure = "rebels", model = "bandits" }"#,
    },
    Entry {
        section: Section::Action,
        name: "remove_unit",
        value: Some(Ty::Id("named character")),
        keys: &[dflt("in", Ty::Holder, "player"), opt("garrison", Ty::Id("building"))],
        aliases: &[],
        about: "The named character leaves that holder, and his worn items go to the pack.",
        example: r#"{ remove_unit = "kord", in = "guard" }"#,
    },
    Entry {
        section: Section::Action,
        name: "victory",
        value: Some(Ty::Bool),
        keys: &[opt("next", Ty::Id("map"))],
        aliases: &[],
        about: "The scenario ends in victory; with next, the campaign goes to that map.",
        example: r#"{ victory = true, next = "marka" }"#,
    },
    Entry {
        section: Section::Action,
        name: "defeat",
        value: Some(Ty::Bool),
        keys: &[],
        aliases: &[],
        about: "The scenario ends in defeat.",
        example: r#"{ defeat = true }"#,
    },
    Entry {
        section: Section::Action,
        name: "travel",
        value: Some(Ty::Id("map")),
        keys: &[opt("at", Ty::Text)],
        aliases: &[],
        about: "The campaign leaves this map for that one, at the point named by at (else its start), without a victory.",
        example: r#"{ travel = "marka", at = "gate" }"#,
    },
    Entry {
        section: Section::Action,
        name: "ending",
        value: Some(Ty::Id("dialog")),
        keys: &[],
        aliases: &[],
        about: "The dialog shows, then the campaign ends with it.",
        example: r#"{ ending = "end_free" }"#,
    },
    Entry {
        section: Section::Action,
        name: "quest",
        value: Some(Ty::Id("quest")),
        keys: &[req("title", Ty::Text), req("text", Ty::Text)],
        aliases: &[],
        about: "An active diary entry is added, or its texts are updated when the id is already active.",
        example: r#"{ quest = "ford", title = "ford.quest.title", text = "ford.quest.text" }"#,
    },
    Entry {
        section: Section::Action,
        name: "complete_quest",
        value: Some(Ty::Id("quest")),
        keys: &[],
        aliases: &[],
        about: "That diary entry moves to the completed ones, dated now (nothing when it is not active).",
        example: r#"{ complete_quest = "ford" }"#,
    },
    Entry {
        section: Section::Action,
        name: "restore_army",
        value: Some(Ty::Choice(&["player"])),
        keys: &[],
        aliases: &["heal_army", "heal_squad", "restore_squad"],
        about: "The hero's squad is healed to full and its fallen stand up again, for nothing.",
        example: r#"{ restore_army = "player" }"#,
    },
];

/// The entries of one section, in the table's order.
pub fn of_section(section: Section) -> impl Iterator<Item = &'static Entry> {
    VOCAB.iter().filter(move |e| e.section == section)
}

/// The name of the entry nearest to `word` among `names` and their aliases (edit distance at
/// most 3; an alias answers with its entry's name). `None` when nothing is near.
pub fn nearest(word: &str, names: &[&'static str]) -> Option<&'static str> {
    let mut best: Option<(usize, &'static str)> = None;
    let mut consider = |candidate: &str, answer: &'static str| {
        let d = super::triggers::edit_distance(word, candidate);
        if d <= 3 && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, answer));
        }
    };
    for &n in names {
        consider(n, n);
    }
    for e in VOCAB.iter().filter(|e| names.contains(&e.name)) {
        for a in e.aliases {
            consider(a, e.name);
        }
    }
    best.map(|(_, n)| n)
}

/// The entries called `name` (an event, condition or action may share a name only by mistake).
fn named(name: &str) -> Vec<&'static Entry> {
    VOCAB.iter().filter(|e| e.name == name).collect()
}

/// The text of `razdor-pack schema`: each section's entries, one heading per section, two lines per
/// entry (its keys, its sentence, its example). A filter keeps the entries of that name.
pub fn text(filter: Option<&str>) -> Result<String, String> {
    let entries = select(filter)?;
    let mut out = String::new();
    for section in [Section::Event, Section::Condition, Section::Action] {
        let shown: Vec<&Entry> = entries.iter().copied().filter(|e| e.section == section).collect();
        if shown.is_empty() {
            continue;
        }
        out.push_str(&format!("{}\n", section.heading()));
        for e in shown {
            out.push_str(&format!("  {}", e.signature()));
            let keys: Vec<String> = e.keys.iter().map(key_words).collect();
            if !keys.is_empty() {
                out.push_str(&format!("   keys: {}", keys.join("; ")));
            }
            if !e.aliases.is_empty() {
                out.push_str(&format!("   (also: {})", e.aliases.join(", ")));
            }
            out.push('\n');
            out.push_str(&format!("      {}\n", e.about));
            out.push_str(&format!("      e.g. {}\n", e.example_line()));
        }
        out.push('\n');
    }
    Ok(out)
}

fn key_words(k: &Key) -> String {
    let mut s = format!("{} ({}", k.name, k.ty.words());
    if k.required {
        s.push_str(", required");
    }
    if let Some(d) = k.default {
        s.push_str(&format!(", default {d}"));
    }
    s.push(')');
    s
}

/// The entries a filter names (all of them without a filter), or the error with the nearest name.
fn select(filter: Option<&str>) -> Result<Vec<&'static Entry>, String> {
    let Some(name) = filter else { return Ok(VOCAB.iter().collect()) };
    let found = named(name);
    if found.is_empty() {
        let names: Vec<&'static str> = VOCAB.iter().map(|e| e.name).collect();
        let hint = match nearest(name, &names) {
            Some(near) => format!("did you mean `{near}`?"),
            None => "`razdor-pack schema` lists them all".to_string(),
        };
        return Err(format!("no event, condition or action `{name}` (hint: {hint})"));
    }
    Ok(found)
}

/// The entries as data: `section`, `name`, `value` (words or null), `keys` (name, type, required,
/// default), `aliases`, `description`, `example` (the TOML line). A filter keeps the entries of
/// that name.
pub fn json(filter: Option<&str>) -> Result<Value, String> {
    let entries = select(filter)?;
    let items: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "section": match e.section { Section::Event => "event", Section::Condition => "condition", Section::Action => "action" },
                "name": e.name,
                "value": e.value.map(Ty::words),
                "keys": e.keys.iter().map(|k| json!({ "name": k.name, "type": k.ty.words(), "required": k.required, "default": k.default })).collect::<Vec<_>>(),
                "aliases": e.aliases,
                "description": e.about,
                "example": e.example_line(),
            })
        })
        .collect();
    Ok(Value::Array(items))
}

/// The files a JSON Schema is written for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaFile {
    /// `triggers/*.toml`: `[vars]` and `[[trigger]]`.
    Triggers,
    /// `dialogs/*.toml`: `[[dialog]]` with `[[dialog.choice]]`.
    Dialogs,
}

impl SchemaFile {
    /// The file a schema name (`triggers`, `dialogs`) means.
    pub fn named(word: &str) -> Option<SchemaFile> {
        match word {
            "triggers" => Some(SchemaFile::Triggers),
            "dialogs" => Some(SchemaFile::Dialogs),
            _ => None,
        }
    }
}

/// One entry as a JSON Schema object: its kind key (`event` with its name, or the entry's name with
/// its value's type), its keys, and no others.
fn entry_schema(e: &Entry) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    match (e.section, e.value) {
        (Section::Event, _) => {
            props.insert("event".into(), json!({ "const": e.name }));
            required.push(json!("event"));
        }
        (_, Some(v)) => {
            props.insert(e.name.into(), v.json_schema());
            required.push(json!(e.name));
        }
        (_, None) => {}
    }
    for k in e.keys {
        props.insert(k.name.into(), k.ty.json_schema());
        if k.required {
            required.push(json!(k.name));
        }
    }
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

/// The `$defs` shared by both files: one `oneOf` per section.
fn defs() -> Map<String, Value> {
    let mut defs = Map::new();
    for (key, section) in [("event", Section::Event), ("condition", Section::Condition), ("action", Section::Action)] {
        let branches: Vec<Value> = of_section(section).map(entry_schema).collect();
        defs.insert(key.into(), json!({ "oneOf": branches }));
    }
    defs
}

/// The JSON Schema (draft 2020-12) of a trigger or dialog file, from the vocabulary.
pub fn json_schema(file: SchemaFile) -> Value {
    let mut defs = defs();
    let name_pattern = json!({ "type": "string", "pattern": "^[a-z0-9_]+$" });
    let mut schema = match file {
        SchemaFile::Triggers => {
            defs.insert(
                "trigger".into(),
                json!({
                    "type": "object",
                    "properties": {
                        "id": name_pattern,
                        "map": { "type": "string", "pattern": "^[a-z0-9-]+$", "description": "The map's id: the name of its folder maps/<id>/." },
                        "when": { "$ref": "#/$defs/event" },
                        "if": { "type": "array", "items": { "$ref": "#/$defs/condition" } },
                        "then": { "type": "array", "items": { "$ref": "#/$defs/action" } },
                        "once": { "type": "boolean", "default": true },
                        "chance": { "type": "integer", "minimum": 0, "maximum": 100 }
                    },
                    "required": ["id", "when"],
                    "additionalProperties": false
                }),
            );
            json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "title": "Razdor package: triggers/*.toml",
                "description": "The story's variables ([vars]) and triggers ([[trigger]]). Generated by `razdor-pack schema --json-schema`.",
                "type": "object",
                "properties": {
                    "vars": { "type": "object", "propertyNames": { "pattern": "^[a-z0-9_]+$" }, "additionalProperties": { "oneOf": [{ "type": "integer" }, { "type": "boolean" }] } },
                    "trigger": { "type": "array", "items": { "$ref": "#/$defs/trigger" } }
                },
                "additionalProperties": false
            })
        }
        SchemaFile::Dialogs => {
            defs.insert(
                "choice".into(),
                json!({
                    "type": "object",
                    "properties": {
                        "id": name_pattern,
                        "text": { "type": "string", "description": "A text key." },
                        "if": { "type": "array", "items": { "$ref": "#/$defs/condition" } },
                        "then": { "type": "array", "items": { "$ref": "#/$defs/action" } }
                    },
                    "required": ["id", "text"],
                    "additionalProperties": false
                }),
            );
            defs.insert(
                "dialog".into(),
                json!({
                    "type": "object",
                    "properties": {
                        "id": name_pattern,
                        "title": { "type": "string", "description": "A text key." },
                        "text": { "type": "string", "description": "A text key." },
                        "speaker": { "type": "string", "description": "A unit: dt:<alias>, whose portrait shows." },
                        "choice": { "type": "array", "maxItems": 5, "items": { "$ref": "#/$defs/choice" } }
                    },
                    "required": ["id", "title", "text"],
                    "additionalProperties": false
                }),
            );
            json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "title": "Razdor package: dialogs/*.toml",
                "description": "The story's dialogs ([[dialog]]) with their choices (at most five). Generated by `razdor-pack schema --json-schema dialogs`.",
                "type": "object",
                "properties": { "dialog": { "type": "array", "items": { "$ref": "#/$defs/dialog" } } },
                "additionalProperties": false
            })
        }
    };
    schema["$defs"] = Value::Object(defs);
    schema
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use toml::Table;

    use super::super::triggers::{self, ACTIONS, ARGUMENTS, CONDITIONS, EVENTS};
    use super::*;

    /// The example of an entry, read as the parser reads a table of its section.
    fn example(e: &Entry) -> Table {
        let text = format!("x = {}", e.example);
        let file: Table = toml::from_str(&text).unwrap_or_else(|err| panic!("{}: {err}", e.name));
        file["x"].as_table().cloned().unwrap_or_else(|| panic!("{}: the example is a table", e.name))
    }

    fn names_of(section: Section) -> BTreeSet<&'static str> {
        of_section(section).map(|e| e.name).collect()
    }

    #[test]
    fn the_table_names_what_the_parser_accepts_and_no_more() {
        for (section, list) in [(Section::Event, &EVENTS[..]), (Section::Condition, &CONDITIONS[..]), (Section::Action, &ACTIONS[..])] {
            let parsed: BTreeSet<&str> = list.iter().copied().collect();
            assert_eq!(parsed.len(), list.len(), "{section:?}: a name is listed twice");
            assert_eq!(names_of(section), parsed, "{section:?}: the vocabulary and the parser's list differ");
            assert_eq!(of_section(section).count(), list.len(), "{section:?}: an entry is described twice");
        }
    }

    #[test]
    fn every_example_is_read_by_the_parser_as_its_entry() {
        for e in VOCAB {
            let t = example(e);
            match e.section {
                Section::Event => {
                    let when = triggers::parse_event(&t).unwrap_or_else(|p| panic!("{}: {}", e.name, p.message));
                    assert_eq!(t.get("event").and_then(|v| v.as_str()), Some(e.name), "{}", e.name);
                    assert!(!format!("{when:?}").is_empty());
                }
                Section::Condition => {
                    triggers::parse_cond(&t).unwrap_or_else(|p| panic!("{}: {} ({})", e.name, p.message, p.hint));
                    assert!(t.contains_key(e.name), "{}: the example has its kind key", e.name);
                }
                Section::Action => {
                    triggers::parse_action(&t).unwrap_or_else(|p| panic!("{}: {} ({})", e.name, p.message, p.hint));
                    assert!(t.contains_key(e.name), "{}: the example has its kind key", e.name);
                }
            }
        }
    }

    #[test]
    fn an_example_has_only_declared_keys_and_all_required_ones() {
        for e in VOCAB {
            let t = example(e);
            let kind = if e.section == Section::Event { "event" } else { e.name };
            let declared: BTreeSet<&str> = e.keys.iter().map(|k| k.name).chain([kind]).collect();
            for key in t.keys() {
                assert!(declared.contains(key.as_str()), "{}: the example's `{key}` is not declared", e.name);
            }
            for k in e.keys.iter().filter(|k| k.required) {
                assert!(t.contains_key(k.name), "{}: the example lacks the required `{}`", e.name, k.name);
            }
        }
    }

    #[test]
    fn every_declared_key_is_an_argument_or_a_kind_the_reader_knows() {
        let kinds: Vec<&str> = EVENTS.iter().chain(CONDITIONS.iter()).chain(ACTIONS.iter()).copied().collect();
        for e in VOCAB {
            for k in e.keys {
                assert!(ARGUMENTS.contains(&k.name) || kinds.contains(&k.name), "{}: `{}` is neither an argument nor a kind of the reader", e.name, k.name);
            }
        }
    }

    #[test]
    fn each_entry_is_described_in_one_line_with_unique_keys() {
        for e in VOCAB {
            assert!(!e.about.is_empty() && e.about.ends_with('.') && !e.about.contains('\n'), "{}: one sentence", e.name);
            let mut keys = BTreeSet::new();
            for k in e.keys {
                assert!(keys.insert(k.name), "{}: key `{}` twice", e.name, k.name);
                assert!(!k.ty.words().is_empty());
            }
        }
    }

    #[test]
    fn an_alias_is_never_a_name_the_parser_accepts() {
        let accepted: BTreeSet<&str> = EVENTS.iter().chain(CONDITIONS.iter()).chain(ACTIONS.iter()).copied().collect();
        for e in VOCAB {
            for a in e.aliases {
                assert!(!accepted.contains(a), "alias `{a}` of `{}` is accepted by the parser", e.name);
            }
        }
    }

    #[test]
    fn heal_army_is_the_restore_army_it_was_invented_for() {
        assert_eq!(nearest("heal_army", &ACTIONS), Some("restore_army"));
        assert_eq!(nearest("hero_att", &CONDITIONS), Some("hero_at"));
        assert_eq!(nearest("xyzzy", &ACTIONS), None);
        let r = triggers::parse_action(&example_table("heal_army = \"player\"")).unwrap_err();
        assert_eq!(r.hint, "did you mean `restore_army`?");
    }

    fn example_table(text: &str) -> Table {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn the_text_and_json_list_every_entry_and_a_filter_finds_one() {
        let all = text(None).unwrap();
        for e in VOCAB {
            assert!(all.contains(e.name), "{} is in the text", e.name);
        }
        assert_eq!(json(None).unwrap().as_array().unwrap().len(), VOCAB.len());
        let one = json(Some("restore_army")).unwrap();
        assert_eq!(one[0]["section"], "action");
        assert_eq!(one[0]["aliases"][0], "heal_army");
        assert!(text(Some("restore_army")).unwrap().contains("restore_army = <player>"));
        let err = json(Some("heal_army")).unwrap_err();
        assert!(err.contains("did you mean `restore_army`?"), "{err}");
    }

    #[test]
    fn the_json_schema_has_one_branch_per_entry() {
        let trig = json_schema(SchemaFile::Triggers);
        assert_eq!(trig["$defs"]["action"]["oneOf"].as_array().unwrap().len(), ACTIONS.len());
        assert_eq!(trig["$defs"]["condition"]["oneOf"].as_array().unwrap().len(), CONDITIONS.len());
        assert_eq!(trig["$defs"]["event"]["oneOf"].as_array().unwrap().len(), EVENTS.len());
        let dialogs = json_schema(SchemaFile::Dialogs);
        assert_eq!(dialogs["$defs"]["choice"]["properties"]["then"]["items"]["$ref"], "#/$defs/action");
        assert_eq!(dialogs["$defs"]["dialog"]["properties"]["choice"]["maxItems"], 5);
        assert_eq!(SchemaFile::named("triggers"), Some(SchemaFile::Triggers));
        assert_eq!(SchemaFile::named("maps"), None);
    }
}
