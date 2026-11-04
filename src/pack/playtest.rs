//! Package tests (`tests/*.toml`): a playthrough of one of the package's maps, or of its whole campaign
//! from the start, played headless by the engine (`pack::runner`). Each step walks, waits, answers or
//! hires as the hero would, and its `expect` checks the game as it stands. A test passes on at least
//! `min_rate` of its seeds. The format is in `docs/reference/rzpack-format.md` (Tests); `razdor-pack test`
//! and the studio's `run_test` run them. No macroquad.

use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use toml::Spanned;

use super::compile;
use super::diag::Report;
use super::load::{Located, Package};
use super::open::{self, Opened};
use super::runner::{drink_for_wounded, heal_within_means, hire_within_means_keeping, minutes_of, raise_within_means, Budget, Carried, Run, Stuck};
use super::triggers::VarValue;
use crate::dt::install::DtInstall;
use crate::rules::campaign::{after_victory, enter};
use crate::rules::clock::MINUTES_PER_DAY;
use crate::rules::content::Content;
use crate::rules::map::Tile;
use crate::rules::triggers::{named_text, PACKAGE_BASE};

/// The folder of the tests, at the package's top.
pub const DIR: &str = "tests/";

/// How many lines of the run's log a failure shows.
const TAIL: usize = 40;

/// The most seeds one test plays (`seeds`, `razdor-pack test --seeds`, the studio's `run_test`).
pub const MAX_SEEDS: u32 = 200;
/// The most game days one step may ask for (`days`, `wait_days`): a year, as the studio's `sim` allows.
pub const MAX_DAYS: u64 = 365;
/// The most game days one seed plays in all, its steps together.
pub const SEED_GAME_DAYS: u64 = 400;
/// The wall-clock time one seed may take.
pub const SEED_SECONDS: u64 = 60;
/// The wall-clock time one call of the studio's `run_test` may take, all its seeds together.
pub const CALL_SECONDS: u64 = 300;

/// What one seed may spend: a wall-clock deadline and the game days it may play in all. A seed past it
/// fails with a message saying which limit it hit (see [`Budget`]).
#[derive(Clone, Copy, Debug)]
pub struct Limit {
    pub deadline: Instant,
    pub game_days: u64,
    /// Whose deadline it is, for the message.
    pub reason: &'static str,
}

impl Limit {
    /// One seed's own limit from now (`SEED_SECONDS`, `SEED_GAME_DAYS`), or the call's deadline when that
    /// comes first.
    pub fn for_seed(call: Option<Instant>) -> Limit {
        let own = Instant::now() + Duration::from_secs(SEED_SECONDS);
        match call {
            Some(c) if c < own => Limit { deadline: c, game_days: SEED_GAME_DAYS, reason: "test run's time limit" },
            _ => Limit { deadline: own, game_days: SEED_GAME_DAYS, reason: "seed's time budget" },
        }
    }

    /// The run's [`Budget`] when `spent` game days are already played by the seed's earlier maps.
    fn budget(&self, spent: f64) -> Budget {
        Budget { deadline: self.deadline, days: self.game_days as f64 - spent, reason: self.reason }
    }
}

/// The seed count a caller asks for (`--seeds`, the studio's `seeds`), checked against `MAX_SEEDS`.
pub fn seeds_asked(n: u32) -> Result<u32, String> {
    if (1..=MAX_SEEDS).contains(&n) {
        Ok(n)
    } else {
        Err(format!("the seed count must be between 1 and {MAX_SEEDS}, not {n}"))
    }
}

/// The threads a run uses: the cores but two (the desktop's), at least one, and no more than the seeds.
pub fn threads_for(cores: usize, seeds: usize) -> usize {
    cores.saturating_sub(2).max(1).min(seeds.max(1))
}

/// A shell line that replays one seed with `razdor-pack sim` (the story's seed is the same, see
/// `open::story_seed`): a map's test on its map, a campaign's on the campaign.
pub fn replay_of(r: &TestResult, seed: u32) -> String {
    let package = shell_word(&r.package);
    match (&r.map, r.campaign) {
        (Some(map), _) => format!("razdor-pack sim {package} --map {map} --seed {seed}"),
        (None, true) => format!("razdor-pack sim {package} --campaign --seed {seed}"),
        (None, false) => format!("razdor-pack sim {package} --seed {seed}"),
    }
}

/// A path as one shell word: quoted when it has a character a shell would split or expand.
fn shell_word(p: &str) -> String {
    if !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+=:,@%".contains(c)) {
        p.to_string()
    } else {
        format!("'{}'", p.replace('\'', "'\\''"))
    }
}

// ---- the file as read -------------------------------------------------------------------------

/// A test file, as serde reads it: `campaign` or `map`, `seeds`, `min_rate` and the steps.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestFile {
    #[serde(default)]
    pub campaign: bool,
    #[serde(default)]
    pub map: Option<String>,
    #[serde(default)]
    pub seeds: Option<u32>,
    #[serde(default)]
    pub min_rate: Option<f64>,
    /// The option «Обход армий на пути» (the route goes round the armies), on from the first map the
    /// test plays after the start of a campaign (or on the map, when it plays one). Off by default, as the game.
    #[serde(default)]
    pub avoid_armies: bool,
    #[serde(default, rename = "step")]
    pub step: Vec<Spanned<StepSpec>>,
    /// The line of each `[[step]]` table, filled by the reader (`pack::load`).
    #[serde(skip)]
    pub step_lines: Vec<u32>,
}

/// One `[[step]]` table. One action at most; `expect` may come with it or stand alone.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepSpec {
    /// A cell `[x, y]`: the hero walks there by the game's route.
    pub walk: Option<[i32; 2]>,
    /// The most days a walk (or a fight) may take. Default 10.
    pub days: Option<u64>,
    /// A building of the map, by its id: the hero walks to its cell.
    pub walk_to_building: Option<String>,
    /// An army of the map, by its id: the hero walks into it where it stands (a greeting or a battle).
    pub walk_to_army: Option<String>,
    /// The hero stands still for this many days (the chapter's end stops it early).
    pub wait_days: Option<u64>,
    /// A dialog's choice, taken when the dialog next opens (until another `choose` for it).
    pub choose: Option<ChooseSpec>,
    /// Hires what the town offers while the purse holds three days of wages: `true`, or a number of
    /// gold to keep back.
    pub hire_all: Option<Hire>,
    /// An army of the map, by its id: the hero walks into it and the battle is fought (auto-resolved).
    pub fight: Option<String>,
    /// At a town: heals the wounded and raises the fallen, while the purse holds three days of wages.
    pub service: Option<bool>,
    /// Drinks a potion of the pack for each wounded unit.
    pub drink: Option<bool>,
    /// A campaign: the map is won, and the hero goes on to its next map (the victory's route).
    pub leave: Option<bool>,
    pub expect: Option<ExpectSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChooseSpec {
    pub dialog: String,
    pub choice: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Hire {
    Flag(bool),
    Keep(i64),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    Bool(bool),
    Int(i64),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum BattleSpec {
    Flag(bool),
    Enemy(String),
}

/// The `expect` table: every key is one check; `op` compares `var`, `squad_size` and `gold`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectSpec {
    pub var: Option<String>,
    pub op: Option<String>,
    pub value: Option<Scalar>,
    pub squad_size: Option<i64>,
    pub gold: Option<i64>,
    pub map: Option<String>,
    pub ending: Option<String>,
    /// A dialog opened so far in the test: its id, or `id -> choice` for the choice it took.
    pub dialog_seen: Option<String>,
    /// On the current map: `true` (a battle was fought), `false` (none), or an enemy's id (a battle with it).
    pub battle: Option<BattleSpec>,
    /// The longest walk between two beats (a dialog, a capture, an encounter) on the current map, in cells.
    pub beat_gap: Option<u64>,
    /// A named character of the package is in the hero's squad and alive.
    pub alive: Option<String>,
}

// ---- the test as the engine runs it -----------------------------------------------------------

/// A test: its name (the file's stem), where it starts, its seeds and the rate it must meet.
#[derive(Debug, Clone)]
pub struct Test {
    pub name: String,
    pub file: String,
    /// Plays the campaign from its start (`campaign.toml`), handing over on each victory.
    pub campaign: bool,
    /// Plays this one map (when not a campaign).
    pub map: Option<String>,
    pub seeds: u32,
    pub min_rate: f64,
    pub avoid_armies: bool,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone)]
pub struct Step {
    /// The line of the step's table in its file.
    pub line: u32,
    pub act: Option<Act>,
    pub expect: Vec<Check>,
    /// The text of the step as the author wrote it, for a failure.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    Walk { to: Tile, days: u64 },
    Building { id: String, days: u64 },
    Army { id: String, days: u64 },
    Fight { army: String, days: u64 },
    Wait(u64),
    Choose { dialog: String, choice: String },
    Hire(i32),
    Service,
    Drink,
    Leave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Ge,
    Le,
    Gt,
    Lt,
}

impl Op {
    fn parse(text: &str) -> Option<Op> {
        Some(match text {
            "==" => Op::Eq,
            "!=" => Op::Ne,
            ">=" => Op::Ge,
            "<=" => Op::Le,
            ">" => Op::Gt,
            "<" => Op::Lt,
            _ => return None,
        })
    }

    fn holds<T: PartialOrd>(self, a: T, b: T) -> bool {
        match self {
            Op::Eq => a == b,
            Op::Ne => a != b,
            Op::Ge => a >= b,
            Op::Le => a <= b,
            Op::Gt => a > b,
            Op::Lt => a < b,
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::Ge => ">=",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Lt => "<",
        }
    }
}

/// One check of an `expect`.
#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    Var { name: String, op: Op, value: VarValue },
    SquadSize(Op, i64),
    Gold(Op, i64),
    Map(String),
    Ending(String),
    DialogSeen { id: String, choice: Option<String> },
    Battle(Option<String>, bool),
    BeatGap(u64),
    Alive(String),
}

impl Check {
    fn describe(&self) -> String {
        match self {
            Check::Var { name, op, value } => format!("var {name} {} {}", op.text(), show(value)),
            Check::SquadSize(op, n) => format!("squad_size {} {n}", op.text()),
            Check::Gold(op, n) => format!("gold {} {n}", op.text()),
            Check::Map(m) => format!("map = \"{m}\""),
            Check::Ending(e) => format!("ending = \"{e}\""),
            Check::DialogSeen { id, choice: None } => format!("dialog_seen = \"{id}\""),
            Check::DialogSeen { id, choice: Some(c) } => format!("dialog_seen = \"{id} -> {c}\""),
            Check::Battle(None, true) => "battle = true".into(),
            Check::Battle(None, false) => "battle = false".into(),
            Check::Battle(Some(enemy), _) => format!("battle = \"{enemy}\""),
            Check::BeatGap(n) => format!("beat_gap = {n}"),
            Check::Alive(name) => format!("alive = \"{name}\""),
        }
    }
}

fn show(v: &VarValue) -> String {
    match v {
        VarValue::Int(i) => i.to_string(),
        VarValue::Bool(b) => b.to_string(),
    }
}

// ---- reading ----------------------------------------------------------------------------------

/// A problem with a test file: where it is, and what is wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub file: String,
    pub line: Option<u32>,
    pub message: String,
    pub hint: String,
}

/// The name of a test from its package path (`tests/marsh_route.toml` is `marsh_route`), when it is a test.
pub fn name_of(path: &str) -> Option<&str> {
    path.strip_prefix(DIR)?.strip_suffix(".toml").filter(|n| !n.is_empty() && !n.contains('/'))
}

/// Reads one test file into a [`Test`], or the problems that stop it.
pub fn read(file: &Located<TestFile>) -> Result<Test, Vec<Problem>> {
    let name = name_of(&file.file).unwrap_or_default().to_string();
    let t = &file.value;
    let mut problems = Vec::new();
    let mut problem = |line: Option<u32>, message: String, hint: &str| problems.push(Problem { file: file.file.clone(), line, message, hint: hint.to_string() });
    if t.campaign && t.map.is_some() {
        problem(Some(file.line), "a test plays a campaign or a map, not both".into(), "remove `map`, or set `campaign = false`");
    }
    if !t.campaign && t.map.is_none() {
        problem(Some(file.line), "a test needs `campaign = true` or `map = \"<id>\"`".into(), "name the map to play, or play the campaign from its start");
    }
    let seeds = t.seeds.unwrap_or(1);
    if seeds == 0 {
        problem(Some(file.line), "seeds is 0: a test runs at least one seed".into(), "seeds = 15");
    } else if seeds > MAX_SEEDS {
        problem(Some(file.line), format!("seeds is {seeds}: a test plays at most {MAX_SEEDS} seeds"), &format!("seeds = {MAX_SEEDS} or fewer"));
    }
    let min_rate = t.min_rate.unwrap_or(1.0);
    if !(min_rate > 0.0 && min_rate <= 1.0) {
        problem(Some(file.line), format!("min_rate is {min_rate}: it is a share from above 0 to 1"), "min_rate = 0.7");
    }
    let mut steps = Vec::new();
    for (i, s) in t.step.iter().enumerate() {
        let line = t.step_lines.get(i).copied().unwrap_or(file.line);
        match read_step(s.get_ref(), line) {
            Ok(step) => steps.push(step),
            Err(list) => {
                for (message, hint) in list {
                    problem(Some(line), message, &hint);
                }
            }
        }
    }
    if t.step.is_empty() {
        problem(Some(file.line), "the test has no steps".into(), "add a [[step]] with a walk, a wait or an expect");
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(Test { name, file: file.file.clone(), campaign: t.campaign, map: t.map.clone(), seeds, min_rate, avoid_armies: t.avoid_armies, steps })
}

/// One step: its one action and its expectations, or the problems with it.
fn read_step(s: &StepSpec, line: u32) -> Result<Step, Vec<(String, String)>> {
    let mut problems: Vec<(String, String)> = Vec::new();
    let mut acts: Vec<Act> = Vec::new();
    let days = s.days.unwrap_or(10);
    let walking = s.walk.is_some() || s.walk_to_building.is_some() || s.walk_to_army.is_some() || s.fight.is_some();
    if s.days.is_some() && !walking {
        problems.push(("`days` goes with a walk, walk_to_building, walk_to_army or fight".into(), "remove `days`, or add the walk".into()));
    }
    if s.days == Some(0) {
        problems.push(("days is 0".into(), "days = 1 or more".into()));
    }
    if days > MAX_DAYS {
        problems.push((format!("days is {days}: a step plays at most {MAX_DAYS} days"), format!("days = {MAX_DAYS} or fewer")));
    }
    if let Some([x, y]) = s.walk {
        if x < 0 || y < 0 {
            problems.push((format!("walk [{x}, {y}]: a cell has no negative coordinate"), "walk = [x, y] on the map".into()));
        } else {
            acts.push(Act::Walk { to: (x, y), days });
        }
    }
    if let Some(id) = &s.walk_to_building {
        acts.push(Act::Building { id: id.clone(), days });
    }
    if let Some(id) = &s.walk_to_army {
        acts.push(Act::Army { id: id.clone(), days });
    }
    if let Some(army) = &s.fight {
        acts.push(Act::Fight { army: army.clone(), days });
    }
    if let Some(n) = s.wait_days {
        if n == 0 {
            problems.push(("wait_days is 0".into(), "wait_days = 1 or more".into()));
        } else if n > MAX_DAYS {
            problems.push((format!("wait_days is {n}: a step waits at most {MAX_DAYS} days"), format!("wait_days = {MAX_DAYS} or fewer")));
        }
        acts.push(Act::Wait(n));
    }
    if let Some(c) = &s.choose {
        acts.push(Act::Choose { dialog: c.dialog.clone(), choice: c.choice.clone() });
    }
    match &s.hire_all {
        None => {}
        Some(Hire::Flag(true)) => acts.push(Act::Hire(0)),
        Some(Hire::Flag(false)) => problems.push(("hire_all = false does nothing".into(), "remove the step".into())),
        Some(Hire::Keep(k)) if *k < 0 || *k > i32::MAX as i64 => problems.push((format!("hire_all keeps {k} gold: not a sum"), "hire_all = 40, or true".into())),
        Some(Hire::Keep(k)) => acts.push(Act::Hire(*k as i32)),
    }
    match s.service {
        Some(true) => acts.push(Act::Service),
        Some(false) => problems.push(("service = false does nothing".into(), "remove the step".into())),
        None => {}
    }
    match s.drink {
        Some(true) => acts.push(Act::Drink),
        Some(false) => problems.push(("drink = false does nothing".into(), "remove the step".into())),
        None => {}
    }
    match s.leave {
        Some(true) => acts.push(Act::Leave),
        Some(false) => problems.push(("leave = false does nothing".into(), "remove the step".into())),
        None => {}
    }
    if acts.len() > 1 {
        problems.push((format!("a step has one action, not {}", acts.len()), "split it into steps".into()));
    }
    let expect = match &s.expect {
        Some(e) => match read_expect(e) {
            Ok(checks) => checks,
            Err(list) => {
                problems.extend(list);
                Vec::new()
            }
        },
        None => Vec::new(),
    };
    if acts.is_empty() && s.expect.is_none() && problems.is_empty() {
        problems.push(("the step does nothing: it has no action and no expect".into(), "add a walk, a wait, a choose, or an expect".into()));
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(Step { line, act: acts.pop(), expect, label: label_of(s) })
}

/// The step's action as it reads, for a failure's message.
fn label_of(s: &StepSpec) -> String {
    if let Some([x, y]) = s.walk {
        return format!("walk [{x}, {y}]");
    }
    if let Some(id) = &s.walk_to_building {
        return format!("walk_to_building \"{id}\"");
    }
    if let Some(id) = &s.walk_to_army {
        return format!("walk_to_army \"{id}\"");
    }
    if let Some(a) = &s.fight {
        return format!("fight \"{a}\"");
    }
    if let Some(n) = s.wait_days {
        return format!("wait_days {n}");
    }
    if let Some(c) = &s.choose {
        return format!("choose {} = \"{}\"", c.dialog, c.choice);
    }
    if s.hire_all.is_some() {
        return "hire_all".into();
    }
    if s.service == Some(true) {
        return "service".into();
    }
    if s.drink == Some(true) {
        return "drink".into();
    }
    if s.leave == Some(true) {
        return "leave: the hero goes on to the next map".into();
    }
    "expect".into()
}

/// The `expect` table as checks, or the problems with it.
fn read_expect(e: &ExpectSpec) -> Result<Vec<Check>, Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    let op = match e.op.as_deref() {
        None => Op::Eq,
        Some(text) => match Op::parse(text) {
            Some(op) => op,
            None => {
                problems.push((format!("op `{text}` is not a comparison"), "op is ==, !=, >=, <=, > or <".into()));
                Op::Eq
            }
        },
    };
    let numeric_target = e.var.is_some() || e.squad_size.is_some() || e.gold.is_some();
    if e.op.is_some() && !numeric_target {
        problems.push(("`op` goes with var, squad_size or gold".into(), "remove `op`, or name what it compares".into()));
    }
    match (&e.var, &e.value) {
        (Some(name), Some(value)) => {
            let value = match value {
                Scalar::Int(i) => VarValue::Int(*i),
                Scalar::Bool(b) => VarValue::Bool(*b),
            };
            if matches!(value, VarValue::Bool(_)) && !matches!(op, Op::Eq | Op::Ne) {
                problems.push((format!("var {name} is a flag: compare it with == or !="), "value = true, with op = \"==\"".into()));
            }
            out.push(Check::Var { name: name.clone(), op, value });
        }
        (Some(name), None) => problems.push((format!("expect var = \"{name}\" needs a value"), "value = 3, with op = \">=\" say".into())),
        (None, Some(_)) => problems.push(("expect value needs a var".into(), "var = \"<name>\" with the value".into())),
        (None, None) => {}
    }
    if let Some(n) = e.squad_size {
        out.push(Check::SquadSize(op, n));
    }
    if let Some(n) = e.gold {
        out.push(Check::Gold(op, n));
    }
    if let Some(m) = &e.map {
        out.push(Check::Map(m.clone()));
    }
    if let Some(end) = &e.ending {
        out.push(Check::Ending(end.clone()));
    }
    if let Some(seen) = &e.dialog_seen {
        let (id, choice) = match seen.split_once(" -> ") {
            Some((id, c)) => (id.trim().to_string(), Some(c.trim().to_string())),
            None => (seen.trim().to_string(), None),
        };
        out.push(Check::DialogSeen { id, choice });
    }
    match &e.battle {
        None => {}
        Some(BattleSpec::Flag(b)) => out.push(Check::Battle(None, *b)),
        Some(BattleSpec::Enemy(name)) => out.push(Check::Battle(Some(name.clone()), true)),
    }
    if let Some(n) = e.beat_gap {
        if n == 0 {
            problems.push(("beat_gap is 0".into(), "beat_gap = 8, cells".into()));
        }
        out.push(Check::BeatGap(n));
    }
    if let Some(name) = &e.alive {
        out.push(Check::Alive(name.clone()));
    }
    if out.is_empty() && problems.is_empty() {
        problems.push(("expect checks nothing".into(), "name a check: var, squad_size, gold, map, ending, dialog_seen, battle, beat_gap or alive".into()));
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

/// Every test of the package that reads, in name order.
pub fn tests(pkg: &Package) -> Vec<Test> {
    pkg.tests.iter().filter_map(|f| read(f).ok()).collect()
}

// ---- checking ---------------------------------------------------------------------------------

/// The rules of the test files (`check`): each file reads, and every name it uses exists: the map, the
/// dialogs, the choices, the buildings and armies of the maps it plays, the variables and the endings.
pub fn check(pkg: &Package, r: &mut Report) {
    for f in &pkg.tests {
        match read(f) {
            Err(problems) => {
                for p in problems {
                    r.error_hint(&p.file, p.line, p.message, p.hint);
                }
            }
            Ok(t) => check_test(pkg, f, &t, r),
        }
    }
}

fn check_test(pkg: &Package, f: &Located<TestFile>, t: &Test, r: &mut Report) {
    let maps = maps_of(pkg, t);
    if t.campaign && pkg.campaign.is_none() {
        r.error_hint(&f.file, Some(f.line), "the test plays the campaign, and the package has no campaign.toml", "set `map = \"<id>\"` to play one map, or add campaign.toml");
    }
    if let Some(m) = &t.map {
        if !pkg.maps.contains_key(m) {
            r.error_hint(&f.file, Some(f.line), format!("the package has no map `{m}`"), "maps are maps/<id>/map.toml");
        }
    }
    let dialogs: BTreeMap<&str, &super::triggers::Dialog> = pkg.dialogs.iter().map(|d| (d.value.id.as_str(), &d.value)).collect();
    let vars: Vec<&str> = pkg.vars.iter().map(|v| v.value.name.as_str()).collect();
    let line_of_step = |s: &Step| Some(s.line);
    for s in &t.steps {
        let line = line_of_step(s);
        match &s.act {
            Some(Act::Building { id, .. }) => {
                if !maps.iter().any(|m| buildings(pkg, m).contains(id)) {
                    r.error_hint(&f.file, line, format!("no building `{id}` on {}", maps_text(&maps)), format!("buildings are the [[building]] ids of the map: {}", list(maps.iter().flat_map(|m| buildings(pkg, m)).collect())));
                }
            }
            Some(Act::Army { id, .. }) | Some(Act::Fight { army: id, .. }) => {
                if !maps.iter().any(|m| armies(pkg, m).contains(id)) {
                    r.error_hint(&f.file, line, format!("no army `{id}` on {}", maps_text(&maps)), format!("armies are the [[army]] ids of the map: {}", list(maps.iter().flat_map(|m| armies(pkg, m)).collect())));
                }
            }
            Some(Act::Choose { dialog, choice }) => match dialogs.get(dialog.as_str()) {
                None => r.error_hint(&f.file, line, format!("no dialog `{dialog}`"), "a dialog is a [[dialog]] id in dialogs/*.toml"),
                Some(d) if !d.choices.is_empty() && !d.choices.iter().any(|c| &c.id == choice) => {
                    let offered: Vec<&str> = d.choices.iter().map(|c| c.id.as_str()).collect();
                    r.error_hint(&f.file, line, format!("dialog `{dialog}` has no choice `{choice}`"), format!("its choices: {}", offered.join(", ")));
                }
                Some(_) => {}
            },
            _ => {}
        }
        for c in &s.expect {
            check_expect(pkg, c, &maps, &dialogs, &vars, &f.file, line, r);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_expect(pkg: &Package, c: &Check, maps: &[String], dialogs: &BTreeMap<&str, &super::triggers::Dialog>, vars: &[&str], file: &str, line: Option<u32>, r: &mut Report) {
    match c {
        Check::Var { name, .. } if !vars.contains(&name.as_str()) => {
            r.error_hint(file, line, format!("no variable `{name}`"), "declare it in [vars] of a triggers/*.toml file");
        }
        Check::Map(m) if !pkg.maps.contains_key(m) => {
            r.error_hint(file, line, format!("the package has no map `{m}`"), "maps are maps/<id>/map.toml");
        }
        Check::Ending(e) if !dialogs.contains_key(e.as_str()) => {
            r.error_hint(file, line, format!("no ending dialog `{e}`"), "an ending is the id of its dialog, e.g. end_free");
        }
        Check::DialogSeen { id, .. } if !dialogs.contains_key(id.as_str()) => {
            r.error_hint(file, line, format!("no dialog `{id}`"), "a dialog is a [[dialog]] id in dialogs/*.toml");
        }
        Check::Battle(Some(enemy), _) if !maps.iter().any(|m| armies(pkg, m).contains(enemy) || buildings(pkg, m).contains(enemy)) => {
            r.error_hint(file, line, format!("no army or building `{enemy}` on {}", maps_text(maps)), "a battle is named by the army's or the building's id");
        }
        _ => {}
    }
}

fn maps_of(pkg: &Package, t: &Test) -> Vec<String> {
    if t.campaign {
        pkg.campaign.as_ref().map(|c| c.maps.iter().map(|m| m.id.clone()).collect()).unwrap_or_default()
    } else {
        t.map.iter().cloned().collect()
    }
}

fn maps_text(maps: &[String]) -> String {
    match maps {
        [one] => format!("map `{one}`"),
        _ => "the campaign's maps".into(),
    }
}

fn list(items: Vec<String>) -> String {
    let mut items = items;
    items.sort();
    items.dedup();
    if items.is_empty() {
        "none".into()
    } else {
        items.join(", ")
    }
}

fn buildings(pkg: &Package, map: &str) -> Vec<String> {
    pkg.maps.get(map).and_then(|m| m.spec.as_ref()).map(|s| s.building.iter().map(|b| b.get_ref().id.clone()).collect()).unwrap_or_default()
}

/// The armies of a map: its own `[[army]]` ids, and the armies the package's triggers spawn (`spawn_army`),
/// which every map knows (`triggers::check_map_refs`).
fn armies(pkg: &Package, map: &str) -> Vec<String> {
    let own = pkg.maps.get(map).and_then(|m| m.spec.as_ref()).map(|s| s.army.iter().map(|a| a.get_ref().id.clone()).collect::<Vec<_>>());
    let spawned = pkg
        .triggers
        .iter()
        .flat_map(|t| t.value.then.iter())
        .chain(pkg.dialogs.iter().flat_map(|d| d.value.choices.iter()).flat_map(|c| c.then.iter()))
        .filter_map(|a| match a {
            super::triggers::Action::SpawnArmy { army, .. } => Some(army.clone()),
            _ => None,
        });
    own.unwrap_or_default().into_iter().chain(spawned).collect()
}

// ---- running ----------------------------------------------------------------------------------

/// The maps a run needs, compiled on the install once: each map's opened game and its content.
pub struct Maps {
    pub by_id: BTreeMap<String, (Opened, Arc<Content>)>,
}

/// Compiles the maps `ids` of `pkg` (read from `root`) on `install`, in the English texts.
pub fn open_maps(pkg: &Package, root: &Path, install: &DtInstall, ids: &[String]) -> Result<Maps, String> {
    let hash = open::package_hash(&pkg.files);
    let mut by_id = BTreeMap::new();
    for id in ids {
        if by_id.contains_key(id) {
            continue;
        }
        let compiled = compile::compile(pkg, install, "en", id).map_err(|r| r.to_text())?;
        let opened = open::build(pkg, hash, root, id, compiled, "en")?;
        let content = Arc::new(Content::from_dt(install).with_package(opened.units.clone(), opened.items.clone()));
        by_id.insert(id.clone(), (opened, content));
    }
    Ok(Maps { by_id })
}

/// One seed of a test, played: where it failed (if it did), the maps it went through and the log.
#[derive(Debug, Clone)]
pub struct SeedRun {
    pub seed: u32,
    pub failure: Option<Failure>,
    pub maps: Vec<String>,
    pub ending: Option<String>,
    pub log: Vec<String>,
    pub secs: f64,
}

/// The step that failed: its number (from 1), its line in the file, what went wrong and the log's tail.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub step: usize,
    pub line: u32,
    pub label: String,
    pub what: String,
    pub tail: Vec<String>,
}

/// The state of one seed's playthrough: the run on the current map, and what came before it.
struct Session<'a> {
    pkg: &'a Package,
    maps: &'a Maps,
    campaign: bool,
    map: String,
    run: Run,
    /// The choices the script has made so far, in order: a campaign's later maps are answered by them.
    choices: Vec<(String, String)>,
    /// The maps played so far, in order.
    played: Vec<String>,
    /// The log of the maps already left (the current run's log is added on top of it).
    log: Vec<String>,
    /// The option «Обход армий на пути» for the maps after the first (`TestFile::avoid_armies`).
    avoid: bool,
    /// The seed: each map of the run is opened for it (`open::Opened::with_run`), as `razdor-pack sim --seed`.
    seed: u32,
    /// The game days the maps left behind have played (the current run counts its own from its start).
    spent: f64,
    limit: Limit,
}

impl Session<'_> {
    fn full_log(&self) -> Vec<String> {
        let mut all = self.log.clone();
        all.extend(self.run.log.iter().cloned());
        all
    }

    /// The campaign's hand-over: the map is won (`done` and a victory), and the campaign's victory route
    /// leads on. The next map is entered as the campaign does (`rules::campaign::enter`), with the choices
    /// made so far. A revisit's map is done when it is entered, so the hand-over is a step of its own
    /// (`leave`), not a reaction to the win.
    fn leave(&mut self) -> Result<(), String> {
        if !self.campaign {
            return Err("`leave` goes with a campaign: a test of one map has no next map".into());
        }
        if !(self.run.done() && self.run.victory()) {
            return Err(format!("the map `{}` is not won: the hero cannot leave it yet", self.map));
        }
        let maps = self.maps;
        let from = &maps.by_id.get(&self.map).ok_or_else(|| format!("the map `{}` is not opened", self.map))?.0;
        let Some(next) = after_victory(&mut self.run.g, &from.routes) else {
            return Err(format!("the map `{}` has no way on: the campaign ends here", self.map));
        };
        let (to, content) = maps.by_id.get(&next).ok_or_else(|| format!("the campaign goes to `{next}`, which the package does not have"))?;
        // The next map's story is seeded for this run, as the first map's is (`Run::carried`).
        let to = to.clone().with_run(self.seed);
        let g = enter(&self.run.g, from, &to, None, content.clone()).map_err(|e| format!("the hand-over to {next}: {e}"))?;
        let refs: Vec<(&str, &str)> = self.choices.iter().map(|(d, c)| (d.as_str(), c.as_str())).collect();
        let mut run = Run::resume(g, &refs).map_err(|e| format!("entering {next}: {e}"))?;
        run.g.avoid_armies = self.avoid;
        self.spent += self.run.day();
        run.budget = Some(self.limit.budget(self.spent));
        let old = std::mem::replace(&mut self.run, run);
        self.log.extend(old.log);
        self.log.push(format!("-- entering {next}"));
        self.map = next.clone();
        self.played.push(next);
        Ok(())
    }

    /// Plays one action on the current run.
    fn act(&mut self, act: &Act) -> Result<(), String> {
        match act {
            Act::Walk { to, days } => self.walk(*to, *days).map_err(|e| format!("walk to {to:?}: {e}")),
            Act::Building { id, days } => {
                let cell = self.building_cell(id)?;
                self.walk(cell, *days).map_err(|e| format!("walk to building {id} at {cell:?}: {e}"))
            }
            Act::Army { id, days } => self.walk_into_army(id, *days),
            Act::Fight { army, days } => self.fight_army(army, *days),
            Act::Wait(days) => self.run.wait_days(*days).map_err(|e| format!("wait {days} days: {e}")),
            Act::Choose { dialog, choice } => {
                self.run.choices.insert(dialog.clone(), choice.clone());
                self.choices.retain(|(d, _)| d != dialog);
                self.choices.push((dialog.clone(), choice.clone()));
                Ok(())
            }
            Act::Hire(keep) => {
                hire_within_means_keeping(&mut self.run, *keep);
                Ok(())
            }
            Act::Service => {
                heal_within_means(&mut self.run);
                raise_within_means(&mut self.run);
                Ok(())
            }
            Act::Drink => {
                drink_for_wounded(&mut self.run);
                Ok(())
            }
            Act::Leave => self.leave(),
        }
    }

    /// Walks the hero to `to` within `days`: a battle, a dialog or a teleport on the way ends the engine's walk
    /// (`Run::walk_to`), and the hero walks on from where he stands until he is there, the chapter is over,
    /// or the days are gone.
    fn walk(&mut self, to: Tile, days: u64) -> Result<(), String> {
        let deadline = self.run.g.clock.total_minutes() + minutes_of(days).map_err(|e| e.to_string())?;
        for _ in 0..1000 {
            if self.run.g.tile() == to || self.run.done() {
                return Ok(());
            }
            let now = self.run.g.clock.total_minutes();
            let left = ((deadline - now) / MINUTES_PER_DAY as f64).ceil().max(0.0) as u64;
            if left == 0 {
                return Err(format!("the hero did not reach {to:?} by day {:.1}", self.run.day()));
            }
            // An event that ends the engine's walk at once (a battle at the start, a dialog) leaves the clock as it
            // was: the loop walks on from there, at most a thousand times.
            self.run.walk_to(to, left).map_err(|e| e.to_string())?;
        }
        Err(format!("the walk to {to:?} did not end (the hero is at {:?})", self.run.g.tile()))
    }

    fn building_cell(&self, id: &str) -> Result<Tile, String> {
        let spec = self.pkg.maps.get(&self.map).and_then(|m| m.spec.as_ref());
        let b = spec.and_then(|s| s.building.iter().find(|b| b.get_ref().id == id));
        match b {
            Some(b) => {
                let [x, y] = b.get_ref().cell;
                Ok((x as i32, y as i32))
            }
            None => Err(format!("no building `{id}` on map `{}`", self.map)),
        }
    }

    /// The cell of the army `name` (its package id) where it stands now, if it is on the map.
    fn army_cell(&self, name: &str) -> Option<Tile> {
        let story = self.run.g.story()?;
        let (&id, _) = story.army_names.iter().find(|(_, n)| n.as_str() == name)?;
        self.run.g.world.armies.iter().find(|a| a.id == id).map(|a| a.tile(&self.run.g.world.map))
    }

    /// Walks into the army where it stands, a few times (it moves): a greeting or a battle ends the walk.
    fn walk_into_army(&mut self, name: &str, days: u64) -> Result<(), String> {
        for _ in 0..8 {
            let (beats, battles) = (self.run.beats.len(), self.run.battles.len());
            let Some(cell) = self.army_cell(name) else { return Err(format!("no army `{name}` stands on the map now")) };
            match self.run.walk_to(cell, days) {
                Ok(()) | Err(Stuck::Late { .. }) => {}
                Err(e) => return Err(format!("walk into {name}: {e}")),
            }
            if self.run.beats.len() != beats || self.run.battles.len() != battles {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Walks into the army `name` until its battle is fought (it moves, so the walk is repeated a day at a time).
    fn fight_army(&mut self, name: &str, days: u64) -> Result<(), String> {
        let before = self.run.battles.len();
        for _ in 0..days {
            if let Some(cell) = self.army_cell(name) {
                match self.run.walk_to(cell, 1) {
                    Ok(()) | Err(Stuck::Late { .. }) | Err(Stuck::NoRoute { .. }) => {}
                    Err(e) => return Err(format!("fight {name}: {e}")),
                }
            }
            if self.run.battles[before..].iter().any(|b| b.enemy == name) {
                return Ok(());
            }
            self.run.wait_days(1).map_err(|e| format!("fight {name}: {e}"))?;
            if self.run.done() {
                break;
            }
        }
        Err(format!("no battle with `{name}` came in {days} days"))
    }

    /// Whether `check` holds now; the message says what it is and what it is.
    fn check(&self, c: &Check) -> Result<(), String> {
        let run = &self.run;
        let fail = |got: String| Err(format!("expect {}: {got}", c.describe()));
        match c {
            Check::Var { name, op, value } => match run.var(name) {
                None => Err(format!("expect {}: the variable `{name}` is not declared", c.describe())),
                Some(got) => {
                    let ok = match (&got, value) {
                        (VarValue::Int(a), VarValue::Int(b)) => op.holds(*a, *b),
                        (VarValue::Bool(a), VarValue::Bool(b)) => op.holds(*a, *b),
                        _ => return fail(format!("{name} is {}, a different kind of value", show(&got))),
                    };
                    if ok {
                        Ok(())
                    } else {
                        fail(format!("{name} is {}", show(&got)))
                    }
                }
            },
            Check::SquadSize(op, n) => {
                let size = run.g.squad.iter().filter(|u| u.alive()).count() as i64;
                if op.holds(size, *n) {
                    Ok(())
                } else {
                    fail(format!("the squad has {size}"))
                }
            }
            Check::Gold(op, n) => {
                let gold = run.g.gold as i64;
                if op.holds(gold, *n) {
                    Ok(())
                } else {
                    fail(format!("the gold is {gold}"))
                }
            }
            Check::Map(m) => {
                if &self.map == m {
                    Ok(())
                } else {
                    fail(format!("the hero is on `{}`", self.map))
                }
            }
            Check::Ending(e) => {
                if run.ending().as_deref() == Some(e.as_str()) {
                    Ok(())
                } else {
                    fail(match run.ending() {
                        Some(got) => format!("the ending is {got}"),
                        None => "no ending has come".into(),
                    })
                }
            }
            Check::DialogSeen { id, choice } => {
                let want = match choice {
                    Some(c) => format!("dialog {id} -> {c}"),
                    None => format!("dialog {id} -> "),
                };
                let seen = self.full_log().iter().any(|l| match choice {
                    Some(_) => l.ends_with(&want),
                    None => l.contains(&want),
                });
                if seen {
                    Ok(())
                } else {
                    fail("the dialog did not open".into())
                }
            }
            Check::Battle(enemy, want) => {
                let ok = match enemy {
                    Some(e) => run.battles.iter().any(|b| &b.enemy == e),
                    None => run.battles.is_empty() != *want,
                };
                if ok {
                    Ok(())
                } else {
                    fail(format!("the battles on this map: {:?}", run.battles.iter().map(|b| (b.enemy.clone(), b.outcome)).collect::<Vec<_>>()))
                }
            }
            Check::BeatGap(limit) => {
                let gap = longest_gap(run);
                if gap as u64 <= *limit {
                    Ok(())
                } else {
                    fail(format!("the longest walk between beats is {gap} cells"))
                }
            }
            Check::Alive(name) => {
                let k = self.named_index(name)?;
                let alive = run.g.squad.iter().any(|u| u.alive() && u.named == PACKAGE_BASE + k);
                if alive {
                    Ok(())
                } else {
                    fail(format!("{name} is not in the squad alive"))
                }
            }
        }
    }

    /// The position of the named character `name` in the story's list (its id is `PACKAGE_BASE + k`).
    fn named_index(&self, name: &str) -> Result<u8, String> {
        let (opened, _) = self.maps.by_id.get(&self.map).ok_or_else(|| format!("the map `{}` is not opened", self.map))?;
        let text = named_text(&opened.story.texts, name, &opened.map);
        opened.story.named.iter().position(|n| *n == text).map(|k| k as u8).ok_or_else(|| format!("no named character `{name}` on map `{}`", self.map))
    }
}

/// The longest walk between two beats (or the ends of the path), in cells (`Run::path`, `Run::beats`).
fn longest_gap(run: &Run) -> usize {
    longest_gap_of(run.path.len().saturating_sub(1), &run.beats)
}

/// [`longest_gap`] for a path of `last` steps (its last index) and the beats' indices in it.
fn longest_gap_of(last: usize, beats: &[usize]) -> usize {
    let mut marks = vec![0usize];
    marks.extend(beats.iter().copied());
    marks.push(last);
    marks.sort_unstable();
    marks.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0)
}

/// Plays one seed of `test`: the first map (the campaign's start, or the test's map) with the leading
/// `choose` steps answered at its start, then the steps in order, the expectations checked after each.
pub fn play_seed(pkg: &Package, maps: &Maps, test: &Test, seed: u32, limit: Limit) -> SeedRun {
    let started = Instant::now();
    let first = if test.campaign {
        pkg.campaign.as_ref().map(|c| c.start.clone()).unwrap_or_default()
    } else {
        test.map.clone().unwrap_or_default()
    };
    let failed_at_start = |what: String| SeedRun {
        seed,
        failure: Some(Failure { step: 0, line: 0, label: "start".into(), what, tail: Vec::new() }),
        maps: Vec::new(),
        ending: None,
        log: Vec::new(),
        secs: started.elapsed().as_secs_f64(),
    };
    // The leading choose steps are answered when the first map starts (its arrival dialogs fire then).
    let mut choices: Vec<(String, String)> = Vec::new();
    for s in &test.steps {
        match &s.act {
            Some(Act::Choose { dialog, choice }) => choices.push((dialog.clone(), choice.clone())),
            _ => break,
        }
    }
    let Some((opened, content)) = maps.by_id.get(&first) else {
        return failed_at_start(format!("the map `{first}` is not opened"));
    };
    let refs: Vec<(&str, &str)> = choices.iter().map(|(d, c)| (d.as_str(), c.as_str())).collect();
    let run = match Run::carried(opened, content.clone(), seed, &refs, &Carried::default()) {
        Ok(run) => run,
        Err(e) => return failed_at_start(format!("the start: {e}")),
    };
    let mut run = run;
    if !test.campaign {
        run.g.avoid_armies = test.avoid_armies;
    }
    run.budget = Some(limit.budget(0.0));
    let mut session = Session { pkg, maps, campaign: test.campaign, map: first.clone(), run, choices, played: vec![first], log: Vec::new(), avoid: test.avoid_armies, seed, spent: 0.0, limit };
    let mut failure = None;
    for (i, step) in test.steps.iter().enumerate() {
        let result = (|| -> Result<(), String> {
            if let Some(act) = &step.act {
                session.act(act)?;
            }
            for c in &step.expect {
                session.check(c)?;
            }
            Ok(())
        })();
        if let Err(what) = result {
            failure = Some(Failure { step: i + 1, line: step.line, label: step.label.clone(), what, tail: tail_of(&session.full_log()) });
            break;
        }
    }
    SeedRun {
        seed,
        failure,
        maps: session.played.clone(),
        ending: session.run.ending(),
        log: session.full_log(),
        secs: started.elapsed().as_secs_f64(),
    }
}

fn tail_of(log: &[String]) -> Vec<String> {
    log[log.len().saturating_sub(TAIL)..].to_vec()
}

/// The result of one test over its seeds.
#[derive(Debug, Clone)]
pub struct TestResult {
    pub name: String,
    pub file: String,
    pub campaign: bool,
    pub map: Option<String>,
    pub seeds: u32,
    pub min_rate: f64,
    /// The package's path as given to the run (for the replay line).
    pub package: String,
    pub passed: u32,
    /// The seeds' playing time, summed.
    pub secs: f64,
    /// The seeds that failed, in seed order.
    pub failures: Vec<SeedRun>,
}

impl TestResult {
    pub fn rate(&self) -> f64 {
        self.passed as f64 / self.seeds as f64
    }

    /// The test meets its `min_rate`.
    pub fn ok(&self) -> bool {
        self.rate() + 1e-9 >= self.min_rate
    }
}

/// What to run: one test by its name, the seeds (instead of each test's own count), and the moment the
/// whole run must end by (`None`: each seed has only its own budget).
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub only: Option<String>,
    pub seeds: Option<u32>,
    pub deadline: Option<Instant>,
}

/// Runs the package's tests over their seeds, in parallel threads (at most the cores the machine has).
/// `root` is the package's folder or `.rzpack`, as the opened maps name it.
pub fn run_package(pkg: &Package, root: &Path, install: &DtInstall, opts: &Options) -> Result<(Vec<TestResult>, f64), String> {
    let mut all = tests(pkg);
    if let Some(name) = &opts.only {
        if !all.iter().any(|t| &t.name == name) {
            let have: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
            return Err(format!("no test named `{name}` (the package's tests: {})", if have.is_empty() { "none".into() } else { have.join(", ") }));
        }
        all.retain(|t| &t.name == name);
    }
    if let Some(n) = opts.seeds {
        let n = seeds_asked(n)?;
        for t in &mut all {
            t.seeds = n;
        }
    }
    let started = Instant::now();
    let mut ids: Vec<String> = Vec::new();
    for t in &all {
        if t.campaign {
            if let Some(c) = &pkg.campaign {
                ids.extend(c.maps.iter().map(|m| m.id.clone()));
            }
        } else if let Some(m) = &t.map {
            ids.push(m.clone());
        }
    }
    let maps = open_maps(pkg, root, install, &ids)?;
    let work: Vec<(usize, u32)> = all.iter().enumerate().flat_map(|(i, t)| (1..=t.seeds).map(move |s| (i, s))).collect();
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(usize, SeedRun)>> = Mutex::new(Vec::new());
    let threads = threads_for(thread::available_parallelism().map(|n| n.get()).unwrap_or(1), work.len());
    let call = opts.deadline;
    thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(&(t, seed)) = work.get(i) else { break };
                let test = &all[t];
                let limit = Limit::for_seed(call);
                let run = panic::catch_unwind(AssertUnwindSafe(|| play_seed(pkg, &maps, test, seed, limit))).unwrap_or_else(|payload| {
                    let msg = payload.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_default();
                    SeedRun { seed, failure: Some(Failure { step: 0, line: 0, label: "engine".into(), what: format!("the engine panicked: {msg}"), tail: Vec::new() }), maps: Vec::new(), ending: None, log: Vec::new(), secs: 0.0 }
                });
                done.lock().unwrap_or_else(|e| e.into_inner()).push((t, run));
            });
        }
    });
    let mut per_test: Vec<Vec<SeedRun>> = vec![Vec::new(); all.len()];
    for (t, run) in done.into_inner().unwrap_or_else(|e| e.into_inner()) {
        per_test[t].push(run);
    }
    let results = all
        .iter()
        .zip(per_test)
        .map(|(t, mut runs)| {
            runs.sort_by_key(|r| r.seed);
            let secs = runs.iter().map(|r| r.secs).sum();
            let failures: Vec<SeedRun> = runs.iter().filter(|r| r.failure.is_some()).cloned().collect();
            TestResult {
                name: t.name.clone(),
                file: t.file.clone(),
                campaign: t.campaign,
                map: t.map.clone(),
                seeds: t.seeds,
                min_rate: t.min_rate,
                package: root.display().to_string(),
                passed: runs.len() as u32 - failures.len() as u32,
                secs,
                failures,
            }
        })
        .collect();
    Ok((results, started.elapsed().as_secs_f64()))
}

/// Whether every test meets its rate (the exit status of `razdor-pack test`).
pub fn all_ok(results: &[TestResult]) -> bool {
    results.iter().all(TestResult::ok)
}

/// The results as text: one line per test, and for a failing test its first failing seed's step and log tail.
pub fn text(results: &[TestResult], wall: f64) -> String {
    let mut out = String::new();
    if results.is_empty() {
        out.push_str("no tests: the package has no tests/*.toml\n");
    }
    for r in results {
        let mode = if r.campaign { "campaign".to_string() } else { format!("map {}", r.map.clone().unwrap_or_default()) };
        let verdict = if r.ok() { "ok  " } else { "FAIL" };
        out.push_str(&format!(
            "{verdict} {:<22} {:<20} {:>3}/{:<3} seeds ({:>3.0}%, min {:>3.0}%) {:>6.2} s\n",
            r.name,
            mode,
            r.passed,
            r.seeds,
            r.rate() * 100.0,
            r.min_rate * 100.0,
            r.secs
        ));
        if let Some(f) = r.failures.first() {
            let fail = f.failure.as_ref().expect("a failure");
            out.push_str(&format!("     seed {}: step {} (tests/{}.toml line {}), {}\n", f.seed, fail.step, r.name, fail.line, fail.label));
            out.push_str(&format!("       {}\n", fail.what));
            out.push_str(&format!("       replay: {}\n", replay_of(r, f.seed)));
            if r.failures.len() > 1 {
                let seeds: Vec<String> = r.failures.iter().map(|x| x.seed.to_string()).collect();
                out.push_str(&format!("     failing seeds: {}\n", seeds.join(", ")));
            }
            out.push_str("     log tail:\n");
            for line in &fail.tail {
                out.push_str(&format!("       {line}\n"));
            }
        }
    }
    out.push_str(&format!("{} test(s), {} of them ok, {:.2} s\n", results.len(), results.iter().filter(|r| r.ok()).count(), wall));
    out
}

/// The results as JSON: one object per test, each with its failures (step, line, what, log tail).
pub fn json(results: &[TestResult]) -> Value {
    Value::Array(
        results
            .iter()
            .map(|r| {
                let failures: Vec<Value> = r
                    .failures
                    .iter()
                    .map(|f| {
                        let fail = f.failure.as_ref().expect("a failure");
                        json!({ "seed": f.seed, "step": fail.step, "line": fail.line, "label": fail.label, "what": fail.what, "log_tail": fail.tail, "maps": f.maps, "replay": replay_of(r, f.seed) })
                    })
                    .collect();
                json!({
                    "test": r.name,
                    "file": r.file,
                    "campaign": r.campaign,
                    "map": r.map,
                    "seeds": r.seeds,
                    "passed": r.passed,
                    "rate": r.rate(),
                    "min_rate": r.min_rate,
                    "ok": r.ok(),
                    "seconds": r.secs,
                    "failures": failures,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> Located<TestFile> {
        let value: TestFile = toml::from_str(text).unwrap_or_else(|e| panic!("{e}"));
        Located { file: "tests/t.toml".into(), line: 1, value }
    }

    #[test]
    fn a_name_is_the_file_stem_of_a_top_level_test() {
        assert_eq!(name_of("tests/marsh_route.toml"), Some("marsh_route"));
        assert_eq!(name_of("tests/a/b.toml"), None);
        assert_eq!(name_of("tests/.toml"), None);
        assert_eq!(name_of("maps/marka/map.toml"), None);
    }

    #[test]
    fn an_unknown_key_does_not_read() {
        assert!(toml::from_str::<TestFile>("map = \"marka\"\nsped = 3\n").is_err());
        assert!(toml::from_str::<TestFile>("map = \"marka\"\n[[step]]\nwalk_to = \"x\"\n").is_err());
        assert!(toml::from_str::<TestFile>("map = \"marka\"\n[[step]]\nexpect = { vr = \"x\" }\n").is_err());
    }

    #[test]
    fn a_step_has_one_action_and_a_test_one_map_or_the_campaign() {
        let both = file("campaign = true\nmap = \"marka\"\n[[step]]\nwait_days = 1\n");
        let problems = read(&both).expect_err("both a campaign and a map");
        assert!(problems.iter().any(|p| p.message.contains("not both")), "{problems:?}");
        let two = file("map = \"marka\"\n[[step]]\nwalk = [1, 2]\nwait_days = 1\n");
        assert!(read(&two).expect_err("two actions").iter().any(|p| p.message.contains("one action")));
        let nothing = file("map = \"marka\"\n[[step]]\nhire_all = false\n");
        assert!(read(&nothing).is_err());
        let ok = file("map = \"marka\"\nseeds = 2\nmin_rate = 0.5\n[[step]]\nwalk_to_building = \"ashford\"\ndays = 4\n[[step]]\nexpect = { var = \"dread\", op = \">=\", value = 2 }\n");
        let t = read(&ok).unwrap_or_else(|p| panic!("{p:?}"));
        assert_eq!((t.seeds, t.min_rate), (2, 0.5));
        assert_eq!(t.steps[0].act, Some(Act::Building { id: "ashford".into(), days: 4 }));
        assert_eq!(t.steps[1].expect, vec![Check::Var { name: "dread".into(), op: Op::Ge, value: VarValue::Int(2) }]);
    }

    #[test]
    fn a_flag_compares_only_with_equal_or_not_equal() {
        let f = file("map = \"marka\"\n[[step]]\nexpect = { var = \"crown_ok\", op = \">\", value = true }\n");
        assert!(read(&f).is_err());
        let g = file("map = \"marka\"\n[[step]]\nexpect = { var = \"crown_ok\", op = \"!=\", value = true }\n");
        assert!(read(&g).is_ok());
    }

    #[test]
    fn the_longest_gap_is_between_beats_and_the_ends() {
        assert_eq!(longest_gap_of(9, &[3]), 6);
        assert_eq!(longest_gap_of(9, &[3, 8]), 5);
        assert_eq!(longest_gap_of(0, &[]), 0);
    }

    /// The example package's tests read and check, and the package checks with no errors.
    #[test]
    fn the_example_package_has_its_tests_and_checks() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let files = super::super::files::Files::load(&root).unwrap_or_else(|e| panic!("{e}"));
        let (pkg, _) = Package::parse(files);
        let names: Vec<String> = tests(&pkg).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["chapter1_patrol", "route_companies", "route_heretic", "route_marsh", "route_order"]);
        let report = super::super::check(&pkg);
        assert!(!report.has_errors(), "{}", report.to_text());
    }

    /// The four campaign routes and the patrol test win on their seeds (skipped without `RAZDOR_DT_DIR`).
    #[test]
    fn the_example_tests_meet_their_rates() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = DtInstall::load(Path::new(&dir)).unwrap_or_else(|e| panic!("{e}"));
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let files = super::super::files::Files::load(&root).unwrap_or_else(|e| panic!("{e}"));
        let (pkg, _) = Package::parse(files);
        let (results, _) = run_package(&pkg, &root, &install, &Options::default()).unwrap_or_else(|e| panic!("{e}"));
        assert!(all_ok(&results), "{}", text(&results, 0.0));
    }

    /// The caps: a test plays at most `MAX_SEEDS` seeds, a step at most `MAX_DAYS` game days (a
    /// `days`, a `wait_days`), and both read as problems past them.
    #[test]
    fn the_seed_and_day_caps_read_as_problems() {
        let at_cap = file("map = \"marka\"\nseeds = 200\n[[step]]\nwait_days = 365\n[[step]]\nwalk_to_building = \"ashford\"\ndays = 365\n");
        assert!(read(&at_cap).is_ok(), "the caps themselves read");
        let seeds = read(&file("map = \"marka\"\nseeds = 201\n[[step]]\nwait_days = 1\n")).expect_err("201 seeds");
        assert!(seeds.iter().any(|p| p.message.contains("at most 200 seeds")), "{seeds:?}");
        let huge = read(&file("map = \"marka\"\nseeds = 4294967295\n[[step]]\nwait_days = 1\n")).expect_err("u32::MAX seeds");
        assert!(huge.iter().any(|p| p.message.contains("at most 200 seeds")), "{huge:?}");
        let wait = read(&file("map = \"marka\"\n[[step]]\nwait_days = 366\n")).expect_err("a 366-day wait");
        assert!(wait.iter().any(|p| p.message.contains("wait_days is 366")), "{wait:?}");
        let walk = read(&file("map = \"marka\"\n[[step]]\nwalk_to_building = \"ashford\"\ndays = 366\n")).expect_err("a 366-day walk");
        assert!(walk.iter().any(|p| p.message.contains("days is 366")), "{walk:?}");
        let fight = read(&file("map = \"marka\"\n[[step]]\nfight = \"patrol\"\ndays = 9999999999\n")).expect_err("a fight of ten billion days");
        assert!(fight.iter().any(|p| p.message.contains("days is 9999999999")), "{fight:?}");
    }

    /// The seed count asked on the command line or by the studio is checked, and the threads leave two
    /// cores to the machine (at least one thread, never more than the seeds).
    #[test]
    fn the_seed_count_asked_is_checked_and_two_cores_are_left_free() {
        assert_eq!(seeds_asked(1), Ok(1));
        assert_eq!(seeds_asked(MAX_SEEDS), Ok(MAX_SEEDS));
        assert!(seeds_asked(0).is_err(), "no seeds");
        assert!(seeds_asked(MAX_SEEDS + 1).is_err(), "over the cap");
        assert_eq!(threads_for(1, 15), 1, "one core: one thread");
        assert_eq!(threads_for(2, 15), 1, "two cores: still one");
        assert_eq!(threads_for(12, 15), 10, "twelve cores leave two");
        assert_eq!(threads_for(12, 3), 3, "never more threads than seeds");
        assert_eq!(threads_for(32, 100), 30);
        assert_eq!(threads_for(0, 0), 1, "nothing to run still gives a thread");
    }

    /// The replay line names the `razdor-pack sim` run that plays the same story: a map's on its map, a
    /// campaign's with `--campaign`, a folder with a space quoted.
    #[test]
    fn a_failing_seed_names_the_sim_that_replays_it() {
        let result = |campaign: bool, map: Option<&str>, package: &str| TestResult {
            name: "t".into(),
            file: "tests/t.toml".into(),
            campaign,
            map: map.map(str::to_string),
            seeds: 15,
            min_rate: 0.8,
            package: package.into(),
            passed: 14,
            secs: 0.0,
            failures: Vec::new(),
        };
        assert_eq!(replay_of(&result(false, Some("marka"), "examples/pack"), 4), "razdor-pack sim examples/pack --map marka --seed 4");
        assert_eq!(replay_of(&result(true, None, "examples/pack"), 7), "razdor-pack sim examples/pack --campaign --seed 7");
        assert_eq!(replay_of(&result(true, None, "my packs/a'b"), 2), "razdor-pack sim 'my packs/a'\\''b' --campaign --seed 2");
    }

    /// A seed gets its own story: two neighbouring seeds do not share one (the old `seed | 1` made
    /// seeds 2 and 3 the same), and seed N is the story `razdor-pack sim --seed N` plays on the same map.
    #[test]
    fn neighbouring_seeds_have_their_own_story_and_seed_n_is_the_sims() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = DtInstall::load(Path::new(&dir)).unwrap_or_else(|e| panic!("{e}"));
        let (opened, content) = super::super::playthrough::brod(&install);
        let story_of = |seed: u32| {
            let run = Run::new(&opened, content.clone(), seed, super::super::playthrough::PREPARED).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            run.g.story().map(|s| s.seed).expect("the map has a story")
        };
        let (two, three) = (story_of(2), story_of(3));
        assert_ne!(two, three, "neighbouring seeds share a story");
        assert_eq!(two, open::story_seed(&opened.id, &opened.map, 2), "seed 2 is `sim --seed 2`'s story");
        assert_eq!(three, open::story_seed(&opened.id, &opened.map, 3), "seed 3 is `sim --seed 3`'s story");
    }

    /// The limits: a Limit whose deadline is already past stops a long wait at its first step, and a seed
    /// with fewer game days than the wait stops with the game-day message. Both fail the seed (no hang).
    #[test]
    fn a_seed_past_its_time_or_its_game_days_fails_with_the_reason() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = DtInstall::load(Path::new(&dir)).unwrap_or_else(|e| panic!("{e}"));
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let files = super::super::files::Files::load(&root).unwrap_or_else(|e| panic!("{e}"));
        let (pkg, _) = Package::parse(files);
        let maps = open_maps(&pkg, &root, &install, &["brod".to_string()]).unwrap_or_else(|e| panic!("{e}"));
        let test = read(&file("map = \"brod\"\n[[step]]\nwait_days = 30\n")).unwrap_or_else(|p| panic!("{p:?}"));
        let now = Instant::now();
        let out_of_time = Limit { deadline: now, game_days: SEED_GAME_DAYS, reason: "seed's time budget" };
        let run = play_seed(&pkg, &maps, &test, 1, out_of_time);
        let f = run.failure.expect("a seed past its deadline fails");
        assert!(f.what.contains("out of time") && f.what.contains("seed's time budget"), "{}", f.what);
        let few_days = Limit { deadline: now + Duration::from_secs(600), game_days: 5, reason: "seed's time budget" };
        let run = play_seed(&pkg, &maps, &test, 1, few_days);
        let f = run.failure.expect("a seed past its game days fails");
        assert!(f.what.contains("out of game days"), "{}", f.what);
        let plenty = Limit { deadline: now + Duration::from_secs(600), game_days: SEED_GAME_DAYS, reason: "seed's time budget" };
        assert!(play_seed(&pkg, &maps, &test, 1, plenty).failure.is_none(), "30 days fit the budget");
    }

    /// A call's deadline before the seed's own one is the limit that applies, and it says so.
    #[test]
    fn the_call_deadline_applies_when_it_comes_first() {
        let soon = Instant::now() + Duration::from_secs(1);
        assert_eq!(Limit::for_seed(Some(soon)).reason, "test run's time limit");
        assert_eq!(Limit::for_seed(None).reason, "seed's time budget");
        assert_eq!(Limit::for_seed(Some(Instant::now() + Duration::from_secs(SEED_SECONDS * 10))).reason, "seed's time budget");
    }
}
