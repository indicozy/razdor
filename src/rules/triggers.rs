//! Package triggers and dialogs in the running game. `pack::triggers` reads the TOML; this file
//! compiles a read package into a [`Story`] with the engine's ids ([`compile`]) and runs it.
//!
//! A game gets its story with [`Game::attach_story`]. The story is static (`Game::story`, not
//! saved); its state is [`TriggerState`], saved in the game only while a story is in play: no
//! story means no draw, no state and no save field, so a map of 1.x plays as before.
//!
//! The moments are the game's own, and a package's triggers run after the scenario's events at
//! the same moment: game start ([`Game::start_story`]); each midnight, as day start, as week start
//! (every 7 days from year 0: the clock has no week) and as new moon (every 28 days: no moon is
//! modelled, so it is a 28-day cycle, the spec's allowance); a hero's step onto a cell; entering a
//! building; a building taken; an army beaten in battle; a dialog's answer
//! ([`Game::answer_story_dialog`]). A variable's change is a moment too (`var_changed`), run
//! inside the same call, with a loop guard.
//!
//! Package armies and named characters take the ids `PACKAGE_BASE + k` (200 up), so a save
//! keeps them. The package generator is [`TriggerState::rng`], seeded from the story; it never
//! draws from `rules::rng`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::ai::{self, goals::{Arrival, Goal, GoalTarget}};
use super::content::{ItemId, UnitId};
use super::events::{ArmyId, EventWorld, Holder as DtHolder};
use super::formation::{Row, Slot};
use super::game::{arrange_troops, Event, Game};
use super::items;
use super::units::{SpellSlot, SPELL_SLOTS};
use super::world::{Army, Troop, MAP_ARMY_SPELL_END};
use super::map::Tile;

use crate::pack::aliases;
use crate::pack::load::Package;
use crate::pack::refs::{DtKind, Ref};
use crate::pack::triggers::{self as pt, Cmp, Place, VarKind, VarValue, Until};

/// The first id of a package's army or named character: the k-th one is `PACKAGE_BASE + k`.
/// Scenario armies and named characters of the base game stay below it.
pub const PACKAGE_BASE: u8 = 200;
/// Package armies and named characters one story can hold: ids up to 255.
const PACKAGE_SLOTS: usize = 56;
/// Trigger runs one raised moment may cause (a loop guard: triggers that change variables that
/// other triggers wait for stop here, and the rest waits for the next moment).
const MAX_RUNS: usize = 200;

/// A dialog a trigger opened: its id, and the ids of the choices visible when it opened (none:
/// the window has one «Далее» button).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDialog {
    pub id: String,
    pub choices: Vec<String>,
}

/// How a package's story ended: its `victory` or `defeat` action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ending {
    Victory,
    Defeat,
}

/// One thing a run of the story did, for the trace (`razdor-pack sim --trace`). Kept only while a
/// trace is asked for ([`TriggerState::trace`]); armies and buildings are named as the story names
/// them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Trace {
    /// A trigger ran, at this moment (its `when`, as text).
    Fired { trigger: String, moment: String },
    /// A variable got a new value.
    Var { name: String, value: VarValue },
    /// A dialog opened, with the choices it shows (none: «Далее»).
    Dialog { id: String, choices: Vec<String> },
    /// The player answered a dialog: the choice, or `None` for «Далее».
    Answer { dialog: String, choice: Option<String> },
    /// A package army came onto the map.
    Spawn { army: String },
    /// The story ended.
    End(Ending),
    /// An army got a goal (`army_goal`), as the story writes it; `clear` reads «clear».
    Goal { army: String, goal: String },
    /// A goal's army arrived at a cell, or in a building (`go_to_building`).
    Arrive { army: String, cell: (i32, i32), building: Option<String> },
    /// The story asked for a travel to another campaign map.
    Travel(String),
    /// The story reached its ending (the dialog).
    Ending(String),
}

/// A moment a trigger can wait for, as the trigger says it (`when`), or as the game raises it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum When {
    GameStart,
    DayStart,
    WeekStart,
    NewMoon,
    /// As a trigger: a cell or an area. Raised with a cell (the cell the hero stepped onto).
    HeroEnters(Place),
    /// A building by its scenario id (`Location::id`).
    HeroEntersBuilding(u16),
    ArmyDefeated(ArmyId),
    BuildingCaptured(u16),
    /// A building the player owned changed owner (`building_lost`).
    BuildingLost(u16),
    /// The sim's developer moment before an auto-win (`sim_victory`): never raised in the game.
    SimVictory,
    DialogChoice { dialog: String, choice: String },
    VarChanged(String),
    /// As a trigger: an army's arrival at a cell and/or in a building, each if given (`None`:
    /// any). Raised with the cell of an arrival at its goal, and its building if it has one.
    ArmyArrives { army: ArmyId, at: Option<(i32, i32)>, building: Option<u16> },
}

/// Whose units, items or spells an action or a condition looks at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    /// The hero's army (and the pack, for items).
    Player,
    /// The hero's pack (items only; for units and spells it means the hero's army).
    Pack,
    /// A scenario or package army, by id.
    Army(ArmyId),
    /// A named character of the hero's army (the package's own id, `PACKAGE_BASE + k`).
    Named(u8),
    /// A building's garrison, by its scenario id (its troops, and its pack for items).
    Garrison(u16),
}

/// A holder of an army or a building: the faction or the relation is set on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entity {
    Army(ArmyId),
    /// A building by its scenario id.
    Building(u16),
}

/// Where an army is teleported to: a cell, or a building's cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    Cell(i32, i32),
    Building(u16),
}

/// A map figure, resolved: a unit's picture, or an army model's (its code, 1 to 12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FigureOf {
    Unit(UnitId),
    Model(u8),
}

impl FigureOf {
    /// The index of the map figure ([`Army::figure`]) this picture draws.
    pub fn index(self, content: &crate::rules::content::Content) -> u8 {
        match self {
            FigureOf::Unit(u) => Army::figure_of_unit(content, u),
            FigureOf::Model(code) => Army::figure_of_model(code),
        }
    }
}

/// A condition (`if`), resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cond {
    Var { name: String, cmp: Cmp, value: VarValue },
    /// True with the given chance in percent, rolled with the package generator.
    Random(u8),
    HeroAt(Place),
    ArmyAt(ArmyId, Place),
    /// `present`: false for the absence of the named character (`value = false`).
    HasNamed { named: u8, unit: UnitId, who: Who, present: bool },
    HasItem(ItemId, Who),
    Gold(i64, Cmp),
    /// `alive`: false for an army that is gone (`value = false`).
    ArmyAlive(ArmyId, bool),
    BuildingOwner(u16, u8),
    HasSpell { spell: u32, who: Who, all: bool },
}

/// An action (`then`), resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Set(String, VarValue),
    /// `value` added to a number variable, the result clamped to `min` and `max` when given.
    Add { name: String, value: i64, min: Option<i64>, max: Option<i64> },
    RandomVar(String, i64, i64),
    Dialog(String),
    GiveGold(i64),
    GiveItem(ItemId, Who),
    /// A unit of the hero's army (`named` 0: an ordinary one; else a named character's id).
    GiveUnit(UnitId, u8),
    GiveXp(i64, Who),
    Spawn { army: ArmyId, at: Tile, faction: u8, model: u8, troops: Vec<UnitId> },
    RemoveArmy(ArmyId),
    /// The army meets the hero now; `rules` is `dt` or `lua:<script>` (a field battle).
    StartBattle(ArmyId, String),
    TeleportHero(Tile),
    TeleportArmy(ArmyId, Dest),
    SetFaction(Entity, u8),
    SetRelation(Entity, u8, i8),
    AddUnits(Entity, Vec<UnitId>),
    Cast { spell: u32, who: Who, until: Until },
    RemoveSpells(Who),
    ForgetSpell(u32),
    End(Ending),
    /// An army's goal (`None`: clear it).
    ArmyGoal { army: ArmyId, goal: Option<Goal> },
    ArmySpeed { army: ArmyId, percent: u32 },
    /// The hero's map figure (`None`: his own).
    HeroFigure(Option<FigureOf>),
    ArmyFigure { army: ArmyId, figure: FigureOf },
    /// A named character leaves the holder (the hero's army, an army or a garrison).
    RemoveUnit { named: u8, who: Who },
    /// A victory whose campaign goes to map `String` (the campaign graph's override).
    VictoryTo(String),
    /// The campaign leaves this map for `map` (entering at point `at`, else the start).
    Travel { map: String, at: Option<String> },
    /// The dialog shows, then the campaign ends (the dialog's id).
    EndCampaign(String),
    /// A diary entry of the package's quest `id` (its texts are keys), added or updated.
    Quest { id: String, title: String, text: String },
    /// The diary entry of the package's quest `id` is completed.
    CompleteQuest(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trigger {
    pub id: String,
    pub when: When,
    pub cond: Vec<Cond>,
    pub then: Vec<Act>,
    pub once: bool,
    pub chance: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    /// A text key.
    pub text: String,
    pub cond: Vec<Cond>,
    pub then: Vec<Act>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogDef {
    pub id: String,
    /// Text keys.
    pub title: String,
    pub text: String,
    /// The unit whose portrait the window shows.
    pub speaker: Option<UnitId>,
    pub choices: Vec<Choice>,
}

/// A package's static story, compiled with the engine's ids.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Story {
    /// The declared variables: kind and default.
    pub vars: BTreeMap<String, (VarKind, VarValue)>,
    pub triggers: Vec<Trigger>,
    pub dialogs: Vec<DialogDef>,
    /// Texts by key, in the language the game shows.
    pub texts: BTreeMap<String, String>,
    /// The display names of the package's named characters: the k-th is `PACKAGE_BASE + k`.
    pub named: Vec<String>,
    /// The battle of an army the hero meets, by id: a script's (`lua:<script>`, a field battle)
    /// or the DT's (`dt`). From the map's `[[army]] battle`; an army not listed fights the DT battle.
    pub battles: BTreeMap<ArmyId, String>,
    /// The package's scripts (`scripts/<name>.lua`, by path): the field battles' hooks are opened
    /// from them when the story is attached (`Game::attach_story`). None without scripts.
    pub scripts: Option<Arc<BTreeMap<String, String>>>,
    /// The names (as the story writes them) of the armies and buildings it refers to, by id: for
    /// the trace.
    pub army_names: BTreeMap<ArmyId, String>,
    pub building_names: BTreeMap<u16, String>,
    /// The seed of the package generator (not zero).
    pub seed: u32,
    /// The ids of the triggers that name a map (`map = "…"`): their fired state belongs to that
    /// map, and a campaign keeps it with the map's state (`rules::campaign`).
    pub scoped: BTreeSet<String>,
}

impl Story {
    /// No triggers and no dialogs: attaching it changes nothing.
    pub fn is_empty(&self) -> bool {
        self.triggers.is_empty() && self.dialogs.is_empty() && self.battles.is_empty()
    }
}

/// A campaign request a story made: the map to travel to, and the point of it to come in at.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Travel {
    pub map: String,
    pub at: Option<String>,
}

/// The story's state in the game: saved only while a story is attached ([`TriggerState::is_empty`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TriggerState {
    /// The variables' current values.
    pub vars: BTreeMap<String, VarValue>,
    /// The ids of the `once` triggers that ran.
    pub fired: BTreeSet<String>,
    /// The package generator's state (see [`Game::draw`]).
    pub rng: u32,
    /// Dialogs queued and not yet handed to the interface. Each call hands its dialogs on, so
    /// this is empty between calls.
    pub dialogs: Vec<PendingDialog>,
    /// The package armies on the map, by id.
    pub spawned: BTreeSet<ArmyId>,
    /// The story's end, once a `victory` or `defeat` action ran.
    pub ended: Option<Ending>,
    /// The hero's map sheet, as an index into the map figures (`hero_figure`); none: his own.
    #[serde(default)]
    pub hero_sheet: Option<u8>,
    /// Goal arrivals queued until the AI's tick is over (see [`goals`]); never saved.
    #[serde(skip)]
    pub arrivals: Vec<Arrival>,
    /// Buildings the player lost to the AI, queued until the AI's tick is over (`building_lost`);
    /// never saved.
    #[serde(skip)]
    pub lost: Vec<u16>,
    /// The map a `victory = true, next = "…"` names (the campaign's override), once a victory ran.
    #[serde(default)]
    pub next: Option<String>,
    /// A `travel` the story asked for, taken by the campaign when the moment has played (`rules::campaign`).
    #[serde(default)]
    pub travel: Option<Travel>,
    /// An `ending` the story reached: the dialog that ends the campaign.
    #[serde(default)]
    pub ending: Option<String>,
    /// What the run did, with the game minute of each thing, while a trace is asked for
    /// (`razdor-pack sim --trace`). Never saved.
    #[serde(skip)]
    pub trace: Option<Vec<(u64, Trace)>>,
}

impl TriggerState {
    /// Nothing recorded: the game's save omits the field.
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
            && self.fired.is_empty()
            && self.rng == 0
            && self.dialogs.is_empty()
            && self.spawned.is_empty()
            && self.ended.is_none()
            && self.hero_sheet.is_none()
            && self.next.is_none()
            && self.travel.is_none()
            && self.ending.is_none()
    }
}

/// Where a package's ids of the base game and of the scenario are. `dt:` aliases come from the
/// alias table; these are what the package's own units and items resolve to (the compile step
/// of the package's units), and the scenario's armies and buildings.
pub trait Resolve {
    /// A unit of this package (`units/*.toml`).
    fn unit(&self, local: &str) -> Option<UnitId>;
    /// An item of this package (`items/*.toml`).
    fn item(&self, local: &str) -> Option<ItemId>;
    /// A scenario army by its name.
    fn army(&self, name: &str) -> Option<ArmyId>;
    /// A scenario building by its name (its scenario id).
    fn building(&self, name: &str) -> Option<u16>;
}

/// A resolver for a package that names no units or items of its own and no scenario.
pub struct NoScenario;

impl Resolve for NoScenario {
    fn unit(&self, _: &str) -> Option<UnitId> {
        None
    }
    fn item(&self, _: &str) -> Option<ItemId> {
        None
    }
    fn army(&self, _: &str) -> Option<ArmyId> {
        None
    }
    fn building(&self, _: &str) -> Option<u16> {
        None
    }
}

/// Compiles a read package's story for map `map` (its id) in language `lang` (its texts). A
/// trigger with another map's `map` is left out; the dialogs the map's triggers open (and those
/// their choices open) are compiled, the rest are not. Errors name the file and the line of each
/// reference that does not resolve on this map.
pub fn compile(pkg: &Package, lang: &str, map: &str, resolve: &dyn Resolve, seed: u32) -> Result<Story, Vec<String>> {
    let mut b = Builder {
        pkg,
        resolve,
        spawns: BTreeMap::new(),
        named: BTreeMap::new(),
        named_order: Vec::new(),
        army_names: BTreeMap::new(),
        building_names: BTreeMap::new(),
        at: String::new(),
        errors: Vec::new(),
    };
    b.assign_spawns();
    // Named characters keep their ids over the package's maps (a campaign carries them): they are
    // numbered by first mention in every trigger and dialog, whichever map each is for.
    let mut scratch = Builder {
        pkg,
        resolve,
        spawns: b.spawns.clone(),
        named: BTreeMap::new(),
        named_order: Vec::new(),
        army_names: BTreeMap::new(),
        building_names: BTreeMap::new(),
        at: String::new(),
        errors: Vec::new(),
    };
    for t in &pkg.triggers {
        let _ = scratch.trigger(&t.value);
    }
    for d in &pkg.dialogs {
        let _ = scratch.dialog(&d.value);
    }
    b.named = scratch.named;
    b.named_order = scratch.named_order;
    let mut story = Story { seed: seed.max(1), ..Story::default() };
    for v in &pkg.vars {
        story.vars.insert(v.value.name.clone(), (v.value.kind, default_of(&v.value)));
    }
    story.texts = pkg.texts.get(lang).or_else(|| pkg.texts.values().next()).cloned().unwrap_or_default();
    for t in pkg.triggers.iter().filter(|t| applies(&t.value, map)) {
        b.at = format!("{}:{}", t.file, t.line);
        if let Some(tr) = b.trigger(&t.value) {
            if t.value.map.is_some() {
                story.scoped.insert(tr.id.clone());
            }
            story.triggers.push(tr);
        }
    }
    let wanted = opened_dialogs(pkg, map);
    for d in pkg.dialogs.iter().filter(|d| wanted.contains(&d.value.id)) {
        b.at = format!("{}:{}", d.file, d.line);
        if let Some(dd) = b.dialog(&d.value) {
            story.dialogs.push(dd);
        }
    }
    story.named = b.named_order.iter().map(|id| named_text(&story.texts, id, map)).collect();
    story.army_names = b.army_names.clone();
    story.army_names.extend(b.spawns.iter().map(|(name, id)| (*id, name.clone())));
    story.building_names = b.building_names.clone();
    if b.errors.is_empty() {
        Ok(story)
    } else {
        Err(b.errors)
    }
}

/// The display name of named character `id` in the story of `map`: its key `named.<id>`; else the
/// map's own `<map>.named.<id>`; else the first key `<block>.named.<id>` of any chapter (in key
/// order); else the id itself.
pub fn named_text(texts: &BTreeMap<String, String>, id: &str, map: &str) -> String {
    if let Some(t) = texts.get(&format!("named.{id}")) {
        return t.clone();
    }
    if let Some(t) = texts.get(&format!("{map}.named.{id}")) {
        return t.clone();
    }
    let suffix = format!(".named.{id}");
    texts.iter().find(|(k, _)| k.ends_with(&suffix)).map_or_else(|| id.to_string(), |(_, v)| v.clone())
}

/// Compiles conditions (a campaign's `if`, `campaign.toml`) against the names of a map, as a
/// trigger's `if` is compiled.
pub fn compile_conditions(pkg: &Package, resolve: &dyn Resolve, conds: &[pt::Cond]) -> Result<Vec<Cond>, Vec<String>> {
    let mut b = Builder {
        pkg,
        resolve,
        spawns: BTreeMap::new(),
        named: BTreeMap::new(),
        named_order: Vec::new(),
        army_names: BTreeMap::new(),
        building_names: BTreeMap::new(),
        at: String::new(),
        errors: Vec::new(),
    };
    let out: Option<Vec<Cond>> = conds.iter().map(|c| b.cond(c)).collect();
    match out {
        Some(out) if b.errors.is_empty() => Ok(out),
        _ => Err(b.errors),
    }
}

/// Whether a trigger applies to map `map`: one without `map`, or one naming it.
fn applies(t: &pt::Trigger, map: &str) -> bool {
    t.map.as_deref().is_none_or(|m| m == map)
}

/// The dialogs the triggers of map `map` open, and the dialogs those open in their choices.
fn opened_dialogs(pkg: &Package, map: &str) -> BTreeSet<String> {
    let mut todo: Vec<String> = pkg.triggers.iter().filter(|t| applies(&t.value, map)).flat_map(|t| opens(&t.value.then)).collect();
    let mut seen = BTreeSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(d) = pkg.dialogs.iter().find(|d| d.value.id == id) {
            todo.extend(d.value.choices.iter().flat_map(|c| opens(&c.then)));
        }
    }
    seen
}

/// The dialogs an action list opens.
fn opens(then: &[pt::Action]) -> Vec<String> {
    then.iter()
        .filter_map(|a| match a {
            pt::Action::Dialog(d) | pt::Action::Ending(d) => Some(d.clone()),
            _ => None,
        })
        .collect()
}

fn default_of(v: &pt::Var) -> VarValue {
    v.default.clone()
}

struct Builder<'a> {
    pkg: &'a Package,
    resolve: &'a dyn Resolve,
    /// The package's spawned armies, by name.
    spawns: BTreeMap<String, ArmyId>,
    /// Named characters by name, in order of first mention.
    named: BTreeMap<String, u8>,
    named_order: Vec<String>,
    /// The scenario's armies and buildings the story names, by id, for the trace.
    army_names: BTreeMap<ArmyId, String>,
    building_names: BTreeMap<u16, String>,
    /// Where the table being compiled is, for the errors.
    at: String,
    errors: Vec<String>,
}

impl Builder<'_> {
    /// Gives every name a `spawn_army` names its army id, in order of first mention.
    fn assign_spawns(&mut self) {
        let mut all: Vec<&pt::Action> = Vec::new();
        for t in &self.pkg.triggers {
            all.extend(&t.value.then);
        }
        for d in &self.pkg.dialogs {
            for c in &d.value.choices {
                all.extend(&c.then);
            }
        }
        for a in all {
            if let pt::Action::SpawnArmy { army, .. } = a {
                if !self.spawns.contains_key(army) && self.spawns.len() < PACKAGE_SLOTS {
                    self.spawns.insert(army.clone(), PACKAGE_BASE + self.spawns.len() as u8);
                }
            }
        }
    }

    fn fail<T>(&mut self, message: impl Into<String>) -> Option<T> {
        self.errors.push(format!("{}: {}", self.at, message.into()));
        None
    }

    fn army(&mut self, name: &str) -> Option<ArmyId> {
        if let Some(id) = self.spawns.get(name) {
            return Some(*id);
        }
        match self.resolve.army(name) {
            Some(id) => {
                self.army_names.insert(id, name.to_string());
                Some(id)
            }
            None => self.fail(format!("no army `{name}`: a spawn_army of this package or a scenario army")),
        }
    }

    fn named(&mut self, name: &str) -> Option<u8> {
        if let Some(id) = self.named.get(name) {
            return Some(*id);
        }
        if self.named_order.len() >= PACKAGE_SLOTS {
            return self.fail("more named characters than the package can hold (56)");
        }
        let id = PACKAGE_BASE + self.named_order.len() as u8;
        self.named.insert(name.to_string(), id);
        self.named_order.push(name.to_string());
        Some(id)
    }

    fn building(&mut self, name: &str) -> Option<u16> {
        match self.resolve.building(name) {
            Some(id) => {
                self.building_names.insert(id, name.to_string());
                Some(id)
            }
            None => self.fail(format!("no building `{name}` in the scenario")),
        }
    }

    fn unit(&mut self, r: &Ref) -> Option<UnitId> {
        let id = match r {
            Ref::Dt { kind: DtKind::Unit, alias } => aliases::resolve(DtKind::Unit, alias).map(UnitId),
            Ref::Local { id } => self.resolve.unit(id),
            _ => None,
        };
        match id {
            Some(u) if u.0 <= 255 => Some(u),
            Some(_) => self.fail(format!("unit `{r}` has an id above 255: not supported yet")),
            None => self.fail(format!("unit `{r}` does not resolve")),
        }
    }

    fn item(&mut self, r: &Ref) -> Option<ItemId> {
        let id = match r {
            Ref::Dt { kind: DtKind::Item, alias } => aliases::resolve(DtKind::Item, alias).map(ItemId),
            Ref::Local { id } => self.resolve.item(id),
            _ => None,
        };
        match id {
            Some(i) => Some(i),
            None => self.fail(format!("item `{r}` does not resolve")),
        }
    }

    fn spell(&mut self, r: &Ref) -> Option<u32> {
        match r {
            Ref::Dt { kind: DtKind::Spell, alias } => match aliases::resolve(DtKind::Spell, alias) {
                Some(s) => Some(s),
                None => self.fail(format!("spell `{r}` does not resolve")),
            },
            _ => self.fail(format!("spell `{r}` is not a base spell")),
        }
    }

    fn who(&mut self, h: &pt::Holder) -> Option<Who> {
        Some(match h {
            pt::Holder::Player => Who::Player,
            pt::Holder::Pack => Who::Pack,
            pt::Holder::Army(name) => Who::Army(self.army(name)?),
            pt::Holder::Named(name) => Who::Named(self.named(name)?),
            pt::Holder::Garrison(name) => Who::Garrison(self.building(name)?),
        })
    }

    fn goal_target(&mut self, t: &pt::Target) -> Option<GoalTarget> {
        Some(match t {
            pt::Target::Hero => GoalTarget::Hero,
            pt::Target::Army(name) => GoalTarget::Army(self.army(name)?),
        })
    }

    fn entity(&mut self, e: &pt::Entity) -> Option<Entity> {
        Some(match e {
            pt::Entity::Army(name) => Entity::Army(self.army(name)?),
            pt::Entity::Building(name) => Entity::Building(self.building(name)?),
        })
    }

    fn declared(&mut self, name: &str) -> Option<()> {
        if self.pkg.vars.iter().any(|v| v.value.name == name) {
            Some(())
        } else {
            self.fail(format!("variable `{name}` is not declared"))
        }
    }

    fn when(&mut self, e: &pt::Event) -> Option<When> {
        Some(match e {
            pt::Event::GameStart => When::GameStart,
            pt::Event::DayStart => When::DayStart,
            pt::Event::WeekStart => When::WeekStart,
            pt::Event::NewMoon => When::NewMoon,
            pt::Event::HeroEnters(p) => When::HeroEnters(*p),
            pt::Event::HeroEntersBuilding(b) => When::HeroEntersBuilding(self.building(b)?),
            pt::Event::ArmyDefeated(a) => When::ArmyDefeated(self.army(a)?),
            pt::Event::BuildingCaptured(b) => When::BuildingCaptured(self.building(b)?),
            pt::Event::BuildingLost(b) => When::BuildingLost(self.building(b)?),
            pt::Event::SimVictory => When::SimVictory,
            pt::Event::DialogChoice { dialog, choice } => When::DialogChoice { dialog: dialog.clone(), choice: choice.clone() },
            pt::Event::VarChanged(name) => {
                self.declared(name)?;
                When::VarChanged(name.clone())
            }
            pt::Event::ArmyArrives { army, at, building } => {
                let building = match building {
                    Some(b) => Some(self.building(b)?),
                    None => None,
                };
                When::ArmyArrives { army: self.army(army)?, at: *at, building }
            }
        })
    }

    fn cond(&mut self, c: &pt::Cond) -> Option<Cond> {
        Some(match c {
            pt::Cond::Var { name, cmp, value } => {
                self.declared(name)?;
                Cond::Var { name: name.clone(), cmp: *cmp, value: value.clone() }
            }
            pt::Cond::Random(p) => Cond::Random(*p),
            pt::Cond::HeroAt(p) => Cond::HeroAt(*p),
            pt::Cond::ArmyAt { army, at } => Cond::ArmyAt(self.army(army)?, *at),
            pt::Cond::HasNamed { named, unit, holder, present } => Cond::HasNamed { named: self.named(named)?, unit: self.unit(unit)?, who: self.who(holder)?, present: *present },
            pt::Cond::HasItem { item, holder } => Cond::HasItem(self.item(item)?, self.who(holder)?),
            pt::Cond::Gold { amount, cmp } => Cond::Gold(*amount, *cmp),
            pt::Cond::ArmyAlive { army, alive } => Cond::ArmyAlive(self.army(army)?, *alive),
            pt::Cond::BuildingOwner { building, faction } => Cond::BuildingOwner(self.building(building)?, *faction),
            pt::Cond::HasSpell { spell, holder, all } => Cond::HasSpell { spell: self.spell(spell)?, who: self.who(holder)?, all: *all },
        })
    }

    fn units(&mut self, troops: &[Ref]) -> Option<Vec<UnitId>> {
        troops.iter().map(|t| self.unit(t)).collect()
    }

    fn figure(&mut self, f: &pt::Figure) -> Option<FigureOf> {
        Some(match f {
            pt::Figure::Unit(u) => FigureOf::Unit(self.unit(u)?),
            pt::Figure::Model(code) => FigureOf::Model(*code),
        })
    }

    fn act(&mut self, a: &pt::Action) -> Option<Act> {
        Some(match a {
            pt::Action::Set { name, value } => {
                self.declared(name)?;
                Act::Set(name.clone(), value.clone())
            }
            pt::Action::Add { name, value, min, max } => {
                self.declared(name)?;
                Act::Add { name: name.clone(), value: *value, min: *min, max: *max }
            }
            pt::Action::RandomVar { name, min, max } => {
                self.declared(name)?;
                Act::RandomVar(name.clone(), *min, *max)
            }
            pt::Action::Dialog(d) => {
                if !self.pkg.dialogs.iter().any(|x| &x.value.id == d) {
                    return self.fail(format!("no dialog `{d}`"));
                }
                Act::Dialog(d.clone())
            }
            pt::Action::GiveGold(n) => Act::GiveGold(*n),
            pt::Action::GiveItem { item, to } => Act::GiveItem(self.item(item)?, self.who(to)?),
            pt::Action::GiveUnit { unit, named } => {
                let u = self.unit(unit)?;
                let k = match named {
                    Some(n) => self.named(n)?,
                    None => 0,
                };
                Act::GiveUnit(u, k)
            }
            pt::Action::GiveXp { amount, to } => Act::GiveXp(*amount, self.who(to)?),
            pt::Action::SpawnArmy { army, at, faction, model, troops } => {
                Act::Spawn { army: self.army(army)?, at: *at, faction: *faction, model: *model, troops: self.units(troops)? }
            }
            pt::Action::RemoveArmy(a) => Act::RemoveArmy(self.army(a)?),
            pt::Action::StartBattle { army, rules } => Act::StartBattle(self.army(army)?, rules.clone()),
            pt::Action::TeleportHero(t) => Act::TeleportHero(*t),
            pt::Action::TeleportArmy { army, to } => {
                let id = self.army(army)?;
                let dest = match to {
                    pt::Dest::Cell(x, y) => Dest::Cell(*x, *y),
                    pt::Dest::Building(b) => Dest::Building(self.building(b)?),
                };
                Act::TeleportArmy(id, dest)
            }
            pt::Action::SetFaction { of, faction } => Act::SetFaction(self.entity(of)?, *faction),
            pt::Action::SetRelation { of, towards, value } => Act::SetRelation(self.entity(of)?, *towards, *value as i8),
            pt::Action::AddUnits { of, troops } => Act::AddUnits(self.entity(of)?, self.units(troops)?),
            pt::Action::Cast { spell, holder, until } => Act::Cast { spell: self.spell(spell)?, who: self.who(holder)?, until: *until },
            pt::Action::RemoveSpells(h) => Act::RemoveSpells(self.who(h)?),
            pt::Action::ForgetSpell(s) => Act::ForgetSpell(self.spell(s)?),
            pt::Action::Victory => Act::End(Ending::Victory),
            pt::Action::Defeat => Act::End(Ending::Defeat),
            pt::Action::ArmyGoal { army, goal } => {
                let army = self.army(army)?;
                let goal = match goal {
                    pt::GoalSpec::GoTo(c) => Some(Goal::GoTo(*c)),
                    pt::GoalSpec::GoToBuilding(b) => Some(Goal::GoToBuilding(self.building(b)?)),
                    pt::GoalSpec::Follow(t) => Some(Goal::Follow(self.goal_target(t)?)),
                    pt::GoalSpec::Attack(t) => Some(Goal::Attack(self.goal_target(t)?)),
                    pt::GoalSpec::Patrol(cells) => Some(Goal::Patrol(cells.clone())),
                    pt::GoalSpec::Clear => None,
                };
                Act::ArmyGoal { army, goal }
            }
            pt::Action::ArmySpeed { army, percent } => Act::ArmySpeed { army: self.army(army)?, percent: *percent },
            pt::Action::HeroFigure(figure) => Act::HeroFigure(match figure {
                Some(f) => Some(self.figure(f)?),
                None => None,
            }),
            pt::Action::ArmyFigure { army, figure } => Act::ArmyFigure { army: self.army(army)?, figure: self.figure(figure)? },
            pt::Action::RemoveUnit { named, holder } => Act::RemoveUnit { named: self.named(named)?, who: self.who(holder)? },
            pt::Action::VictoryTo(map) => Act::VictoryTo(map.clone()),
            pt::Action::Travel { map, at } => Act::Travel { map: map.clone(), at: at.clone() },
            pt::Action::Quest { id, title, text } => Act::Quest { id: id.clone(), title: title.clone(), text: text.clone() },
            pt::Action::CompleteQuest(id) => Act::CompleteQuest(id.clone()),
            pt::Action::Ending(dialog) => {
                if !self.pkg.dialogs.iter().any(|x| &x.value.id == dialog) {
                    return self.fail(format!("no dialog `{dialog}`"));
                }
                Act::EndCampaign(dialog.clone())
            }
        })
    }

    fn trigger(&mut self, t: &pt::Trigger) -> Option<Trigger> {
        let when = self.when(&t.when)?;
        let cond = t.cond.iter().map(|c| self.cond(c)).collect::<Option<Vec<_>>>()?;
        let then = t.then.iter().map(|a| self.act(a)).collect::<Option<Vec<_>>>()?;
        Some(Trigger { id: t.id.clone(), when, cond, then, once: t.once, chance: t.chance })
    }

    fn dialog(&mut self, d: &pt::Dialog) -> Option<DialogDef> {
        let speaker = match &d.speaker {
            Some(r) => Some(self.unit(r)?),
            None => None,
        };
        let choices = d
            .choices
            .iter()
            .map(|c| {
                let cond = c.cond.iter().map(|x| self.cond(x)).collect::<Option<Vec<_>>>()?;
                let then = c.then.iter().map(|x| self.act(x)).collect::<Option<Vec<_>>>()?;
                Some(Choice { id: c.id.clone(), text: c.text.clone(), cond, then })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(DialogDef { id: d.id.clone(), title: d.title.clone(), text: d.text.clone(), speaker, choices })
    }
}

/// A moment as the trace writes it: the event, with the names the story gives its armies and
/// buildings.
fn moment_name(story: &Story, when: &When) -> String {
    let army = |id: &ArmyId| story.army_names.get(id).cloned().unwrap_or_else(|| format!("#{id}"));
    let building = |id: &u16| story.building_names.get(id).cloned().unwrap_or_else(|| format!("#{id}"));
    match when {
        When::GameStart => "game_start".into(),
        When::DayStart => "day_start".into(),
        When::WeekStart => "week_start".into(),
        When::NewMoon => "new_moon".into(),
        When::HeroEnters(Place::Cell(x, y)) => format!("hero_enters [{x}, {y}]"),
        When::HeroEnters(Place::Area(x, y, w, h)) => format!("hero_enters [{x}, {y}, {w}, {h}]"),
        When::HeroEntersBuilding(b) => format!("hero_enters_building {}", building(b)),
        When::ArmyDefeated(a) => format!("army_defeated {}", army(a)),
        When::BuildingCaptured(b) => format!("building_captured {}", building(b)),
        When::BuildingLost(b) => format!("building_lost {}", building(b)),
        When::SimVictory => "sim_victory".into(),
        When::DialogChoice { dialog, choice } => format!("dialog_choice {dialog}.{choice}"),
        When::VarChanged(v) => format!("var_changed {v}"),
        When::ArmyArrives { army: a, at, building: b } => {
            let mut text = format!("army_arrives {}", army(a));
            if let Some((x, y)) = at {
                text += &format!(" [{x}, {y}]");
            }
            if let Some(b) = b {
                text += &format!(" in {}", building(b));
            }
            text
        }
    }
}

/// A goal as the trace writes it, with the names the story gives its armies and buildings.
fn goal_text(story: &Story, goal: &Goal) -> String {
    let army = |id: &ArmyId| story.army_names.get(id).cloned().unwrap_or_else(|| format!("#{id}"));
    let target = |t: &GoalTarget| match t {
        GoalTarget::Hero => "hero".to_string(),
        GoalTarget::Army(id) => army(id),
    };
    match goal {
        Goal::GoTo((x, y)) => format!("go_to [{x}, {y}]"),
        Goal::GoToBuilding(b) => format!("go_to_building {}", story.building_names.get(b).cloned().unwrap_or_else(|| format!("#{b}"))),
        Goal::Follow(t) => format!("follow {}", target(t)),
        Goal::Attack(t) => format!("attack {}", target(t)),
        Goal::Patrol(cells) => format!("patrol through {} cells", cells.len()),
    }
}

/// Whether trigger moment `on` is raised `raised`: a cell inside an area, else the same moment.
fn matches(on: &When, raised: &When) -> bool {
    match (on, raised) {
        (When::HeroEnters(p), When::HeroEnters(Place::Cell(x, y))) => place_has(p, *x, *y),
        (When::ArmyArrives { army, at, building }, When::ArmyArrives { army: raised, at: cell, building: raised_building }) => {
            army == raised && at.is_none_or(|p| Some(p) == *cell) && building.is_none_or(|b| Some(b) == *raised_building)
        }
        (a, b) => a == b,
    }
}

fn place_has(p: &Place, x: i32, y: i32) -> bool {
    match *p {
        Place::Cell(a, b) => a == x && b == y,
        Place::Area(a, b, w, h) => x >= a && x < a + w && y >= b && y < b + h,
    }
}

fn compare(v: i64, cmp: Cmp, n: i64) -> bool {
    match cmp {
        Cmp::Eq => v == n,
        Cmp::Ne => v != n,
        Cmp::Lt => v < n,
        Cmp::Le => v <= n,
        Cmp::Gt => v > n,
        Cmp::Ge => v >= n,
    }
}

fn set_spell(slots: &mut [Option<SpellSlot>; SPELL_SLOTS], spell: u32, until: u64, now: u64) {
    if let Some(s) = slots.iter_mut().flatten().find(|s| s.spell == spell) {
        s.until = until;
        return;
    }
    if let Some(free) = slots.iter_mut().find(|s| s.as_ref().is_none_or(|x| x.until <= now)) {
        *free = Some(SpellSlot { spell, until });
    }
}

fn has_spell(slots: &[Option<SpellSlot>; SPELL_SLOTS], spell: u32, now: u64) -> bool {
    slots.iter().flatten().any(|s| s.spell == spell && s.until > now)
}

fn entity_holder(e: Entity) -> DtHolder {
    match e {
        Entity::Army(a) => DtHolder::Army(a),
        Entity::Building(b) => DtHolder::Building(b),
    }
}

impl Game {
    /// Attaches a package's story. An empty story attaches nothing. Variables the state does not
    /// hold yet take their defaults; the package generator starts from the story's seed, once.
    pub fn attach_story(&mut self, story: Story) {
        if story.is_empty() {
            self.story = None;
            return;
        }
        // The field battles' hooks come from the package's scripts (the Lua runtime, feature `lua`).
        #[cfg(feature = "lua")]
        {
            self.field_host = story.scripts.clone().map(|s| Arc::new(crate::lua::field::Host::new(s)) as Arc<dyn super::field::FieldHost>);
        }
        for (name, (_, default)) in &story.vars {
            self.triggers.vars.entry(name.clone()).or_insert_with(|| default.clone());
        }
        if self.triggers.rng == 0 {
            self.triggers.rng = story.seed.max(1);
        }
        self.story = Some(Arc::new(story));
    }

    /// The attached story, if any.
    pub fn story(&self) -> Option<&Story> {
        self.story.as_deref()
    }

    /// The package's ending, once a `victory` or `defeat` action ran.
    pub fn story_end(&self) -> Option<Ending> {
        self.triggers.ended
    }

    /// A package variable's current value.
    pub fn story_var(&self, name: &str) -> Option<VarValue> {
        self.triggers.vars.get(name).cloned()
    }

    /// A dialog of the attached story, by id.
    pub fn story_dialog(&self, id: &str) -> Option<&DialogDef> {
        self.story()?.dialogs.iter().find(|d| d.id == id)
    }

    /// A text of the story by its key (the key itself when the story has no such text).
    pub fn story_text(&self, key: &str) -> String {
        self.story().and_then(|s| s.texts.get(key)).cloned().unwrap_or_else(|| key.to_string())
    }

    /// The game starts: the triggers on `game_start` run; their events wait in
    /// [`Game::drain_events`], as the scenario's opening events do.
    pub fn start_story(&mut self) {
        let events = self.story_events(When::GameStart);
        self.pending.extend(events);
    }


    /// The player chose `choice` in dialog `dialog` (`None`: its «Далее»): the choice's actions run,
    /// then the triggers on `dialog_choice`. Returns what followed.
    pub fn answer_story_dialog(&mut self, dialog: &str, choice: Option<&str>) -> Vec<Event> {
        let Some(story) = self.story.clone() else { return Vec::new() };
        let Some(d) = story.dialogs.iter().find(|d| d.id == dialog) else { return Vec::new() };
        let mut queue = VecDeque::new();
        self.note(Trace::Answer { dialog: dialog.to_string(), choice: choice.map(str::to_string) });
        if let Some(cid) = choice {
            let Some(c) = d.choices.iter().find(|c| c.id == cid) else { return Vec::new() };
            for a in &c.then {
                self.act(a, &story, &mut queue);
            }
            queue.push_back(When::DialogChoice { dialog: d.id.clone(), choice: c.id.clone() });
        }
        self.run_queue(&story, queue)
    }

    /// The moments of a midnight that begins `day` (an absolute day index): day start every day,
    /// week start every 7th day and new moon every 28th. Called by the clock's midnight.
    pub(crate) fn story_midnight(&mut self, day: u64) -> Vec<Event> {
        if self.story.is_none() {
            return Vec::new();
        }
        let mut events = self.story_events(When::DayStart);
        if day.is_multiple_of(7) {
            events.extend(self.story_events(When::WeekStart));
        }
        if day.is_multiple_of(28) {
            events.extend(self.story_events(When::NewMoon));
        }
        events
    }

    /// Runs the triggers that wait for `when`. A game without a story does nothing here.
    pub(crate) fn story_events(&mut self, when: When) -> Vec<Event> {
        let Some(story) = self.story.clone() else { return Vec::new() };
        self.run_queue(&story, VecDeque::from([when]))
    }

    /// Runs the triggers for each raised moment in the queue, in order, and hands on the dialogs
    /// they opened. A variable's change queues its `var_changed` moment.
    fn run_queue(&mut self, story: &Story, mut queue: VecDeque<When>) -> Vec<Event> {
        let mut runs = 0;
        while let Some(when) = queue.pop_front() {
            runs += 1;
            if runs > MAX_RUNS {
                crate::diag!("package triggers: loop guard reached, {} moments left for the next", queue.len() + 1);
                break;
            }
            for t in &story.triggers {
                if !matches(&t.when, &when) || (t.once && self.triggers.fired.contains(&t.id)) {
                    continue;
                }
                if !t.cond.iter().all(|c| self.holds(c)) {
                    continue;
                }
                if t.chance < 100 && self.draw(100) >= i64::from(t.chance) {
                    continue;
                }
                if t.once {
                    self.triggers.fired.insert(t.id.clone());
                }
                self.note(Trace::Fired { trigger: t.id.clone(), moment: moment_name(story, &when) });
                for a in &t.then {
                    self.act(a, story, &mut queue);
                }
            }
        }
        std::mem::take(&mut self.triggers.dialogs).into_iter().map(Event::Package).collect()
    }

    /// What a battle's end is compared with: the armies beaten and the buildings taken so far.
    /// None without a story, so a game without one reads nothing.
    pub(crate) fn story_battle_snapshot(&self) -> Option<(BTreeSet<ArmyId>, BTreeSet<u16>)> {
        self.story.as_ref()?;
        Some((self.beaten_armies.clone(), self.world.locations.iter().filter(|l| l.owned()).map(|l| l.id).collect()))
    }

    /// The moments of a battle's end: an army the player beat (`army_defeated`), a building its
    /// garrison's defeat gave him (`building_captured`).
    pub(crate) fn story_battle_end(&mut self, before: (BTreeSet<ArmyId>, BTreeSet<u16>)) -> Vec<Event> {
        let (beaten, owned) = before;
        let armies: Vec<ArmyId> = self.beaten_armies.difference(&beaten).copied().collect();
        let buildings: Vec<u16> = self.world.locations.iter().filter(|l| l.owned()).map(|l| l.id).filter(|id| !owned.contains(id)).collect();
        let mut events = Vec::new();
        for a in armies {
            events.extend(self.story_events(When::ArmyDefeated(a)));
        }
        for b in buildings {
            events.extend(self.story_events(When::BuildingCaptured(b)));
        }
        events
    }

    /// The package generator: a 32-bit linear congruential step, `n` values in `0..n` by the high
    /// bits. Its state is saved; `rules::rng` (the original's generator) is never touched.
    fn draw(&mut self, n: i64) -> i64 {
        self.triggers.rng = self.triggers.rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        if n <= 1 {
            return 0;
        }
        ((u64::from(self.triggers.rng) * n as u64) >> 32) as i64
    }

    fn now(&self) -> u64 {
        self.clock.total_minutes() as u64
    }

    fn var_value(&self, name: &str) -> Option<VarValue> {
        self.triggers.vars.get(name).cloned().or_else(|| self.story().and_then(|s| s.vars.get(name)).map(|v| v.1.clone()))
    }

    fn var_int(&self, name: &str) -> i64 {
        match self.var_value(name) {
            Some(VarValue::Int(n)) => n,
            _ => 0,
        }
    }

    fn set_var(&mut self, name: &str, value: VarValue, queue: &mut VecDeque<When>) {
        if self.var_value(name).as_ref() != Some(&value) {
            self.note(Trace::Var { name: name.to_string(), value: value.clone() });
            self.triggers.vars.insert(name.to_string(), value);
            queue.push_back(When::VarChanged(name.to_string()));
        }
    }

    /// Records a step of the run for the trace, when one is asked for (see [`TriggerState::trace`]).
    pub(crate) fn note(&mut self, what: Trace) {
        let at = self.now();
        if let Some(log) = self.triggers.trace.as_mut() {
            log.push((at, what));
        }
    }

    pub(crate) fn holds(&mut self, c: &Cond) -> bool {
        match c {
            Cond::Var { name, cmp, value } => match (self.var_value(name), value) {
                (Some(VarValue::Int(a)), VarValue::Int(b)) => compare(a, *cmp, *b),
                (Some(VarValue::Bool(a)), VarValue::Bool(b)) => match cmp {
                    Cmp::Eq => a == *b,
                    Cmp::Ne => a != *b,
                    _ => false,
                },
                _ => false,
            },
            Cond::Random(p) => self.draw(100) < i64::from(*p),
            Cond::HeroAt(p) => {
                let (x, y) = self.tile();
                place_has(p, x, y)
            }
            // On the map only: a waiting army keeps its last cell in its record.
            Cond::ArmyAt(id, p) => self.world.armies.iter().find(|a| a.id == *id).is_some_and(|a| {
                let (x, y) = a.tile(&self.world.map);
                place_has(p, x, y)
            }),
            Cond::HasNamed { named, unit, who, present } => self.named_unit_in(*who, *named, *unit) == *present,
            Cond::HasItem(item, who) => self.holds_item(*item, *who),
            Cond::Gold(n, cmp) => compare(i64::from(self.gold), *cmp, *n),
            Cond::ArmyAlive(id, alive) => (self.army_active(*id) || self.army_inactive(*id)) == *alive,
            Cond::BuildingOwner(b, faction) => self.building_state(*b).is_some_and(|(_, f)| f == *faction),
            Cond::HasSpell { spell, who, all } => {
                let now = self.now();
                let slots = self.slots_of(*who);
                if *all {
                    !slots.is_empty() && slots.iter().all(|s| has_spell(s, *spell, now))
                } else {
                    slots.iter().any(|s| has_spell(s, *spell, now))
                }
            }
        }
    }

    /// A named character of `named` (of unit type `unit`) among the troops `who` names.
    fn named_unit_in(&self, who: Who, named: u8, unit: UnitId) -> bool {
        // A named character who died in battle is not in the army any more (his record stays).
        let troop = |t: &Troop| t.named == named && t.unit == unit && t.alive();
        match who {
            Who::Player | Who::Pack | Who::Named(_) => self.squad.iter().any(|u| u.named == named && u.def == unit && u.alive()),
            Who::Army(id) => self.army_ref(id).is_some_and(|a| a.troops.iter().any(troop)),
            Who::Garrison(b) => self.location_ref(b).is_some_and(|l| l.garrison.iter().any(troop)),
        }
    }

    fn holds_item(&self, item: ItemId, who: Who) -> bool {
        match who {
            Who::Player | Who::Pack => self.pack.contains(item) || self.squad.iter().any(|u| u.items.contains(&Some(item))),
            Who::Army(id) => self.army_ref(id).is_some_and(|a| a.items.contains(&item)),
            Who::Named(k) => self.squad.iter().any(|u| u.named == k && u.items.contains(&Some(item))),
            Who::Garrison(b) => self.location_ref(b).is_some_and(|l| l.garrison.iter().any(|t| t.worn.contains(&Some(item))) || l.treasure.contains(&item)),
        }
    }

    fn act(&mut self, a: &Act, story: &Story, queue: &mut VecDeque<When>) {
        match a {
            Act::Set(name, v) => self.set_var(name, v.clone(), queue),
            Act::Add { name, value, min, max } => {
                let mut v = self.var_int(name).saturating_add(*value);
                if let Some(lo) = min {
                    v = v.max(*lo);
                }
                if let Some(hi) = max {
                    v = v.min(*hi);
                }
                self.set_var(name, VarValue::Int(v), queue);
            }
            Act::RandomVar(name, lo, hi) => {
                let v = VarValue::Int(lo + self.draw(hi - lo + 1));
                self.set_var(name, v, queue);
            }
            Act::Dialog(id) => self.open_dialog(id, story),
            Act::GiveGold(n) => self.add_gold(*n),
            Act::GiveItem(item, who) => self.give_package_item(*item, *who),
            Act::GiveUnit(unit, named) => self.add_unit(unit.0 as u8, *named, None),
            Act::GiveXp(n, who) => self.give_package_xp(*n, *who),
            Act::Spawn { army, at, faction, model, troops } => self.spawn_package_army(*army, *at, *faction, *model, troops),
            Act::RemoveArmy(id) => self.deactivate_army(*id),
            Act::StartBattle(id, rules) => self.start_field_battle(*id, rules),
            Act::TeleportHero(t) => self.teleport_player(t.0, t.1),
            Act::TeleportArmy(id, dest) => self.teleport_package_army(*id, *dest),
            Act::SetFaction(of, faction) => {
                let was = match of {
                    Entity::Building(b) => self.location_ref(*b).is_some_and(|l| l.owned()),
                    Entity::Army(_) => false,
                };
                self.set_faction(entity_holder(*of), *faction);
                if let Entity::Building(b) = of {
                    if was && !self.location_ref(*b).is_some_and(|l| l.owned()) {
                        queue.push_back(When::BuildingLost(*b));
                    }
                }
            }
            Act::SetRelation(of, towards, value) => self.set_relation(entity_holder(*of), *towards, *value),
            Act::AddUnits(of, troops) => self.add_package_troops(*of, troops),
            Act::RemoveUnit { named, who } => self.remove_named(*named, *who),
            Act::Cast { spell, who, until } => self.cast_package_spell(*spell, *who, *until),
            Act::RemoveSpells(who) => {
                for slots in self.slots_mut(*who) {
                    *slots = [None; SPELL_SLOTS];
                }
                self.recount_if_hero(*who);
            }
            Act::ForgetSpell(s) => self.forget_spell(*s as u8),
            Act::ArmyGoal { army, goal } => {
                if self.set_army_goal(*army, goal.clone()) {
                    let name = story.army_names.get(army).cloned().unwrap_or_else(|| army.to_string());
                    let text = goal.as_ref().map_or_else(|| "clear".to_string(), |g| goal_text(story, g));
                    self.note(Trace::Goal { army: name, goal: text });
                }
            }
            Act::ArmySpeed { army, percent } => {
                self.set_army_pace(*army, *percent);
            }
            Act::HeroFigure(figure) => {
                self.triggers.hero_sheet = figure.map(|f| f.index(&self.content));
            }
            Act::ArmyFigure { army, figure } => {
                let figure = figure.index(&self.content);
                if let Some(a) = self.army_mut_any(*army) {
                    a.figure = figure;
                }
            }
            Act::End(e) => {
                if self.triggers.ended.is_none() {
                    self.note(Trace::End(*e));
                }
                self.triggers.ended.get_or_insert(*e);
            }
            Act::VictoryTo(map) => {
                self.triggers.next.get_or_insert_with(|| map.clone());
                self.act(&Act::End(Ending::Victory), story, queue);
            }
            Act::Travel { map, at } => {
                self.note(Trace::Travel(map.clone()));
                self.triggers.travel = Some(Travel { map: map.clone(), at: at.clone() });
            }
            Act::Quest { id, title, text } => {
                let minutes = self.clock.total_minutes() as u64;
                self.journal.story_quest(id, minutes, title, text);
            }
            Act::CompleteQuest(id) => {
                let minutes = self.clock.total_minutes() as u64;
                self.journal.story_quest_done(id, minutes);
            }
            Act::EndCampaign(dialog) => {
                self.open_dialog(dialog, story);
                self.note(Trace::Ending(dialog.clone()));
                self.triggers.ending.get_or_insert_with(|| dialog.clone());
            }
        }
    }

    /// Opens dialog `id` now: the choices whose conditions hold are the ones it shows.
    fn open_dialog(&mut self, id: &str, story: &Story) {
        let Some(d) = story.dialogs.iter().find(|d| d.id == id) else { return };
        let choices: Vec<String> = d.choices.iter().filter(|c| c.cond.iter().all(|x| self.holds(x))).map(|c| c.id.clone()).collect();
        self.note(Trace::Dialog { id: d.id.clone(), choices: choices.clone() });
        // The diary keeps every dialog shown, so a closed one can be reread («Сообщения»).
        let minutes = self.clock.total_minutes() as u64;
        self.journal.story_message(&d.id, minutes, &d.title, &d.text);
        self.triggers.dialogs.push(PendingDialog { id: d.id.clone(), choices });
    }

    fn army_ref(&self, id: ArmyId) -> Option<&Army> {
        self.world.armies.iter().chain(self.world.inactive.iter()).find(|a| a.id == id)
    }

    pub(crate) fn army_mut_any(&mut self, id: ArmyId) -> Option<&mut Army> {
        self.world.armies.iter_mut().chain(self.world.inactive.iter_mut()).find(|a| a.id == id)
    }

    /// The spell slots of the units `who` names (copies, for the conditions).
    fn slots_of(&self, who: Who) -> Vec<[Option<SpellSlot>; SPELL_SLOTS]> {
        match who {
            Who::Player | Who::Pack => self.squad.iter().map(|u| u.spells).collect(),
            Who::Named(k) => self.squad.iter().filter(|u| u.named == k).map(|u| u.spells).collect(),
            Who::Army(id) => self.army_ref(id).map_or_else(Vec::new, |a| a.troops.iter().map(|t| t.spells).collect()),
            Who::Garrison(b) => self.location_ref(b).map_or_else(Vec::new, |l| l.garrison.iter().map(|t| t.spells).collect()),
        }
    }

    fn location_ref(&self, b: u16) -> Option<&crate::rules::world::Location> {
        self.world.locations.iter().find(|l| l.id == b)
    }

    fn slots_mut(&mut self, who: Who) -> Vec<&mut [Option<SpellSlot>; SPELL_SLOTS]> {
        match who {
            Who::Player | Who::Pack => self.squad.iter_mut().map(|u| &mut u.spells).collect(),
            Who::Named(k) => self.squad.iter_mut().filter(|u| u.named == k).map(|u| &mut u.spells).collect(),
            Who::Army(id) => {
                let w = &mut self.world;
                match w.armies.iter_mut().chain(w.inactive.iter_mut()).find(|a| a.id == id) {
                    Some(a) => a.troops.iter_mut().map(|t: &mut Troop| &mut t.spells).collect(),
                    None => Vec::new(),
                }
            }
            Who::Garrison(b) => match self.world.locations.iter_mut().find(|l| l.id == b) {
                Some(l) => l.garrison.iter_mut().map(|t: &mut Troop| &mut t.spells).collect(),
                None => Vec::new(),
            },
        }
    }

    fn recount_if_hero(&mut self, who: Who) {
        if matches!(who, Who::Player | Who::Pack | Who::Named(_)) {
            self.mark_dirty(ai::HERO);
            self.recount_hero();
        }
    }

    fn cast_package_spell(&mut self, spell: u32, who: Who, until: Until) {
        let now = self.now();
        let end = match until {
            Until::Permanent => MAP_ARMY_SPELL_END,
            Until::Minutes(m) => now.saturating_add(m),
        };
        for slots in self.slots_mut(who) {
            set_spell(slots, spell, end, now);
        }
        self.recount_if_hero(who);
    }

    /// An item to a holder: an army's loot, a named character's free worn slot (else the pack),
    /// or the pack. A named character not in the army gets nothing.
    fn give_package_item(&mut self, item: ItemId, who: Who) {
        match who {
            Who::Army(id) => {
                if let Some(a) = self.army_mut_any(id) {
                    a.items.push(item);
                }
            }
            Who::Named(k) => {
                let Some(i) = self.squad.iter().position(|u| u.named == k) else { return };
                let content = self.content.clone();
                let free = (0..items::slot_count(&content)).find(|&s| self.squad[i].items[s].is_none());
                match free {
                    Some(s) => items::put_on(&content, &mut self.squad[i], s, item),
                    None => {
                        self.pack.add(item);
                    }
                }
            }
            Who::Garrison(b) => {
                let content = self.content.clone();
                if let Some(l) = self.world.locations.iter_mut().find(|l| l.id == b) {
                    ai::give_item_to(&content, &mut l.garrison, &mut l.treasure, item);
                }
            }
            Who::Player | Who::Pack => {
                self.pack.add(item);
            }
        }
    }

    fn give_package_xp(&mut self, xp: i64, who: Who) {
        match who {
            Who::Player | Who::Pack => self.add_experience(xp),
            Who::Army(id) => self.give_unit_xp(DtHolder::Army(id), None, xp),
            Who::Named(k) => {
                if let Some(i) = self.squad.iter().position(|u| u.named == k) {
                    self.give_unit_xp(DtHolder::Player, Some(i as u8), xp);
                }
            }
            Who::Garrison(b) => self.give_unit_xp(DtHolder::Building(b), None, xp),
        }
    }

    /// A package army on the map at `at`: its troops in formation, the faction's attitude. An id
    /// already taken, or a cell off the map, spawns nothing.
    fn spawn_package_army(&mut self, id: ArmyId, at: Tile, faction: u8, model: u8, troops: &[UnitId]) {
        let taken = self.world.armies.iter().chain(self.world.inactive.iter()).any(|a| a.id == id) || self.world.respawns.iter().any(|r| r.army.id == id);
        if taken || troops.is_empty() || !self.world.map.in_bounds(at) {
            return;
        }
        let content = self.content.clone();
        let mut list: Vec<Troop> = troops.iter().map(|u| Troop::new(*u, 1, Slot::new(Row::Front, 0))).collect();
        arrange_troops(&content, &mut list, 0);
        let speed = Army::speed_for(0, troops[0].0);
        self.world.spawn_army(id, at, faction, model, list, speed);
        // An AI army from its first step on: its record is set up as a fresh one (`fresh_mind`).
        let last = self.world.armies.len() - 1;
        self.fresh_mind(last);
        self.triggers.spawned.insert(id);
        let name = self.story().and_then(|s| s.army_names.get(&id)).cloned().unwrap_or_else(|| id.to_string());
        self.note(Trace::Spawn { army: name });
        if let Some(uid) = self.world.armies.last().map(|a| a.uid) {
            self.mark_dirty(uid);
        }
    }

    fn teleport_package_army(&mut self, id: ArmyId, dest: Dest) {
        let tile = match dest {
            Dest::Cell(x, y) => (x, y),
            Dest::Building(b) => match self.world.locations.iter().find(|l| l.id == b) {
                Some(l) => l.tile,
                None => return,
            },
        };
        if !self.world.map.in_bounds(tile) {
            return;
        }
        let centre = self.world.map.center(tile);
        if let Some(a) = self.army_mut_any(id) {
            a.pos = centre;
            a.post = tile;
            a.box_centre = None;
            a.path.clear();
        }
    }

    /// `remove_unit`: the named character leaves the holder; his worn items go to the pack.
    fn remove_named(&mut self, named: u8, who: Who) {
        let mut worn: Vec<ItemId> = Vec::new();
        match who {
            Who::Player | Who::Pack | Who::Named(_) => {
                for u in self.squad.iter().filter(|u| u.named == named) {
                    worn.extend(u.items.iter().flatten());
                }
                self.squad.retain(|u| u.named != named);
            }
            Who::Army(id) => {
                if let Some(a) = self.army_mut_any(id) {
                    for t in a.troops.iter().filter(|t| t.named == named) {
                        worn.extend(t.worn.iter().flatten());
                    }
                    a.troops.retain(|t| t.named != named);
                }
            }
            Who::Garrison(b) => {
                if let Some(l) = self.world.locations.iter_mut().find(|l| l.id == b) {
                    for t in l.garrison.iter().filter(|t| t.named == named) {
                        worn.extend(t.worn.iter().flatten());
                    }
                    l.garrison.retain(|t| t.named != named);
                }
            }
        }
        for item in worn {
            self.pack.add(item);
        }
    }

    fn add_package_troops(&mut self, of: Entity, troops: &[UnitId]) {
        let content = self.content.clone();
        let fresh = |troops: &[UnitId]| -> Vec<Troop> { troops.iter().map(|u| Troop::new(*u, 1, Slot::new(Row::Front, 0))).collect() };
        match of {
            Entity::Army(id) => {
                if let Some(a) = self.army_mut_any(id) {
                    a.troops.extend(fresh(troops));
                    arrange_troops(&content, &mut a.troops, 0);
                }
            }
            Entity::Building(b) => {
                if let Some(l) = self.world.locations.iter_mut().find(|l| l.id == b) {
                    l.garrison.extend(fresh(troops));
                    let defence = l.garrison_defence;
                    arrange_troops(&content, &mut l.garrison, defence);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::check as pack_check;
    use crate::pack::files::Files;
    use crate::rules::content::{Content, HeroClass};
    use crate::rules::clock::Clock;
    use crate::rules::journal::Tab;
    use crate::rules::script::ScriptEnd;

    fn content() -> Arc<Content> {
        Arc::new(Content::builtin())
    }

    fn game() -> Game {
        Game::new(content(), HeroClass::Knight)
    }

    /// A story made of runtime parts (the compile step is tested on TOML below).
    fn story(vars: &[(&str, VarValue)], triggers: Vec<Trigger>, dialogs: Vec<DialogDef>) -> Story {
        Story {
            vars: vars.iter().map(|(n, v)| (n.to_string(), (kind_of(v), v.clone()))).collect(),
            triggers,
            dialogs,
            texts: BTreeMap::new(),
            named: Vec::new(),
            seed: 7,
            ..Story::default()
        }
    }

    fn kind_of(v: &VarValue) -> VarKind {
        match v {
            VarValue::Int(_) => VarKind::Int,
            VarValue::Bool(_) => VarKind::Bool,
        }
    }

    fn trig(id: &str, when: When, cond: Vec<Cond>, then: Vec<Act>) -> Trigger {
        Trigger { id: id.into(), when, cond, then, once: true, chance: 100 }
    }

    fn dialog(id: &str, choices: Vec<Choice>) -> DialogDef {
        DialogDef { id: id.into(), title: "t".into(), text: "x".into(), speaker: None, choices }
    }

    fn choice(id: &str, cond: Vec<Cond>, then: Vec<Act>) -> Choice {
        Choice { id: id.into(), text: id.into(), cond, then }
    }

    /// A unit type of the base game, by alias.
    fn unit(alias: &str) -> UnitId {
        UnitId(aliases::resolve(DtKind::Unit, alias).expect("alias") )
    }

    fn pending_ids(events: &[Event]) -> Vec<PendingDialog> {
        events.iter().filter_map(|e| if let Event::Package(d) = e { Some(d.clone()) } else { None }).collect()
    }

    #[test]
    fn an_empty_story_changes_nothing_over_thirty_days() {
        let mut plain = game();
        let mut with = game();
        with.attach_story(Story::default());
        assert!(with.story().is_none(), "an empty story attaches nothing");
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for _ in 0..30 {
            plain.pass_time(24.0 * 60.0, &mut a);
            with.pass_time(24.0 * 60.0, &mut b);
        }
        assert_eq!(a, b, "the same events");
        let (sa, sb) = (serde_json::to_string(&plain).unwrap(), serde_json::to_string(&with).unwrap());
        assert_eq!(sa, sb, "the same save, byte for byte");
        assert!(!sa.contains("\"triggers\""), "no state field while no story is in play");
    }

    #[test]
    fn a_save_without_the_field_loads_and_one_with_it_round_trips() {
        let mut g = game();
        let plain = serde_json::to_value(&g).unwrap();
        let loaded: Game = serde_json::from_value(plain).unwrap();
        assert!(loaded.triggers.is_empty());
        g.attach_story(story(&[("dread", VarValue::Int(0))], vec![], vec![dialog("d", vec![])]));
        g.triggers.fired.insert("x".into());
        let json = serde_json::to_string(&g).unwrap();
        assert!(json.contains("\"triggers\""));
        let back: Game = serde_json::from_str(&json).unwrap();
        assert_eq!(back.triggers, g.triggers);
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
    }

    #[test]
    fn conditions_on_variables_random_and_gold() {
        let mut g = game();
        g.attach_story(story(&[("dread", VarValue::Int(3)), ("held", VarValue::Bool(true))], vec![], vec![dialog("d", vec![])]));
        let c = |name: &str, cmp: Cmp, v: VarValue| Cond::Var { name: name.into(), cmp, value: v };
        assert!(g.holds(&c("dread", Cmp::Ge, VarValue::Int(3))));
        assert!(!g.holds(&c("dread", Cmp::Gt, VarValue::Int(3))));
        assert!(g.holds(&c("held", Cmp::Eq, VarValue::Bool(true))));
        assert!(!g.holds(&c("held", Cmp::Ne, VarValue::Bool(true))));
        assert!(!g.holds(&c("dread", Cmp::Eq, VarValue::Bool(true))), "a number is not a flag");
        assert!(g.holds(&Cond::Random(100)));
        assert!(!g.holds(&Cond::Random(0)));
        g.gold = 40;
        assert!(g.holds(&Cond::Gold(40, Cmp::Ge)));
        assert!(!g.holds(&Cond::Gold(41, Cmp::Ge)));
    }

    #[test]
    fn the_package_generator_is_its_own_and_saved() {
        let mut g = game();
        g.attach_story(story(&[], vec![], vec![dialog("d", vec![])]));
        let before = g.rng.state();
        let seeds = g.triggers.rng;
        let rolls: Vec<i64> = (0..5).map(|_| g.draw(100)).collect();
        assert_eq!(g.rng.state(), before, "the original's generator is not drawn");
        assert_ne!(g.triggers.rng, seeds, "the package generator advanced");
        assert!(rolls.iter().all(|r| (0..100).contains(r)));
        let mut h = game();
        h.attach_story(story(&[], vec![], vec![dialog("d", vec![])]));
        let again: Vec<i64> = (0..5).map(|_| h.draw(100)).collect();
        assert_eq!(rolls, again, "the same seed, the same rolls");
    }

    #[test]
    fn variable_actions_and_the_change_moment() {
        let mut g = game();
        let watch = trig("watch", When::VarChanged("dread".into()), vec![], vec![Act::Add { name: "alarm".into(), value: 1, min: None, max: None }]);
        g.attach_story(story(
            &[("dread", VarValue::Int(0)), ("alarm", VarValue::Int(0))],
            vec![watch],
            vec![dialog("d", vec![])],
        ));
        let mut q = VecDeque::new();
        let s = g.story.clone().unwrap();
        g.act(&Act::Add { name: "dread".into(), value: 2, min: None, max: None }, &s, &mut q);
        assert_eq!(g.story_var("dread"), Some(VarValue::Int(2)));
        g.act(&Act::Set("dread".into(), VarValue::Int(2)), &s, &mut q);
        assert_eq!(q.len(), 1, "setting the same value is no change");
        let events = g.run_queue(&s, q);
        assert!(events.is_empty());
        assert_eq!(g.story_var("alarm"), Some(VarValue::Int(1)), "the change ran its trigger");
        g.act(&Act::RandomVar("dread".into(), 5, 5), &s, &mut VecDeque::new());
        assert_eq!(g.story_var("dread"), Some(VarValue::Int(5)), "a range of one value");
    }

    #[test]
    fn runaway_triggers_stop_at_the_loop_guard() {
        // Each change raises the next change: the chain never ends, and the guard stops it.
        let mut g = game();
        let mut bump = trig("bump", When::VarChanged("n".into()), vec![], vec![Act::Add { name: "n".into(), value: 1, min: None, max: None }]);
        bump.once = false;
        g.attach_story(story(&[("n", VarValue::Int(0))], vec![bump], vec![dialog("d", vec![])]));
        g.story_events(When::VarChanged("n".into()));
        match g.story_var("n") {
            Some(VarValue::Int(n)) => assert!(n > 1 && n <= MAX_RUNS as i64 + 1, "stopped by the guard at {n}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn day_week_and_moon_moments() {
        let mut g = game();
        let day = trig("day", When::DayStart, vec![], vec![Act::Add { name: "days".into(), value: 1, min: None, max: None }]);
        let week = trig("week", When::WeekStart, vec![], vec![Act::Add { name: "weeks".into(), value: 1, min: None, max: None }]);
        let moon = trig("moon", When::NewMoon, vec![], vec![Act::Add { name: "moons".into(), value: 1, min: None, max: None }]);
        let repeat = |mut t: Trigger| {
            t.once = false;
            t
        };
        g.attach_story(story(
            &[("days", VarValue::Int(0)), ("weeks", VarValue::Int(0)), ("moons", VarValue::Int(0))],
            vec![repeat(day), repeat(week), repeat(moon)],
            vec![dialog("d", vec![])],
        ));
        for d in 0..30 {
            g.story_midnight(d);
        }
        assert_eq!(g.story_var("days"), Some(VarValue::Int(30)));
        assert_eq!(g.story_var("weeks"), Some(VarValue::Int(5)), "days 0, 7, 14, 21, 28");
        assert_eq!(g.story_var("moons"), Some(VarValue::Int(2)), "days 0 and 28");
    }

    #[test]
    fn a_hero_step_onto_a_cell_or_into_an_area_raises_its_moment() {
        let mut g = game();
        let (x, y) = g.tile();
        let on_cell = trig("cell", When::HeroEnters(Place::Cell(x, y)), vec![], vec![Act::Add { name: "hits".into(), value: 1, min: None, max: None }]);
        let mut on_area = trig("area", When::HeroEnters(Place::Area(x - 1, y - 1, 3, 3)), vec![], vec![Act::Add { name: "area".into(), value: 1, min: None, max: None }]);
        on_area.once = false;
        g.attach_story(story(&[("hits", VarValue::Int(0)), ("area", VarValue::Int(0))], vec![on_cell, on_area], vec![dialog("d", vec![])]));
        g.story_events(When::HeroEnters(Place::Cell(x, y)));
        g.story_events(When::HeroEnters(Place::Cell(x + 1, y)));
        g.story_events(When::HeroEnters(Place::Cell(x + 9, y)));
        assert_eq!(g.story_var("hits"), Some(VarValue::Int(1)), "once, on that cell");
        assert_eq!(g.story_var("area"), Some(VarValue::Int(2)), "twice in the area, not outside it");
    }

    #[test]
    fn a_dialog_opens_with_the_choices_its_conditions_allow() {
        let mut g = game();
        let shown = dialog(
            "gate",
            vec![
                choice("pay", vec![], vec![Act::GiveGold(10)]),
                choice("secret", vec![Cond::Var { name: "dread".into(), cmp: Cmp::Ge, value: VarValue::Int(5) }], vec![]),
            ],
        );
        let open = trig("open", When::GameStart, vec![], vec![Act::Dialog("gate".into())]);
        g.attach_story(story(&[("dread", VarValue::Int(0))], vec![open], vec![shown]));
        g.start_story();
        let shown = pending_ids(&g.drain_events());
        assert_eq!(shown, vec![PendingDialog { id: "gate".into(), choices: vec!["pay".into()] }], "the hidden choice is left out");
    }

    #[test]
    fn a_dialog_without_choices_answers_with_next() {
        let mut g = game();
        let open = trig("open", When::GameStart, vec![], vec![Act::Dialog("note".into())]);
        g.attach_story(story(&[], vec![open], vec![dialog("note", vec![])]));
        g.start_story();
        let shown = pending_ids(&g.drain_events());
        assert_eq!(shown, vec![PendingDialog { id: "note".into(), choices: vec![] }]);
        let gold = g.gold;
        assert!(g.answer_story_dialog("note", None).is_empty());
        assert_eq!(g.gold, gold);
    }

    #[test]
    fn three_triggers_tell_a_story_from_start_to_victory() {
        // 1. On game start a dialog with two choices. 2. «take» sets dread to 2 and gives gold.
        // 3. On the week start, with dread at 2, an army appears. 4. Beating it is the victory.
        let mut g = game();
        let rebels = 200;
        let intro = dialog("intro", vec![choice("take", vec![], vec![Act::Set("dread".into(), VarValue::Int(2)), Act::GiveGold(50)]), choice("refuse", vec![], vec![])]);
        let start = trig("start", When::GameStart, vec![], vec![Act::Dialog("intro".into())]);
        let (x, y) = (2, 2);
        let spawn = trig(
            "spawn",
            When::WeekStart,
            vec![Cond::Var { name: "dread".into(), cmp: Cmp::Ge, value: VarValue::Int(2) }],
            vec![Act::Spawn { army: rebels, at: (x, y), faction: 4, model: 5, troops: vec![unit("knight")] }],
        );
        let win = trig("win", When::ArmyDefeated(rebels), vec![], vec![Act::End(Ending::Victory)]);
        g.attach_story(story(&[("dread", VarValue::Int(0))], vec![start, spawn, win], vec![intro]));
        let gold = g.gold;

        g.start_story();
        let shown = pending_ids(&g.drain_events());
        assert_eq!(shown, vec![PendingDialog { id: "intro".into(), choices: vec!["take".into(), "refuse".into()] }]);

        let after = g.answer_story_dialog("intro", Some("take"));
        assert!(after.is_empty());
        assert_eq!(g.story_var("dread"), Some(VarValue::Int(2)));
        assert_eq!(g.gold, gold + 50);

        let armies = g.world.armies.len();
        g.story_events(When::WeekStart);
        assert_eq!(g.world.armies.len(), armies + 1, "the army appears on the week start");
        assert!(g.world.armies.iter().any(|a| a.id == rebels && a.faction == 4));
        assert!(g.triggers.spawned.contains(&rebels));
        assert_eq!(g.script_end(), None);

        g.story_events(When::ArmyDefeated(rebels));
        assert_eq!(g.story_end(), Some(Ending::Victory));
        assert_eq!(g.script_end(), Some(ScriptEnd::Package(Ending::Victory)));
    }

    #[test]
    fn a_choice_runs_its_triggers_and_the_others_do_not() {
        let mut g = game();
        let d = dialog("d", vec![choice("a", vec![], vec![]), choice("b", vec![], vec![])]);
        let on_a = trig("on_a", When::DialogChoice { dialog: "d".into(), choice: "a".into() }, vec![], vec![Act::Add { name: "a".into(), value: 1, min: None, max: None }]);
        let on_b = trig("on_b", When::DialogChoice { dialog: "d".into(), choice: "b".into() }, vec![], vec![Act::Add { name: "b".into(), value: 1, min: None, max: None }]);
        g.attach_story(story(&[("a", VarValue::Int(0)), ("b", VarValue::Int(0))], vec![on_a, on_b], vec![d]));
        g.answer_story_dialog("d", Some("b"));
        assert_eq!(g.story_var("a"), Some(VarValue::Int(0)));
        assert_eq!(g.story_var("b"), Some(VarValue::Int(1)));
        assert!(g.answer_story_dialog("d", Some("nope")).is_empty(), "an unknown choice does nothing");
    }

    #[test]
    fn items_units_and_xp_go_to_their_holders() {
        let mut g = game();
        let relic = ItemId(aliases::resolve(DtKind::Item, "viking_sword").expect("alias"));
        g.attach_story(story(&[], vec![], vec![dialog("d", vec![])]));
        let s = g.story.clone().unwrap();
        let mut q = VecDeque::new();
        let before = g.pack.iter().count();
        g.act(&Act::GiveItem(relic, Who::Pack), &s, &mut q);
        assert_eq!(g.pack.iter().count(), before + 1);
        assert!(g.holds(&Cond::HasItem(relic, Who::Player)));
        let squad = g.squad.len();
        g.act(&Act::GiveUnit(unit("knight"), 0), &s, &mut q);
        assert_eq!(g.squad.len(), squad + 1, "a unit joins the hero's army");
        let xp = g.hero().xp;
        g.act(&Act::GiveXp(100, Who::Player), &s, &mut q);
        assert!(g.hero().xp > xp || g.hero().level > 1, "the hero gains experience");
    }

    #[test]
    fn spells_on_units_and_armies() {
        let mut g = game();
        let spell = aliases::resolve(DtKind::Spell, "fireball").unwrap_or(1);
        g.attach_story(story(&[], vec![], vec![dialog("d", vec![])]));
        let s = g.story.clone().unwrap();
        let mut q = VecDeque::new();
        let c = Cond::HasSpell { spell, who: Who::Player, all: true };
        assert!(!g.holds(&c));
        g.act(&Act::Cast { spell, who: Who::Player, until: Until::Minutes(120) }, &s, &mut q);
        assert!(g.holds(&Cond::HasSpell { spell, who: Who::Player, all: true }), "every unit of the hero's army holds it");
        g.act(&Act::RemoveSpells(Who::Player), &s, &mut q);
        assert!(!g.holds(&Cond::HasSpell { spell, who: Who::Player, all: false }));
        g.act(&Act::Cast { spell, who: Who::Player, until: Until::Permanent }, &s, &mut q);
        assert!(g.holds(&Cond::HasSpell { spell, who: Who::Player, all: false }));
        g.act(&Act::ForgetSpell(spell), &s, &mut q);
        assert!(!g.spells.contains(&(spell as u8)));
    }

    #[test]
    fn a_package_army_spawns_once_moves_and_leaves() {
        let mut g = game();
        g.attach_story(story(&[], vec![], vec![dialog("d", vec![])]));
        let s = g.story.clone().unwrap();
        let mut q = VecDeque::new();
        let troops = vec![unit("knight")];
        g.act(&Act::Spawn { army: 201, at: (3, 3), faction: 4, model: 5, troops: troops.clone() }, &s, &mut q);
        let n = g.world.armies.len();
        g.act(&Act::Spawn { army: 201, at: (3, 3), faction: 4, model: 5, troops: troops.clone() }, &s, &mut q);
        assert_eq!(g.world.armies.len(), n, "an id in use spawns nothing");
        assert!(g.holds(&Cond::ArmyAt(201, Place::Cell(3, 3))));
        assert!(g.holds(&Cond::ArmyAlive(201, true)));
        g.act(&Act::TeleportArmy(201, Dest::Cell(4, 4)), &s, &mut q);
        assert!(g.holds(&Cond::ArmyAt(201, Place::Area(4, 4, 1, 1))), "teleported");
        g.act(&Act::RemoveArmy(201), &s, &mut q);
        assert!(!g.holds(&Cond::ArmyAt(201, Place::Cell(4, 4))), "it left the map");
    }

    #[test]
    fn the_faction_and_relation_of_an_army_and_a_building() {
        let mut g = game();
        g.attach_story(story(&[], vec![], vec![dialog("d", vec![])]));
        let s = g.story.clone().unwrap();
        let mut q = VecDeque::new();
        g.act(&Act::Spawn { army: 202, at: (5, 5), faction: 4, model: 5, troops: vec![unit("knight")] }, &s, &mut q);
        g.act(&Act::SetFaction(Entity::Army(202), 3), &s, &mut q);
        assert_eq!(g.army_ref(202).map(|a| a.faction), Some(3));
        let b = g.world.locations[0].id;
        g.act(&Act::SetFaction(Entity::Building(b), 4), &s, &mut q);
        assert_eq!(g.world.locations[0].faction, 4);
        assert!(g.holds(&Cond::BuildingOwner(b, 4)));
        g.act(&Act::AddUnits(Entity::Army(202), vec![unit("knight")]), &s, &mut q);
        assert_eq!(g.army_ref(202).map(|a| a.troops.len()), Some(2));
    }

    #[test]
    fn a_victory_and_a_defeat_end_the_story_once() {
        let mut g = game();
        g.attach_story(story(&[], vec![], vec![dialog("d", vec![])]));
        let s = g.story.clone().unwrap();
        let mut q = VecDeque::new();
        g.act(&Act::End(Ending::Defeat), &s, &mut q);
        g.act(&Act::End(Ending::Victory), &s, &mut q);
        assert_eq!(g.story_end(), Some(Ending::Defeat), "the first ending holds");
        assert_eq!(g.script_end(), Some(ScriptEnd::Package(Ending::Defeat)));
    }

    fn package(files: &[(&str, &str)]) -> Package {
        let mut f = Files::default();
        for (p, t) in files {
            f.insert(p, t.as_bytes());
        }
        Package::parse(f).0
    }

    #[test]
    fn a_toml_story_compiles_to_engine_ids() {
        let manifest = "id = \"demo\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"story.title\"\nauthor = \"Razdor\"\nlicense = \"MIT\"\n";
        let pkg = package(&[
            ("package.toml", manifest),
            ("texts/en.toml", "[story]\ntitle = \"Story\"\n\n[gate]\ntitle = \"The gate\"\ntext = \"Who goes there?\"\npay = \"Pay\"\n\n[named]\nivo = \"Ivo the Bold\"\n"),
            (
                "triggers/main.toml",
                "[vars]\ndread = 0\nheld = false\n\n[[trigger]]\nid = \"start\"\nwhen = { event = \"game_start\" }\nthen = [ { dialog = \"gate\" } ]\n\n[[trigger]]\nid = \"spawn\"\nwhen = { event = \"week_start\" }\nif = [ { var = \"dread\", op = \">=\", value = 1 } ]\nthen = [ { spawn_army = \"rebels\", at = [5, 5], faction = \"enemy\", troops = [\"dt:knight\"] } ]\n\n[[trigger]]\nid = \"win\"\nwhen = { event = \"army_defeated\", army = \"rebels\" }\nthen = [ { victory = true }, { give_gold = 5 } ]\n\n[[trigger]]\nid = \"take\"\nwhen = { event = \"dialog_choice\", dialog = \"gate\", choice = \"pay\" }\nthen = [ { set = \"dread\", value = 1 }, { give_unit = \"dt:knight\", named = \"ivo\" } ]\n",
            ),
            (
                "dialogs/gate.toml",
                "[[dialog]]\nid = \"gate\"\ntitle = \"gate.title\"\ntext = \"gate.text\"\nspeaker = \"dt:knight\"\n\n[[dialog.choice]]\nid = \"pay\"\ntext = \"gate.pay\"\nthen = [ { give_gold = -10 } ]\n",
            ),
        ]);
        let report = pack_check::check(&pkg);
        assert!(!report.has_errors(), "{}", report.to_text());
        let story = compile(&pkg, "en", "demo", &NoScenario, 9).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(story.triggers.len(), 4);
        assert_eq!(story.dialogs.len(), 1);
        assert_eq!(story.dialogs[0].speaker, Some(unit("knight")));
        assert_eq!(story.named, ["Ivo the Bold"], "the named character takes its text");
        assert_eq!(story.seed, 9);
        // The army «rebels» is the package's first spawned army: id 200.
        assert!(matches!(&story.triggers[2].when, When::ArmyDefeated(200)));
        assert!(matches!(&story.triggers[1].then[0], Act::Spawn { army: 200, .. }));
        // The named character is the first named one: id 200, and the trigger gives it to the hero.
        assert!(matches!(&story.triggers[3].then[1], Act::GiveUnit(_, 200)));
        let mut g = game();
        g.attach_story(story);
        g.start_story();
        let shown = pending_ids(&g.drain_events());
        assert_eq!(shown, vec![PendingDialog { id: "gate".into(), choices: vec!["pay".into()] }]);
        g.gold = 100;
        g.answer_story_dialog("gate", Some("pay"));
        assert_eq!(g.gold, 90, "the choice's gold");
        assert_eq!(g.story_var("dread"), Some(VarValue::Int(1)));
        let ivo = g.squad.iter().find(|u| u.named == 200).cloned().expect("the named unit joined");
        assert_eq!(g.unit_label(&ivo), "Ivo the Bold", "its own name");
    }

    #[test]
    fn a_story_with_an_unknown_name_does_not_compile() {
        let manifest = "id = \"demo\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"story.title\"\nauthor = \"Razdor\"\nlicense = \"MIT\"\n";
        let pkg = package(&[
            ("package.toml", manifest),
            ("texts/en.toml", "[story]\ntitle = \"Story\"\n"),
            ("triggers/main.toml", "[[trigger]]\nid = \"t\"\nwhen = { event = \"army_defeated\", army = \"ghost\" }\nthen = [ { give_gold = 1 } ]\n"),
        ]);
        let errs = compile(&pkg, "en", "demo", &NoScenario, 1).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("no army `ghost`") && e.contains("triggers/main.toml:")), "{errs:?}");
    }

    #[test]
    fn a_trigger_scoped_to_a_map_compiles_only_on_that_map() {
        let manifest = "id = \"demo\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"story.title\"\nauthor = \"Razdor\"\nlicense = \"MIT\"\n";
        let pkg = package(&[
            ("package.toml", manifest),
            ("texts/en.toml", "[story]\ntitle = \"Story\"\n"),
            (
                "triggers/main.toml",
                "[[trigger]]\nid = \"here\"\nmap = \"one\"\nwhen = { event = \"game_start\" }\nthen = [ { give_gold = 1 } ]\n\n[[trigger]]\nid = \"all\"\nwhen = { event = \"game_start\" }\nthen = [ { give_gold = 2 } ]\n",
            ),
        ]);
        let on_one = compile(&pkg, "en", "one", &NoScenario, 1).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(on_one.triggers.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["here", "all"]);
        let on_two = compile(&pkg, "en", "two", &NoScenario, 1).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(on_two.triggers.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["all"], "the other map's trigger is left out");
    }

    #[test]
    fn the_trace_keeps_what_the_run_did_only_when_asked() {
        let mut g = game();
        let intro = dialog("intro", vec![choice("take", vec![], vec![Act::Set("dread".into(), VarValue::Int(2))])]);
        let start = trig("start", When::GameStart, vec![], vec![Act::Dialog("intro".into())]);
        g.attach_story(story(&[("dread", VarValue::Int(0))], vec![start], vec![intro.clone()]));
        g.triggers.trace = Some(Vec::new());
        g.start_story();
        g.drain_events();
        g.answer_story_dialog("intro", Some("take"));
        let log: Vec<Trace> = g.triggers.trace.clone().unwrap_or_default().into_iter().map(|(_, t)| t).collect();
        assert!(log.contains(&Trace::Fired { trigger: "start".into(), moment: "game_start".into() }));
        assert!(log.contains(&Trace::Dialog { id: "intro".into(), choices: vec!["take".into()] }));
        assert!(log.contains(&Trace::Var { name: "dread".into(), value: VarValue::Int(2) }));
        assert!(log.contains(&Trace::Answer { dialog: "intro".into(), choice: Some("take".into()) }));
        let json = serde_json::to_string(&g.triggers).unwrap();
        assert!(!json.contains("trace"), "the trace is never saved");

        let mut plain = game();
        plain.attach_story(story(&[("dread", VarValue::Int(0))], vec![], vec![intro]));
        plain.answer_story_dialog("intro", Some("take"));
        assert!(plain.triggers.trace.is_none(), "nothing is kept unless a trace is asked for");
    }

    /// A story with the diary's texts: a dialog `gate` (its title and text are keys) and the keys
    /// of a quest `ford`, in the game's language (here Russian).
    fn diary_story() -> Story {
        let mut s = story(&[], vec![], vec![dialog("gate", vec![])]);
        s.dialogs[0].title = "gate.title".into();
        s.dialogs[0].text = "gate.text".into();
        for (k, v) in [("gate.title", "Ворота"), ("gate.text", "Кто идёт?"), ("ford.title", "Удержать брод"), ("ford.text", "Соберите людей."), ("ford.new", "Мёртвые у брода.")] {
            s.texts.insert(k.into(), v.into());
        }
        s
    }

    fn quest(id: &str, text: &str) -> Act {
        Act::Quest { id: id.into(), title: "ford.title".into(), text: text.into() }
    }

    /// Runs `acts` at game minute `minutes`, as a story's actions do.
    fn run_at(g: &mut Game, minutes: f64, s: &Story, acts: &[Act]) {
        g.clock.set_total_minutes(minutes);
        let mut q = VecDeque::new();
        for a in acts {
            g.act(a, s, &mut q);
        }
    }

    #[test]
    fn a_quest_is_added_updated_and_completed_with_its_dates() {
        let mut g = game();
        let s = diary_story();
        g.attach_story(s.clone());
        run_at(&mut g, 100.0, &s, &[quest("ford", "ford.text")]);
        run_at(&mut g, 200.0, &s, &[quest("ford", "ford.new")]);
        let active = g.journal_rows(Tab::Active);
        assert_eq!(active.len(), 1, "an active id is updated, not added again");
        assert_eq!(active[0].title, "Удержать брод", "the texts are the story's, in the game's language");
        assert_eq!(active[0].text, "Мёртвые у брода.");
        assert_eq!(active[0].date, Some(Clock::at_minutes(100)), "the date is when it was added");
        assert_eq!(active[0].elapsed, Some(100), "the time since it was received");
        run_at(&mut g, 300.0, &s, &[Act::CompleteQuest("ford".into()), Act::CompleteQuest("nope".into())]);
        assert!(g.journal_rows(Tab::Active).is_empty(), "completed: out of the active tab");
        let done = g.journal_rows(Tab::Completed);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].date, Some(Clock::at_minutes(300)), "dated when it was completed");
        assert_eq!(done[0].text, "Мёртвые у брода.");
        run_at(&mut g, 310.0, &s, &[Act::CompleteQuest("ford".into())]);
        assert_eq!(g.journal_rows(Tab::Completed).len(), 1, "completing it again, or an unknown id, changes nothing");
        run_at(&mut g, 400.0, &s, &[quest("ford", "ford.text")]);
        run_at(&mut g, 500.0, &s, &[Act::CompleteQuest("ford".into())]);
        let done = g.journal_rows(Tab::Completed);
        assert_eq!(done.iter().map(|r| r.date).collect::<Vec<_>>(), vec![Some(Clock::at_minutes(500)), Some(Clock::at_minutes(300))], "a quest added again is a new entry, newest first");
    }

    #[test]
    fn a_dialog_shown_is_a_message_once_a_minute_and_reread_later() {
        let mut g = game();
        let s = diary_story();
        g.attach_story(s.clone());
        g.clock.set_total_minutes(50.0);
        g.open_dialog("gate", &s);
        g.open_dialog("gate", &s);
        let msgs = g.journal_rows(Tab::Messages);
        assert_eq!(msgs.len(), 1, "the same dialog in the same minute is kept once");
        assert_eq!((msgs[0].title.as_str(), msgs[0].text.as_str()), ("Ворота", "Кто идёт?"));
        g.clock.set_total_minutes(60.0);
        g.open_dialog("gate", &s);
        assert_eq!(g.journal_rows(Tab::Messages).len(), 2, "shown again later: a new message");
    }

    #[test]
    fn a_dialog_choice_adds_a_quest_and_a_campaign_keeps_it_until_updated() {
        let mut g = game();
        let mut d = dialog("gate", vec![choice("take", vec![], vec![quest("ford", "ford.text")])]);
        d.title = "gate.title".into();
        d.text = "gate.text".into();
        let mut s = diary_story();
        s.dialogs = vec![d];
        g.attach_story(s.clone());
        g.clock.set_total_minutes(10.0);
        g.answer_story_dialog("gate", Some("take"));
        assert_eq!(g.journal_rows(Tab::Active).len(), 1, "a choice's quest is added");
        g.journal.next_chapter();
        let active = g.journal_rows(Tab::Active);
        assert_eq!(active.len(), 1, "an active quest stays active on the next map");
        assert_eq!(active[0].chapter, 0, "it still belongs to the map it came from");
        run_at(&mut g, 20.0, &s, &[quest("ford", "ford.new")]);
        let active = g.journal_rows(Tab::Active);
        assert_eq!((active.len(), active[0].chapter), (1, 1), "updated on the next map, it is listed there");
    }

}

#[cfg(test)]
mod wish_tests {
    use super::*;
    use crate::rules::content::{Content, HeroClass};
    use crate::rules::game::Event;

    fn game() -> Game {
        Game::new(Arc::new(Content::builtin()), HeroClass::Knight)
    }

    /// A story with one dialog, so it attaches, and the given variables.
    fn story() -> Story {
        let dialog = DialogDef { id: "d".into(), title: "t".into(), text: "x".into(), speaker: None, choices: vec![] };
        Story { dialogs: vec![dialog], seed: 7, ..Story::default() }
    }

    fn unit(alias: &str) -> UnitId {
        UnitId(aliases::resolve(DtKind::Unit, alias).expect("alias"))
    }

    fn figure_of(g: &Game, army: ArmyId) -> Option<u8> {
        g.world.armies.iter().chain(g.world.inactive.iter()).find(|a| a.id == army).map(|a| a.figure)
    }

    #[test]
    fn a_garrison_takes_spells_items_xp_and_its_named_units() {
        let mut g = game();
        g.attach_story(story());
        let s = g.story.clone().unwrap();
        let mut q = VecDeque::new();
        let b = g.world.locations[0].id;
        let relic = ItemId(aliases::resolve(DtKind::Item, "viking_sword").expect("alias"));
        g.act(&Act::AddUnits(Entity::Building(b), vec![unit("knight")]), &s, &mut q);
        let garrison = g.world.locations[0].garrison.len();
        assert!(garrison >= 1, "the garrison has its troop");

        let spell = aliases::resolve(DtKind::Spell, "fireball").unwrap_or(1);
        let c = Cond::HasSpell { spell, who: Who::Garrison(b), all: true };
        assert!(!g.holds(&c));
        g.act(&Act::Cast { spell, who: Who::Garrison(b), until: Until::Minutes(120) }, &s, &mut q);
        assert!(g.holds(&c), "the garrison's units hold the spell");
        g.act(&Act::RemoveSpells(Who::Garrison(b)), &s, &mut q);
        assert!(!g.holds(&Cond::HasSpell { spell, who: Who::Garrison(b), all: false }), "and it is taken off");

        g.act(&Act::GiveItem(relic, Who::Garrison(b)), &s, &mut q);
        assert!(g.holds(&Cond::HasItem(relic, Who::Garrison(b))), "the item is the garrison's");
        assert!(!g.holds(&Cond::HasItem(relic, Who::Player)) || g.pack.contains(relic), "not given to the hero");

        let before = g.world.locations[0].garrison.iter().map(|t| (t.level, t.xp)).collect::<Vec<_>>();
        g.act(&Act::GiveXp(400, Who::Garrison(b)), &s, &mut q);
        let after = g.world.locations[0].garrison.iter().map(|t| (t.level, t.xp)).collect::<Vec<_>>();
        assert_ne!(before, after, "the garrison's units gain experience");

        g.world.locations[0].garrison.push(Troop { named: 200, ..Troop::new(unit("knight"), 1, Slot::new(Row::Front, 5)) });
        let named = Cond::HasNamed { named: 200, unit: unit("knight"), who: Who::Garrison(b), present: true };
        assert!(g.holds(&named), "a named character of the garrison");
        assert!(!g.holds(&Cond::HasNamed { named: 200, unit: unit("knight"), who: Who::Player, present: true }), "not in the hero's army");
    }

    #[test]
    fn a_named_character_of_an_army_is_found_by_its_army() {
        let mut g = game();
        g.attach_story(story());
        let s = g.story.clone().unwrap();
        let mut q = VecDeque::new();
        g.act(&Act::Spawn { army: 201, at: (3, 3), faction: 4, model: 5, troops: vec![unit("knight")] }, &s, &mut q);
        let i = g.world.armies.iter().position(|a| a.id == 201).expect("spawned");
        g.world.armies[i].troops.push(Troop { named: 201, ..Troop::new(unit("knight"), 1, Slot::new(Row::Front, 5)) });
        assert!(g.holds(&Cond::HasNamed { named: 201, unit: unit("knight"), who: Who::Army(201), present: true }));
        assert!(!g.holds(&Cond::HasNamed { named: 202, unit: unit("knight"), who: Who::Army(201), present: true }));
    }

    #[test]
    fn goals_paces_and_figures_act_and_the_trace_keeps_the_goal_changes() {
        let mut g = game();
        g.attach_story(story());
        g.triggers.trace = Some(Vec::new());
        let s = g.story.clone().unwrap();
        let mut q = VecDeque::new();
        g.act(&Act::Spawn { army: 201, at: (3, 3), faction: 4, model: 5, troops: vec![unit("knight")] }, &s, &mut q);
        g.act(&Act::ArmyGoal { army: 201, goal: Some(Goal::GoTo((9, 9))) }, &s, &mut q);
        let i = g.world.armies.iter().position(|a| a.id == 201).unwrap();
        assert_eq!(g.world.armies[i].mind.goal.as_ref().map(|x| x.goal.clone()), Some(Goal::GoTo((9, 9))));
        g.act(&Act::ArmySpeed { army: 201, percent: 150 }, &s, &mut q);
        assert_eq!(g.world.armies[i].mind.pace, 150);
        g.act(&Act::ArmySpeed { army: 201, percent: 100 }, &s, &mut q);
        assert_eq!(g.world.armies[i].mind.pace, 0, "the normal pace is not stored");
        g.act(&Act::ArmyFigure { army: 201, figure: FigureOf::Unit(unit("knight")) }, &s, &mut q);
        assert_eq!(figure_of(&g, 201), Some(6), "a knight's army shows the knight figure");
        g.act(&Act::HeroFigure(Some(FigureOf::Unit(unit("knight")))), &s, &mut q);
        assert_eq!(g.triggers.hero_sheet, Some(6));
        g.act(&Act::HeroFigure(None), &s, &mut q);
        assert_eq!(g.triggers.hero_sheet, None, "default restores");
        g.act(&Act::ArmyGoal { army: 201, goal: None }, &s, &mut q);
        assert!(g.world.armies[i].mind.goal.is_none());
        let log: Vec<Trace> = g.triggers.trace.clone().unwrap_or_default().into_iter().map(|(_, t)| t).collect();
        assert!(log.contains(&Trace::Goal { army: "201".into(), goal: "go_to [9, 9]".into() }), "{log:?}");
        assert!(log.contains(&Trace::Goal { army: "201".into(), goal: "clear".into() }), "{log:?}");
    }

    #[test]
    fn army_arrives_fires_on_its_cell_and_on_its_building() {
        let mut g = game();
        let b = g.world.locations[0].id;
        let count = |name: &str| Trigger { id: name.into(), when: When::ArmyArrives { army: 201, at: None, building: None }, cond: vec![], then: vec![Act::Add { name: name.into(), value: 1, min: None, max: None }], once: false, chance: 100 };
        let mut on_cell = count("cell");
        on_cell.when = When::ArmyArrives { army: 201, at: Some((9, 9)), building: None };
        let mut in_building = count("house");
        in_building.when = When::ArmyArrives { army: 201, at: None, building: Some(b) };
        let vars = [("cell", VarValue::Int(0)), ("house", VarValue::Int(0))];
        let mut story = story();
        story.vars = vars.iter().map(|(n, v)| (n.to_string(), (VarKind::Int, v.clone()))).collect();
        story.triggers = vec![on_cell, in_building];
        g.attach_story(story);
        g.story_events(When::ArmyArrives { army: 201, at: Some((8, 8)), building: None });
        assert_eq!(g.story_var("cell"), Some(VarValue::Int(0)), "another cell: nothing");
        g.story_events(When::ArmyArrives { army: 201, at: Some((9, 9)), building: None });
        assert_eq!(g.story_var("cell"), Some(VarValue::Int(1)));
        assert_eq!(g.story_var("house"), Some(VarValue::Int(0)), "no building: the house trigger waits");
        let events: Vec<Event> = g.story_events(When::ArmyArrives { army: 201, at: Some((40, 10)), building: Some(b) });
        assert!(events.is_empty());
        assert_eq!(g.story_var("house"), Some(VarValue::Int(1)), "its building");
        assert_eq!(g.story_var("cell"), Some(VarValue::Int(1)), "not its cell");
        g.story_events(When::ArmyArrives { army: 202, at: Some((9, 9)), building: None });
        assert_eq!(g.story_var("cell"), Some(VarValue::Int(1)), "another army: nothing");
    }

    #[test]
    fn a_hero_figure_is_saved_only_with_a_story_in_play() {
        let mut g = game();
        g.attach_story(story());
        g.triggers.hero_sheet = Some(10);
        let json = serde_json::to_string(&g).unwrap();
        let back: Game = serde_json::from_str(&json).unwrap();
        assert_eq!(back.triggers.hero_sheet, Some(10));
        let plain = serde_json::to_string(&game()).unwrap();
        assert!(!plain.contains("\"triggers\""), "no state field while no story is in play");
    }

    #[test]
    fn the_figure_of_a_unit_follows_the_loaders_rule() {
        let c = Content::builtin();
        assert_eq!(Army::figure_of_unit(&c, unit("knight")), 6, "an ordinary unit: the knight figure");
        assert_eq!(Army::figure_of_unit(&c, UnitId(2)), 10, "the archmage's own unit: the mage figure, as the loader gives it");
    }

    #[test]
    fn an_army_model_is_a_figure_of_its_own_picture() {
        assert_eq!(Army::figure_of_model(1), 0, "knight");
        assert_eq!(Army::figure_of_model(5), 4, "bandits: the Rogue");
        assert_eq!(Army::figure_of_model(6), 5, "peasants");
        assert_eq!(Army::figure_of_model(4), 6, "feudal: the Knight");
        assert_eq!(Army::figure_of_model(10), 7, "necromancer");
        assert_eq!(Army::figure_of_model(12), 8, "zombies");
        assert_eq!(Army::figure_of_model(11), 9, "ghosts");
        assert_eq!(FigureOf::Unit(unit("bandit")).index(&Content::builtin()), 6, "a bandit unit draws the knight, as the loader does");
        assert_eq!(FigureOf::Model(5).index(&Content::builtin()), 4);
    }

    #[test]
    fn add_clamps_the_result_to_its_bounds() {
        let mut g = game();
        let s = Story::default();
        let mut q = std::collections::VecDeque::new();
        g.triggers.vars.insert("crown".into(), VarValue::Int(4));
        g.act(&Act::Add { name: "crown".into(), value: 3, min: Some(-5), max: Some(5) }, &s, &mut q);
        assert_eq!(g.var_int("crown"), 5, "clamped at the top");
        g.act(&Act::Add { name: "crown".into(), value: -20, min: Some(-5), max: Some(5) }, &s, &mut q);
        assert_eq!(g.var_int("crown"), -5, "clamped at the bottom");
        g.act(&Act::Add { name: "crown".into(), value: 2, min: None, max: None }, &s, &mut q);
        assert_eq!(g.var_int("crown"), -3, "no bounds: plain");
    }

    #[test]
    fn a_named_character_who_died_is_not_in_the_army_any_more() {
        let mut g = game();
        g.attach_story(story());
        let s = g.story.clone().unwrap();
        let mut q = VecDeque::new();
        g.act(&Act::Spawn { army: 201, at: (3, 3), faction: 4, model: 5, troops: vec![unit("knight")] }, &s, &mut q);
        let i = g.world.armies.iter().position(|a| a.id == 201).expect("spawned");
        g.world.armies[i].troops.push(Troop { named: 201, ..Troop::new(unit("knight"), 1, Slot::new(Row::Front, 5)) });
        let here = Cond::HasNamed { named: 201, unit: unit("knight"), who: Who::Army(201), present: true };
        assert!(g.holds(&here), "alive: in the army");
        g.world.armies[i].troops.iter_mut().find(|t| t.named == 201).expect("the named troop").died_at = Some(1);
        assert!(!g.holds(&here), "dead in battle: not in the army");
        assert!(g.holds(&Cond::HasNamed { named: 201, unit: unit("knight"), who: Who::Army(201), present: false }), "its absence holds");
    }

    #[test]
    fn remove_unit_takes_the_named_character_out_and_his_items_to_the_pack() {
        let mut g = game();
        g.attach_story(story());
        let s = g.story.clone().unwrap();
        let mut q = VecDeque::new();
        g.act(&Act::Spawn { army: 202, at: (3, 3), faction: 4, model: 5, troops: vec![unit("knight")] }, &s, &mut q);
        let i = g.world.armies.iter().position(|a| a.id == 202).expect("spawned");
        let item = ItemId(200);
        let mut worn = [None; crate::rules::items::MAX_SLOTS];
        worn[0] = Some(item);
        g.world.armies[i].troops.push(Troop { named: 200, worn, ..Troop::new(unit("knight"), 1, Slot::new(Row::Front, 5)) });
        assert!(g.holds(&Cond::HasNamed { named: 200, unit: unit("knight"), who: Who::Army(202), present: true }));
        g.act(&Act::RemoveUnit { named: 200, who: Who::Army(202) }, &s, &mut q);
        assert!(!g.world.armies[i].troops.iter().any(|t| t.named == 200), "the named character leaves the army");
        assert!(g.pack.contains(item), "his worn item goes to the pack");
    }

    #[test]
    fn a_name_is_looked_up_by_its_key_then_its_map_then_any_chapter() {
        let mut t = BTreeMap::new();
        t.insert("marka.named.ilm".to_string(), "Brother Ilm".to_string());
        t.insert("camp.named.kord".to_string(), "Kord".to_string());
        assert_eq!(named_text(&t, "ilm", "marka"), "Brother Ilm", "the map's own chapter block");
        assert_eq!(named_text(&t, "ilm", "salt"), "Brother Ilm", "another chapter's block");
        assert_eq!(named_text(&t, "tess", "salt"), "tess", "no name: the id");
        t.insert("named.ilm".to_string(), "Ilm".to_string());
        assert_eq!(named_text(&t, "ilm", "salt"), "Ilm", "the package-wide key first");
    }
}
