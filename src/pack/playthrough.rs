//! Test support: a real playthrough of a package's map. The hero walks with the game's own route
//! planner and clock (`set_destination`, `tick`, `wait`), the AI moves the armies as it does in
//! play, battles are fought by the battle engine (`sim::fight_it_out`), questions are answered
//! yes, and the package's dialogs are answered by a script of choices. Every dialog, battle and
//! capture is logged with its day, so a failure says what happened.
//!
//! The chapter playthroughs of «Брод» (skipped without `RAZDOR_DT_DIR`) are at the bottom.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use super::open::{open, Opened};
use super::triggers::VarValue;
use crate::dt::install::DtInstall;
use crate::rules::battle::Outcome;
use crate::rules::cheats::{self, Cheat};
use crate::rules::clock::MINUTES_PER_DAY;
use crate::rules::content::{Content, HeroClass};
use crate::rules::game::{Event, Foe, Game, STEP_SECONDS};
use crate::rules::map::Tile;
use crate::rules::rng::Rng;
use crate::rules::sim::{fight_it_out, FieldFight};
use crate::rules::triggers::{Ending, PendingDialog};

/// One battle the hero fought.
#[derive(Clone, Debug, PartialEq)]
pub struct Fight {
    /// The enemy as the package names it (`army_names`, or the building's name).
    pub enemy: String,
    pub outcome: Outcome,
    /// The day after the start when it was fought.
    pub day: f64,
    /// The squad's size before the battle.
    pub squad: usize,
}

/// Why a playthrough stopped before its goal.
#[derive(Clone, Debug, PartialEq)]
pub enum Stuck {
    /// The planner has no route to the cell.
    NoRoute { to: Tile, day: f64 },
    /// The hero did not reach the cell within the day limit.
    Late { to: Tile, day: f64 },
    /// The hero lost a battle.
    Lost { enemy: String, day: f64 },
    /// A dialog's choice is not one it offers, or a loop did not end.
    Broken(String),
}

impl fmt::Display for Stuck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stuck::NoRoute { to, day } => write!(f, "no route to {to:?} on day {day:.1}"),
            Stuck::Late { to, day } => write!(f, "did not reach {to:?} by day {day:.1}"),
            Stuck::Lost { enemy, day } => write!(f, "lost the battle against {enemy} on day {day:.1}"),
            Stuck::Broken(what) => write!(f, "{what}"),
        }
    }
}

/// The state of the game at one day's start (sampled once a day by the walks and the waits).
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    /// The day after the start.
    pub day: f64,
    pub gold: i32,
    /// The squad's size.
    pub squad: usize,
    /// The gold wage bill and the income of the owned castles and forts, per day.
    pub wages: i32,
    pub income: i32,
}

/// A playthrough: the game, what it did, and the choices the script makes.
pub struct Run {
    pub g: Game,
    /// What happened, one line per dialog, battle and capture, with its day.
    pub log: Vec<String>,
    /// The choice to pick in each dialog, by dialog id. A dialog not listed takes its first choice
    /// (or «Далее» when it has none).
    pub choices: HashMap<String, String>,
    /// The battles fought so far, oldest first.
    pub battles: Vec<Fight>,
    /// The field battles among them (a package's `lua:<script>` battle): rounds, winner, the defenders' HP.
    pub fields: Vec<FieldFight>,
    /// One sample a day: the gold, the squad, the bills and the income (see [`Sample`]).
    pub ledger: Vec<Sample>,
    /// Units the game deserted for unpaid wages (its noon reports' deserter lists). A unit a story
    /// takes out of the squad (`remove_unit`, a companion's leave) is not a desertion.
    pub deserted: usize,
    /// The cells the hero stood on, one per step (the first is the start), for the beat-gap test.
    pub path: Vec<Tile>,
    /// When set, a walk ends at once after a step that brings a raid (`raid_*`) within this many cells, so the
    /// caller can meet it on the way (see [`go_meeting_raids`]).
    pub stop_near_raids: Option<i32>,
    /// The index into `path` of each story beat (a dialog, a capture, an encounter), in order.
    pub beats: Vec<usize>,
    start: f64,
    last_day: i64,
}

impl Run {
    /// A game of `opened` (its story seeded with `seed`, the game's generator at `seed`), the map
    /// revealed (the planner needs explored cells; no route waits for the fog), its opening moment
    /// answered. Campaign-free: the package's first map.
    pub fn new(opened: &Opened, content: Arc<Content>, seed: u32, choices: &[(&str, &str)]) -> Result<Run, Stuck> {
        Run::carried(opened, content, seed, choices, &Carried::default())
    }

    /// [`Run::new`] with what the earlier chapters carry in (`carry`): the gold, the squad and the
    /// story's variables, set before the map's moments fire (as `Carried::apply` says).
    pub(crate) fn carried(opened: &Opened, content: Arc<Content>, seed: u32, choices: &[(&str, &str)], carry: &Carried) -> Result<Run, Stuck> {
        let mut o = opened.clone();
        o.story.seed = seed | 1;
        let mut g = Game::from_scenario(content, &o.scenario, HeroClass::Knight);
        g.rng = Rng::new(seed);
        g.seed_events(seed);
        cheats::run(&Cheat::Reveal, Some(&mut g), None).expect("reveal the map");
        carry.apply(&mut g, &o);
        g.avoid_armies = carry.avoid_armies;
        g.triggers.trace = Some(Vec::new());
        o.start(&mut g);
        let start = g.clock.total_minutes();
        let mut run = Run::fresh(g, choices, start);
        run.service()?;
        Ok(run)
    }

    fn fresh(g: Game, choices: &[(&str, &str)], start: f64) -> Run {
        let here = g.tile();
        Run {
            g,
            log: Vec::new(),
            choices: choices.iter().map(|(d, c)| (d.to_string(), c.to_string())).collect(),
            battles: Vec::new(),
            fields: Vec::new(),
            ledger: Vec::new(),
            deserted: 0,
            path: vec![here],
            stop_near_raids: None,
            beats: Vec::new(),
            start,
            last_day: -1,
        }
    }

    /// A run on a game that already exists: a campaign's hand-over (`rules::campaign::enter`) starts
    /// the next map's story itself, and the run answers what that raised. The map is revealed, as in
    /// [`Run::new`], with the option «Let pass» (a step onto a friendly army is no battle). The day count starts now.
    pub(crate) fn resume(mut g: Game, choices: &[(&str, &str)]) -> Result<Run, Stuck> {
        cheats::run(&Cheat::Reveal, Some(&mut g), None).expect("reveal the map");
        // The option «Let pass»: the planner may cross a friendly army, and a step onto it is no battle.
        g.friends_let_pass = true;
        let start = g.clock.total_minutes();
        let mut run = Run::fresh(g, choices, start);
        run.service()?;
        Ok(run)
    }

    /// The day after the start.
    pub fn day(&self) -> f64 {
        (self.g.clock.total_minutes() - self.start) / MINUTES_PER_DAY as f64
    }

    /// The chapter is over (a victory, a defeat or an ending).
    pub fn done(&self) -> bool {
        self.g.story_end().is_some() || self.g.script_end().is_some() || self.ending().is_some()
    }

    /// The campaign's ending dialog, once a story's `ending` action ran (`campaign.toml`'s last map).
    pub fn ending(&self) -> Option<String> {
        self.g.triggers.ending.clone()
    }

    /// The story's victory was reached.
    pub fn victory(&self) -> bool {
        self.g.story_end() == Some(Ending::Victory)
    }

    /// The story's value of variable `name` (`None` when it is not declared).
    pub fn var(&self, name: &str) -> Option<VarValue> {
        self.g.story_var(name)
    }

    pub(crate) fn note(&mut self, what: impl AsRef<str>) {
        self.log.push(format!("day {:>5.2}: {}", self.day(), what.as_ref()));
    }

    /// Answers what is waiting: a question (yes), a battle the hero is in, the events the game
    /// has raised (dialogs, captures, encounters). Runs until nothing waits.
    pub fn service(&mut self) -> Result<(), Stuck> {
        self.sample();
        for _ in 0..10_000 {
            if self.g.pending_question().is_some() {
                let ev = self.g.answer_question(true);
                self.settle(ev)?;
                continue;
            }
            if self.g.foe.is_some() {
                self.fight()?;
                continue;
            }
            let ev = self.g.drain_events();
            if ev.is_empty() {
                // A scenario window the screen would close (a no-op for a package's own dialogs).
                let closed = self.g.release_unshown_window(false);
                if closed.is_empty() {
                    return Ok(());
                }
                self.settle(closed)?;
                continue;
            }
            self.settle(ev)?;
        }
        Err(Stuck::Broken("the events did not settle".into()))
    }

    /// Records the day's sample when a new day has begun.
    fn sample(&mut self) {
        let day = ((self.g.clock.total_minutes() - self.start) / MINUTES_PER_DAY as f64).floor() as i64;
        if day == self.last_day {
            return;
        }
        self.last_day = day;
        let squad = self.g.squad.iter().filter(|u| u.alive()).count();
        let income = self.g.world.locations.iter().filter(|l| l.owned() && matches!(l.kind, crate::rules::world::LocationKind::Castle | crate::rules::world::LocationKind::Fort)).map(|l| l.gold_income * self.g.difficulty() / 100).sum();
        self.ledger.push(Sample { day: self.day(), gold: self.g.gold, squad, wages: self.g.daily_wages(), income });
    }

    /// Answers the dialogs among `events` (and those the answers open), notes the captures and the
    /// encounters.
    pub(crate) fn settle(&mut self, events: Vec<Event>) -> Result<(), Stuck> {
        let mut queue: VecDeque<Event> = events.into();
        while let Some(e) = queue.pop_front() {
            if matches!(e, Event::Package(_) | Event::Captured(_) | Event::Encounter(_)) {
                self.beats.push(self.path.len() - 1);
            }
            match e {
                Event::Package(d) => {
                    let choice = self.choice_for(&d)?;
                    self.note(format!("dialog {} -> {}", d.id, choice.as_deref().unwrap_or("(next)")));
                    queue.extend(self.g.answer_story_dialog(&d.id, choice.as_deref()));
                }
                Event::NewDay(r) if !r.deserted.is_empty() => {
                    self.deserted += r.deserted.len();
                    self.note(format!("{} unit(s) deserted unpaid", r.deserted.len()));
                }
                Event::Captured(l) => {
                    let name = self.g.world.locations[l].name.clone();
                    self.note(format!("captured {name}"));
                }
                Event::Encounter(i) => {
                    let name = self.army_name(i);
                    self.note(format!("caught by {name}"));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The choice the script makes in `d`: the listed one, else its first; a choice the dialog does
    /// not offer is a broken script.
    fn choice_for(&self, d: &PendingDialog) -> Result<Option<String>, Stuck> {
        let Some(want) = self.choices.get(&d.id) else { return Ok(d.choices.first().cloned()) };
        if d.choices.is_empty() || d.choices.contains(want) {
            return Ok(Some(want.clone()));
        }
        Err(Stuck::Broken(format!("dialog {} offers {:?}, not {want}", d.id, d.choices)))
    }

    fn army_name(&self, i: usize) -> String {
        let Some(a) = self.g.world.armies.get(i) else { return format!("army #{i}") };
        self.g.story().and_then(|s| s.army_names.get(&a.id).cloned()).unwrap_or_else(|| a.name.clone())
    }

    /// The name of the pending foe: an army, or a building's garrison.
    fn foe_name(&self) -> String {
        match self.g.foe {
            Some(Foe::Army(i)) => self.army_name(i),
            Some(Foe::Garrison(l)) => self.g.world.locations[l].name.clone(),
            None => "nobody".into(),
        }
    }

    /// Fights the pending foe on the AI (both sides), records it, and stops the walk on a loss.
    fn fight(&mut self) -> Result<(), Stuck> {
        let enemy = self.foe_name();
        let squad = self.g.squad.len();
        let hp: Vec<i32> = self.g.squad.iter().map(|u| u.hp).collect();
        let at = self.g.tile();
        self.note(format!("battle with {enemy} starts at {at:?}; squad hp {hp:?}"));
        let outcome = fight_it_out(&mut self.g, &mut self.fields);
        let day = self.day();
        self.battles.push(Fight { enemy: enemy.clone(), outcome, day, squad });
        self.note(format!("battle vs {enemy}: {outcome:?} ({squad} in the squad)"));
        let ev = self.g.drain_events();
        self.settle(ev)?;
        if outcome == Outcome::Defeat {
            return Err(Stuck::Lost { enemy, day });
        }
        Ok(())
    }

    /// Walks the hero to `to` by the game's own route, over the clock, until he stands there, or a
    /// battle ends the walk (a building or an army he goes for is fought on the way, so his walk
    /// ends there). A lost battle, no route, or `max_days` without arriving are errors.
    pub fn walk_to(&mut self, to: Tile, max_days: u64) -> Result<(), Stuck> {
        let deadline = self.g.clock.total_minutes() + (max_days * MINUTES_PER_DAY) as f64;
        let before = self.battles.len();
        // The minute of the last route planned: a second plan in the same minute (a friendly army
        // in the way stops the walk at once, see `Game::step_contact`) lets a minute pass first,
        // so the armies move on.
        let mut planned_at = None;
        let mut stalled = 0;
        let mut clock = self.g.clock.total_minutes();
        for _ in 0..2_000_000 {
            let stood = self.g.tile();
            self.service()?;
            // No time passing for a long while: the walk is blocked (the route stops, nothing moves).
            if self.g.clock.total_minutes() == clock {
                stalled += 1;
                if stalled > 5_000 {
                    return Err(Stuck::Broken(format!("the walk to {to:?} is blocked at {:?} on day {:.2}", self.g.tile(), self.day())));
                }
            } else {
                stalled = 0;
                clock = self.g.clock.total_minutes();
            }
            if self.done() {
                return Ok(());
            }
            if self.g.clock.total_minutes() >= deadline {
                return Err(Stuck::Late { to, day: self.day() });
            }
            if self.g.foe.is_none() && !self.g.moving() {
                if self.g.tile() == to || self.battles.len() > before {
                    return Ok(());
                }
                let now = self.g.clock.total_minutes();
                if planned_at == Some(now) {
                    let ev = self.g.wait(4);
                    self.settle(ev)?;
                    continue;
                }
                planned_at = Some(now);
                if !self.g.set_destination(to) {
                    return Err(Stuck::NoRoute { to, day: self.day() });
                }
                self.note(format!("walks to {to:?}"));
            }
            let ev = self.g.tick(STEP_SECONDS);
            self.settle(ev)?;
            // A teleport (a stone's choice, a story's move) ends the walk where he lands.
            let now = self.g.tile();
            if self.path.last() != Some(&now) {
                self.path.push(now);
            }
            if self.stop_near_raids.is_some_and(|reach| raid_near(self, reach).is_some()) {
                return Ok(());
            }
            if (now.0 - stood.0).abs() > 1 || (now.1 - stood.1).abs() > 1 {
                return Ok(());
            }
        }
        Err(Stuck::Broken(format!("the walk to {to:?} did not end")))
    }

    /// Stands still for `days` days (the clock runs; the armies move; the story's moments fire),
    /// fighting whatever comes. Stops early when the chapter is over.
    pub fn wait_days(&mut self, days: u64) -> Result<(), Stuck> {
        let end = self.g.clock.total_minutes() + (days * MINUTES_PER_DAY) as f64;
        for _ in 0..2_000_000 {
            self.service()?;
            if self.done() || self.g.clock.total_minutes() >= end {
                return Ok(());
            }
            let ev = self.g.wait(4);
            self.settle(ev)?;
        }
        Err(Stuck::Broken("the wait did not end".into()))
    }

    /// Waits up to `days` days for the chapter to end (`wait_days` in day steps).
    pub fn wait_until_done(&mut self, days: u64) -> Result<(), Stuck> {
        for _ in 0..days {
            self.wait_days(1)?;
            if self.done() {
                break;
            }
        }
        Ok(())
    }

    /// Hires what is offered here, cheapest first, while the total spent stays at most `budget`
    /// gold and the squad has room. Returns the units hired. The hero must stand in the building.
    pub fn hire_all(&mut self, budget: i32) -> Vec<String> {
        let mut hired = Vec::new();
        let mut spent = 0;
        loop {
            let mut offers = self.g.recruits_here();
            offers.sort_by_key(|&k| self.g.hire_price(k).amount);
            let Some(kind) = offers.into_iter().find(|&k| spent + self.g.hire_price(k).amount <= budget) else { break };
            let price = self.g.hire_price(kind).amount;
            if self.g.hire(kind).is_err() {
                break;
            }
            spent += price;
            let name = self.g.content.unit(kind).name.clone();
            hired.push(name.clone());
            self.note(format!("hired {name} for {price}"));
        }
        hired
    }

    /// Units of the squad by name, for the logs and the tests.
    pub fn squad_names(&self) -> Vec<String> {
        self.g.squad.iter().map(|u| self.g.content.unit(u.def).name.clone()).collect()
    }
}

/// The map `id` of the reference package, opened on `install` (skipped without `RAZDOR_DT_DIR`):
/// its content (the base game with the package's units) and its opened map.
pub(crate) fn open_map(install: &DtInstall, id: &str) -> (Opened, Arc<Content>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
    let opened = open(&root, Some(id), "en", install).unwrap_or_else(|e| panic!("{e}"));
    let content = Arc::new(Content::from_dt(install).with_package(opened.units.clone(), opened.items.clone()));
    (opened, content)
}

/// What the earlier chapters carry into a chapter (`[carry]`): the gold, the story's variables
/// (forced before the story starts, as the sim's `--set`), and the squad: ordinary members as
/// (unit alias, level, count) and named characters as (name, unit alias, level), placed in the
/// formation after the hero's own troops.
#[derive(Clone, Debug, Default)]
pub(crate) struct Carried {
    pub gold: Option<i32>,
    pub vars: Vec<(String, VarValue)>,
    pub squad: Vec<(&'static str, i32, usize)>,
    pub named: Vec<(&'static str, &'static str, i32)>,
    /// The player's «Обход армий на пути» option (`avoid_armies`): the route goes round the armies
    /// (an escort walking beside the hero blocks the route otherwise: a friend stepped onto stops
    /// the walk, and the planner crosses moving armies). Off by default, as the original.
    pub avoid_armies: bool,
}

impl Carried {
    /// Puts the carried state into `g` (the story is not attached yet): the gold, the variables
    /// and the squad. A named character's id is its place in the story's list of names.
    pub(crate) fn apply(&self, g: &mut Game, opened: &Opened) {
        if let Some(gold) = self.gold {
            g.gold = gold;
        }
        for (name, value) in &self.vars {
            g.triggers.vars.insert(name.clone(), value.clone());
        }
        for &(alias, level, count) in &self.squad {
            for _ in 0..count {
                place(g, alias, level, 0);
            }
        }
        for &(name, alias, level) in &self.named {
            let text = crate::rules::triggers::named_text(&opened.story.texts, name, &opened.map);
            let k = opened.story.named.iter().position(|n| *n == text).unwrap_or_else(|| panic!("no named character {name}"));
            place(g, alias, level, crate::rules::triggers::PACKAGE_BASE + k as u8);
        }
        // Paid up at the chapter's start, as a save's load does (save.rs): a unit never paid
        // (`last_paid` 0) would leave at the first short noon. A carry that places units only.
        if !self.squad.is_empty() || !self.named.is_empty() {
            let now = g.clock.total_minutes() as u64;
            for u in g.squad.iter_mut() {
                u.last_paid = now;
            }
        }
    }
}

/// Adds one unit of `alias` to the squad, in the first free slot of the formation.
fn place(g: &mut Game, alias: &str, level: i32, named: u8) {
    let id = crate::pack::aliases::resolve(crate::pack::refs::DtKind::Unit, alias).unwrap_or_else(|| panic!("no unit {alias}"));
    let taken: Vec<_> = g.squad.iter().map(|u| u.slot).collect();
    let slot = g.content.formation.new_unit_slot(&taken).expect("the formation has room for the carried squad");
    let mut u = crate::rules::units::Unit::new(&g.content, crate::rules::content::UnitId(id), slot);
    u.level = level;
    u.named = named;
    u.heal_full(&g.content);
    g.squad.push(u);
}

/// The chapter «Брод» of the reference package, opened on `install`: its content and its opened map,
/// shared by every playthrough.
pub(crate) fn brod(install: &DtInstall) -> (Opened, Arc<Content>) {
    open_map(install, "brod")
}

/// The choices the prepared hero makes: Varga joins, the toll is talked away, the marsh folk
/// are helped; the side quests (the barrow, the refugees, the deserters) are left alone.
pub(crate) const PREPARED: &[(&str, &str)] = &[
    ("varga_meet", "join"),
    ("toll", "talk"),
    ("marsh_plea", "help"),
    ("barrow", "leave"),
    ("refugees", "leave"),
    ("deserters", "pass"),
];

/// The prepared route up to the wraiths: the tower taken and garrisoned from its recruits,
/// Varga met, the toll talked away, the marsh folk helped, then the wraiths fought.
pub(crate) fn prepared_to_wraiths(run: &mut Run) -> Result<(), Stuck> {
    run.walk_to((44, 14), 10)?;
    run.walk_to((44, 14), 3)?;
    run.hire_all(run.g.gold);
    run.walk_to((34, 17), 5)?;
    run.walk_to((33, 23), 5)?;
    run.walk_to((6, 38), 10)?;
    run.walk_to((6, 44), 10)
}

/// The start army straight to the wraiths.
fn unprepared_to_wraiths(run: &mut Run) -> Result<(), Stuck> {
    run.walk_to((6, 44), 10)
}

/// Whether the barrow host (the package's army `host`) stands within three cells of the hero.
fn host_near(run: &Run) -> bool {
    let here = run.g.tile();
    run.g.world.armies.iter().enumerate().any(|(i, a)| {
        let t = a.tile(&run.g.world.map);
        run.army_name(i) == "host" && (t.0 - here.0).abs() <= 3 && (t.1 - here.1).abs() <= 3
    })
}

/// The hero takes one step to a free cell beside him and back. An AI army attacks only when
/// his step ends beside it, never while he stands (world.md §4.3), so this is how a waiting
/// hero meets an army that came to him.
fn step_out_and_back(run: &mut Run) -> Result<(), Stuck> {
    let here = run.g.tile();
    let cells: Vec<Tile> = (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (here.0 + dx, here.1 + dy))).filter(|&n| n != here && run.g.world.map.in_bounds(n)).collect();
    for n in cells {
        match run.walk_to(n, 1) {
            Ok(()) => return run.walk_to(here, 1),
            Err(Stuck::NoRoute { .. }) => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Waits up to `days` days for the barrow host to come. When it is near, the hero steps out
/// and back (`step_out_and_back`), so the host's attack comes on his step. Stops when the
/// chapter is over.
pub(crate) fn meet_host(run: &mut Run, days: u64) -> Result<(), Stuck> {
    for _ in 0..days {
        run.wait_days(1)?;
        if run.done() {
            return Ok(());
        }
        if host_near(run) {
            step_out_and_back(run)?;
            if run.done() {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// The nearest raid (an army named `raid_*`) within `reach` cells of the hero, if any.
fn raid_near(run: &Run, reach: i32) -> Option<Tile> {
    let here = run.g.tile();
    run.g
        .world
        .armies
        .iter()
        .enumerate()
        .filter(|(i, _)| run.army_name(*i).starts_with("raid_"))
        .map(|(_, a)| a.tile(&run.g.world.map))
        .find(|t| (t.0 - here.0).abs() <= reach && (t.1 - here.1).abs() <= reach)
}

/// Walks into the raids within `reach` cells of the hero, one after another, until none is near: a
/// walk into an army is a battle on contact (the way an AI army meets a walker). A raid the hero cannot
/// reach is left to the map.
pub(crate) fn intercept_raids(run: &mut Run, reach: i32) -> Result<(), Stuck> {
    for _ in 0..50 {
        let Some(t) = raid_near(run, reach) else { return Ok(()) };
        let (before, here, clock) = (run.battles.len(), run.g.tile(), run.g.clock.total_minutes());
        match run.walk_to(t, 1) {
            Ok(()) => {}
            Err(Stuck::NoRoute { .. }) => return Ok(()),
            Err(e) => return Err(e),
        }
        if run.battles.len() == before && run.g.tile() == here && run.g.clock.total_minutes() == clock {
            return Ok(());
        }
    }
    Ok(())
}

/// Walks to `to` (as [`go`]), meeting each raid that comes within eight cells on the way (a raid the hero
/// walks into is a battle on contact). The raids are the dead's own walk to the tower or Reedmere.
pub(crate) fn go_meeting_raids(run: &mut Run, to: Tile, days: u64) -> Result<(), Stuck> {
    let deadline = run.g.clock.total_minutes() + (days * MINUTES_PER_DAY) as f64;
    run.stop_near_raids = Some(8);
    let mut result = Ok(());
    for _ in 0..5_000 {
        if run.done() || run.g.tile() == to {
            break;
        }
        if run.g.clock.total_minutes() >= deadline {
            result = Err(Stuck::Late { to, day: run.day() });
            break;
        }
        if raid_near(run, 8).is_some() {
            if let Err(e) = intercept_raids(run, 8) {
                result = Err(e);
                break;
            }
        }
        if let Err(e) = run.walk_to(to, days) {
            result = Err(e);
            break;
        }
    }
    run.stop_near_raids = None;
    result
}

/// The side-quest route of chapter one, in the order of the narrative's beats (docs/ideas/pepelnaya-marka-narrative.md
/// §5): the hanged man's note, the farmstead, the tower and its recruits, the scout's letter, Varga, the toll
/// talked away, the barrow answered by `barrow` (`break`, `seal` or `leave`), the deserters of the north woods
/// handed to the Crown, the herbalist, the refugees taken to Reedmere, the marsh folk helped and the wraiths
/// beaten. The raids come on the Dread: the hero meets them as he walks (`go_meeting_raids`), and the host
/// rises the midnight after the warning at Dread 5. Each dialog is answered as the route needs (`Run::choices`).
pub(crate) fn side_quests_route(run: &mut Run, barrow: &str, hire: fn(&mut Run, i32)) -> Result<(), Stuck> {
    run.choices.insert("barrow".into(), barrow.into());
    run.choices.insert("deserters".into(), "hand".into());
    run.choices.insert("refugees".into(), "take".into());
    run.choices.insert("marsh_plea".into(), "help".into());
    run.choices.insert("varga_meet".into(), "join".into());
    run.choices.insert("toll".into(), "talk".into());
    // East: the hanged man's note, the farmstead, the tower and its recruits, the scout's letter, Varga (Dread 2:
    // the first raid) and the toll talked away (Dread 3).
    go_meeting_raids(run, (38, 23), 4)?;
    go_meeting_raids(run, (41, 21), 3)?;
    go_meeting_raids(run, (44, 14), 6)?;
    // The rite costs forty gold: the hiring keeps it when the barrow is to be sealed.
    hire(run, if barrow == "seal" { 40 } else { 0 });
    go_meeting_raids(run, (37, 12), 5)?;
    go_meeting_raids(run, (32, 17), 5)?;
    go_meeting_raids(run, (29, 23), 6)?;
    // Back to the tower, which heals what the raids cost while the hero holds it (the raids meet him there).
    go_meeting_raids(run, (44, 14), 6)?;
    if run.var("tower_taken") == Some(VarValue::Bool(true)) {
        super::playthrough_campaign::heal_within_means(run);
        // The gold the tower brings in since the first visit buys what it offers now (the seal's forty kept).
        hire(run, if barrow == "seal" { 40 } else { 0 });
    }
    // North: the barrow, the milestone on the north road, the north bridge and the deserters of the north woods.
    go_meeting_raids(run, (41, 6), 8)?;
    go_meeting_raids(run, (33, 7), 6)?;
    go_meeting_raids(run, (24, 8), 6)?;
    go_meeting_raids(run, (17, 5), 8)?;
    // West bank: the grave marker, the refugees' cart, the herbalist's road, Reedmere (the marsh folk's plea,
    // the family brought home), then the wraiths. The chapter's last walk: the host comes to the hero there.
    go_meeting_raids(run, (12, 14), 6)?;
    go_meeting_raids(run, (11, 25), 8)?;
    go_meeting_raids(run, (13, 31), 6)?;
    go_meeting_raids(run, (9, 38), 6)?;
    go_meeting_raids(run, (6, 38), 6)?;
    go_meeting_raids(run, (6, 44), 8)?;
    // The herbalist's salve, drunk where the wounded stand.
    super::playthrough_ch23::drink_for_wounded(run);
    // The hero waits for the host, meeting the raids that come.
    meet_host_and_raids(run, 30)
}

/// Waits up to `days` days for the host, meeting the raids that come within eight cells of the hero and
/// stepping out and back to the host when it is near (the way [`meet_host`] does).
pub(crate) fn meet_host_and_raids(run: &mut Run, days: u64) -> Result<(), Stuck> {
    for _ in 0..days {
        run.wait_days(1)?;
        if run.done() {
            return Ok(());
        }
        intercept_raids(run, 8)?;
        if host_near(run) {
            step_out_and_back(run)?;
            if run.done() {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// The test player's hiring for [`side_quests_route`]: every unit the gold buys, keeping `keep` gold.
fn hire_all_keeping(run: &mut Run, keep: i32) {
    let budget = run.g.gold - keep;
    run.hire_all(budget);
}

/// The install of the tests, or `None` (the test is skipped) without `RAZDOR_DT_DIR`.
pub(crate) fn install() -> Option<DtInstall> {
    let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
    Some(DtInstall::load(Path::new(&dir)).unwrap_or_else(|e| panic!("{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::refs::DtKind;
    use crate::rules::content::UnitId;

    fn fight_against(run: &Run, enemy: &str) -> Option<Fight> {
        run.battles.iter().find(|b| b.enemy == enemy).cloned()
    }

    /// The squad members of unit type `alias`.
    fn count_of(run: &Run, alias: &str) -> usize {
        let id = crate::pack::aliases::resolve(DtKind::Unit, alias).expect("the alias exists");
        run.g.squad.iter().filter(|u| u.def == UnitId(id)).count()
    }

    #[test]
    fn the_prepared_hero_beats_the_wraiths_and_gets_the_hunters() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, PREPARED).unwrap_or_else(|e| panic!("{e}"));
        prepared_to_wraiths(&mut run).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert_eq!(fight_against(&run, "wraiths").map(|b| b.outcome), Some(Outcome::Victory), "{:?}", run.log);
        assert_eq!(count_of(&run, "hunter"), 2, "the marsh folk's two hunters join: {:?}", run.log);
        assert!(run.log.iter().any(|l| l.contains("dialog marsh_thanks")), "{:?}", run.log);
        assert!(run.log.iter().any(|l| l.contains("hired")), "the tower's recruits were hired: {:?}", run.log);
    }

    #[test]
    fn the_start_army_loses_to_the_wraiths() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, PREPARED).unwrap_or_else(|e| panic!("{e}"));
        let stuck = unprepared_to_wraiths(&mut run).expect_err("the wraiths beat the start army");
        assert!(matches!(stuck, Stuck::Lost { ref enemy, .. } if enemy == "wraiths"), "{stuck}");
    }

    #[test]
    fn the_toll_fight_starts_a_battle() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, &[("toll", "fight")]).unwrap_or_else(|e| panic!("{e}"));
        let stuck = run.walk_to((33, 23), 10).expect_err("the start army loses the toll fight");
        assert!(matches!(stuck, Stuck::Lost { ref enemy, .. } if enemy == "toll"), "{stuck}");
        assert_eq!(run.battles.len(), 1, "{:?}", run.log);
        assert!(run.log.iter().any(|l| l.contains("dialog toll -> fight")), "{:?}", run.log);
    }

    /// The day of a log line (`day  4.23: …`).
    fn day_of(line: &str) -> f64 {
        line.trim_start_matches("day").trim().split(':').next().and_then(|d| d.trim().parse().ok()).unwrap_or(f64::NAN)
    }

    /// The first log line that says `what`, with its day.
    fn logged(run: &Run, what: &str) -> Option<f64> {
        run.log.iter().find(|l| l.contains(what)).map(|l| day_of(l))
    }

    /// Whether the diary has the package quest `id` active (`Quest`) or completed (`Completed`).
    fn quest_active(run: &Run, id: &str) -> bool {
        run.g.journal.entries.iter().any(|e| e.kind == crate::rules::journal::EntryKind::Quest && e.story.as_deref() == Some(id))
    }

    fn quest_done(run: &Run, id: &str) -> bool {
        run.g.journal.entries.iter().any(|e| e.kind == crate::rules::journal::EntryKind::Completed && e.story.as_deref() == Some(id))
    }

    /// The arrival opens the main quest «Удержать брод» and the tower's sub-quest in the diary.
    #[test]
    fn the_arrival_opens_the_main_quest_and_the_tower_in_the_diary() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let run = Run::new(&opened, content, 1, &[]).unwrap_or_else(|e| panic!("{e}"));
        assert!(quest_active(&run, "hold_ford"), "the main quest from the arrival");
        assert!(quest_active(&run, "tower"), "the tower's sub-quest from the arrival");
        assert!(run.g.journal.entries.iter().any(|e| e.story.as_deref() == Some("arrival")), "the writ is in «Сообщения»");
    }

    /// The host rises at the midnight after its warning (Dread 5): the warning's day and the host's day
    /// are consecutive midnights, on the main route.
    #[test]
    fn the_host_rises_the_midnight_after_the_warning_at_dread_five() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, PREPARED).unwrap_or_else(|e| panic!("{e}"));
        side_quests_route(&mut run, "leave", hire_all_keeping).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        let warned = logged(&run, "dialog host_warning").expect("the warning comes");
        let rose = logged(&run, "dialog host_rises").expect("the host rises");
        eprintln!("warning on day {warned:.2}, host on day {rose:.2}");
        assert_eq!(rose.floor(), warned.floor() + 1.0, "the host rises the midnight after the warning: {:?}", run.log);
    }

    /// The main route meets the raids as the Dread brings them: the first when Dread reaches 2 (Varga's
    /// deed), the second after the first is beaten (Dread 4), both beaten, then the host and the victory.
    #[test]
    fn the_main_route_meets_two_raids_that_the_dread_brings() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, PREPARED).unwrap_or_else(|e| panic!("{e}"));
        side_quests_route(&mut run, "leave", hire_all_keeping).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        let first = logged(&run, "dialog raid_sent").expect("the first raid is sent");
        assert!(first < 1.0, "the first raid comes with Dread 2, on the first day: day {first:.2}");
        let beaten_1 = logged(&run, "battle vs raid_1: Victory").expect("raid 1 is beaten");
        let second = run.log.iter().filter(|l| l.contains("dialog raid_sent")).nth(1).map(|l| day_of(l)).expect("the second raid is sent");
        assert!(second >= beaten_1 - 0.01, "the second raid comes at Dread 4, after the first is beaten: {second:.2} vs {beaten_1:.2}");
        assert!(logged(&run, "battle vs raid_2: Victory").is_some(), "{:?}", run.log);
        assert!(logged(&run, "dialog rider").is_some() && run.victory(), "the rider hands the March over: {:?}", run.log);
    }

    /// The diary tracks the chapter: the sub-quests and the raids are completed as the beats are played, and
    /// the main quest is done at the victory.
    #[test]
    fn the_diary_tracks_the_chapter_beat_by_beat() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, PREPARED).unwrap_or_else(|e| panic!("{e}"));
        side_quests_route(&mut run, "leave", hire_all_keeping).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        for id in ["hold_ford", "tower", "varga", "toll", "refugees", "deserters", "marsh", "barrow", "raid_1", "raid_2"] {
            assert!(quest_done(&run, id), "the quest {id} is completed: {:?}", run.g.journal.entries.iter().map(|e| (e.story.clone(), e.kind)).collect::<Vec<_>>());
        }
        for dialog in ["farm", "tower_log", "stones", "scout", "deserter_note", "herb", "marsh_thanks", "host_warning", "rider"] {
            assert!(run.log.iter().any(|l| l.contains(&format!("dialog {dialog} "))), "the beat {dialog} is played: {:?}", run.log);
        }
        assert!(run.g.journal.entries.iter().any(|e| e.story.as_deref() == Some("host_rises")), "the host's dialog is in «Сообщения»");
    }

    /// The main route has a beat (a dialog, a capture or an encounter) at most eight cells apart, from the start to
    /// the host's fight and the tower after it: the walking between beats is what the owner called «the empty map».
    /// The gap is measured in the cells the hero stood on (`Run::path`), the beats by their index in it.
    #[test]
    fn the_main_route_has_a_beat_every_eight_cells() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, PREPARED).unwrap_or_else(|e| panic!("{e}"));
        side_quests_route(&mut run, "leave", hire_all_keeping).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        let mut marks = vec![0usize];
        marks.extend(run.beats.iter().copied());
        marks.push(run.path.len() - 1);
        marks.sort_unstable();
        let mut gaps: Vec<(usize, usize, usize)> = marks.windows(2).map(|w| (w[1] - w[0], w[0], w[1])).collect();
        gaps.sort_unstable_by(|a, b| b.cmp(a));
        let worst = gaps[0];
        eprintln!("the main route: {} cells, {} beats, the longest gap {} cells (path {}..{} = {:?} to {:?})", run.path.len() - 1, run.beats.len(), worst.0, worst.1, worst.2, run.path[worst.1], run.path[worst.2]);
        assert!(worst.0 <= 8, "the longest walk between beats is {} cells (path {}..{}): {:?}\ncells: {:?}", worst.0, worst.1, worst.2, run.log, &run.path[worst.1..=worst.2]);
    }

    /// The raids come with Dread, not with days: the first on the route's first day at Varga's deed, not on
    /// day four. A raid nobody meets takes the watchtower the hero holds: the loss clears its flag, the raid
    /// stands in the tower, and the dialog says so.
    #[test]
    fn a_raid_nobody_meets_takes_the_watchtower() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, PREPARED).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((44, 14), 10).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((44, 14), 3).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((32, 17), 5).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        run.walk_to((40, 28), 3).unwrap_or_else(|e| panic!("{e}"));
        run.wait_days(4).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert_eq!(run.var("tower_taken"), Some(VarValue::Bool(false)), "{:?}", run.log);
        assert!(logged(&run, "dialog raid_tower").is_some_and(|d| (0.0..4.0).contains(&d)), "{:?}", run.log);
    }

    /// A raid nobody meets burns Reedmere when the hero holds no tower: the village's ash counts and the marsh
    /// folk trust the hero less. The second Dread step (the toll talked away) sends it.
    #[test]
    fn a_raid_nobody_meets_burns_reedmere() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, PREPARED).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((32, 17), 5).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((29, 23), 6).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        // The hero walks off the road: an AI army never attacks a hero who stands still (world.md §4.3).
        run.walk_to((40, 28), 3).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        run.wait_days(8).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert_eq!(run.var("villages_burned"), Some(VarValue::Int(1)), "{:?}", run.log);
        assert_eq!(run.var("marsh_trust"), Some(VarValue::Int(-1)), "{:?}", run.log);
        assert!(logged(&run, "dialog raid_village").is_some(), "{:?}", run.log);
        assert!(quest_done(&run, "raid_1"), "the raid's quest is closed with its loss");
    }

    /// Breaking the barrow mound leaves the host one unit weaker: at its rising it has a ghoul alone.
    #[test]
    fn breaking_the_barrow_mound_leaves_the_host_one_unit() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, &[("barrow", "break")]).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((44, 14), 10).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((44, 14), 3).unwrap_or_else(|e| panic!("{e}"));
        hire_all_keeping(&mut run, 0);
        run.walk_to((41, 6), 3).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert_eq!(run.var("barrow_broken"), Some(VarValue::Bool(true)), "{:?}", run.log);
        assert!(run.battles.iter().any(|b| b.enemy == "barrow_mound" && b.outcome == Outcome::Victory), "{:?}", run.log);
        // The host comes when the Dread reaches five (the weekly backstop, here): its first day, at the midnight.
        let mut host_units = None;
        for _ in 0..40 {
            run.wait_days(1).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
            if let Some((_, a)) = run.g.world.armies.iter().enumerate().find(|(i, _)| run.army_name(*i) == "host") {
                host_units = Some(a.troops.len());
                break;
            }
            if run.done() {
                break;
            }
        }
        assert_eq!(host_units, Some(1), "the host has one unit fewer: {:?}", run.log);
    }

    /// Sealing the barrow costs forty gold and one Dread (at least none): the rite is a deed the other way.
    #[test]
    fn sealing_the_barrow_costs_forty_gold_and_a_dread() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, &[("barrow", "seal")]).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((44, 14), 10).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((44, 14), 3).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(run.var("dread"), Some(VarValue::Int(1)), "the watchtower: one Dread");
        let gold = run.g.gold;
        run.walk_to((41, 6), 3).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert_eq!(run.var("barrow_sealed"), Some(VarValue::Bool(true)), "{:?}", run.log);
        assert_eq!(run.g.gold, gold - 40, "the rite costs forty gold");
        assert_eq!(run.var("dread"), Some(VarValue::Int(0)), "the rite lowers Dread by one");
    }

    /// The deserters of the north woods serve for thirty gold (two swordsmen join) or the Crown has them (its
    /// standing rises).
    #[test]
    fn the_deserters_serve_for_gold_or_go_to_the_crown() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        // Varga and the toll stay out of it: the squad grows by the two deserters only.
        let choices = [("deserters", "serve"), ("varga_meet", "refuse"), ("toll", "talk")];
        let mut run = Run::new(&opened, content.clone(), 1, &choices).unwrap_or_else(|e| panic!("{e}"));
        let (gold, squad) = (run.g.gold, run.g.squad.len());
        run.walk_to((17, 5), 8).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert_eq!(run.g.gold, gold - 30, "thirty gold");
        assert_eq!(run.g.squad.len(), squad + 2, "two join the squad");
        assert_eq!(run.var("deserters"), Some(VarValue::Int(1)));

        let mut run = Run::new(&opened, content, 1, &[("deserters", "hand"), ("varga_meet", "refuse"), ("toll", "talk")]).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((17, 5), 8).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert_eq!(run.var("crown"), Some(VarValue::Int(1)), "the Crown's standing rises");
        assert_eq!(run.var("deserters"), Some(VarValue::Int(2)));
    }

    /// The refugees taken to Reedmere are thanked there: the marsh folk trust the hero more and he gets gold.
    #[test]
    fn the_refugees_taken_home_are_thanked_at_reedmere() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, &[("refugees", "take"), ("marsh_plea", "ride_on")]).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((11, 25), 6).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert_eq!(run.var("refugees"), Some(VarValue::Bool(true)), "{:?}", run.log);
        assert!(quest_active(&run, "refugees"), "the refugees' quest runs until they are home");
        let gold = run.g.gold;
        run.walk_to((9, 38), 6).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert!(logged(&run, "dialog refugees_thanks").is_some(), "{:?}", run.log);
        assert_eq!(run.var("refugees_done"), Some(VarValue::Bool(true)));
        assert_eq!(run.g.gold, gold + 15, "fifteen coins from the sack");
        assert!(quest_done(&run, "refugees"));
    }

    /// The tower heals the squad for a fee while the hero holds it (the church-and-barracks rule: its hire tab).
    #[test]
    fn the_tower_heals_the_squad_for_a_fee() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, PREPARED).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((44, 14), 10).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to((44, 14), 3).unwrap_or_else(|e| panic!("{e}"));
        run.g.squad[0].hp = 5;
        run.g.gold = 300;
        let gold = run.g.gold;
        let healed = crate::pack::playthrough_campaign::heal_within_means(&mut run);
        assert!(healed >= 1, "the tower heals a wounded unit: {:?}", run.log);
        assert!(run.g.squad[0].hp > 5, "the unit's hp rises");
        assert!(run.g.gold < gold, "the healing costs gold");
    }

    /// The herbalist sells her salve for fifteen gold: a healing potion joins the pack.
    #[test]
    fn the_herbalist_sells_a_healing_potion() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, &[("herb", "buy"), ("toll", "talk"), ("varga_meet", "refuse")]).unwrap_or_else(|e| panic!("{e}"));
        let (gold, potions) = (run.g.gold, run.g.pack.count());
        run.walk_to((13, 31), 6).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert!(logged(&run, "dialog herb -> buy").is_some(), "{:?}", run.log);
        assert_eq!(run.g.gold, gold - 15, "fifteen gold");
        assert_eq!(run.g.pack.count(), potions + 1, "one potion in the pack");
    }

    /// Chapter one won on the prepared path: the host rises (the weekly backstop brings Dread to five), comes for
    /// the hero and is beaten, and the chapter's victory fires with the rider's hand-over.
    #[test]
    fn chapter_one_is_won_on_the_prepared_path() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, PREPARED).unwrap_or_else(|e| panic!("{e}"));
        prepared_to_wraiths(&mut run).unwrap_or_else(|e| panic!("{e}"));
        meet_host(&mut run, 40).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert!(run.victory(), "{:?}", run.log);
        assert!(logged(&run, "dialog rider").is_some(), "{:?}", run.log);
    }

    /// The prepared and the unprepared routes over 30 seeds (the game's generator, the story's
    /// rolls and the events' draws). Battles have no randomness, so each route's rate is 0 or 100
    /// unless the seed changes the walk; the sweep reports the rates.
    #[test]
    fn the_wraiths_fight_over_thirty_seeds() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let started = std::time::Instant::now();
        let (mut won_prepared, mut lost_unprepared) = (0, 0);
        for seed in 1..=30u32 {
            let mut run = Run::new(&opened, content.clone(), seed, PREPARED).unwrap_or_else(|e| panic!("{e}"));
            if prepared_to_wraiths(&mut run).is_ok() && fight_against(&run, "wraiths").is_some_and(|b| b.outcome == Outcome::Victory) {
                won_prepared += 1;
            }
            let mut run = Run::new(&opened, content.clone(), seed, PREPARED).unwrap_or_else(|e| panic!("{e}"));
            if unprepared_to_wraiths(&mut run).is_err() && fight_against(&run, "wraiths").is_some_and(|b| b.outcome == Outcome::Defeat) {
                lost_unprepared += 1;
            }
        }
        eprintln!("wraiths over 30 seeds: prepared won {won_prepared}/30, start army lost {lost_unprepared}/30, in {:.2}s", started.elapsed().as_secs_f64());
        assert!(won_prepared * 100 >= 75 * 30, "prepared won {won_prepared}/30");
        assert!(lost_unprepared * 100 >= 60 * 30, "start army lost {lost_unprepared}/30");
    }

    /// The prepared route (the wraiths beaten, then the host met) wins the chapter in at least 70% of 30 seeds.
    #[test]
    fn the_host_over_thirty_seeds() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let started = std::time::Instant::now();
        let mut won_prepared = 0;
        for seed in 1..=30u32 {
            let mut run = Run::new(&opened, content.clone(), seed, PREPARED).unwrap_or_else(|e| panic!("{e}"));
            if prepared_to_wraiths(&mut run).is_ok() && meet_host(&mut run, 40).is_ok() && run.victory() {
                won_prepared += 1;
            }
        }
        eprintln!("host over 30 seeds: prepared won {won_prepared}/30 in {:.2}s", started.elapsed().as_secs_f64());
        assert!(won_prepared * 100 >= 70 * 30, "prepared won {won_prepared}/30");
    }

    /// The side-quest route wins chapter one in at least 70% of fifteen seeds, with the barrow sealed or left
    /// alone. (The break is not in the sweep: its fight leaves the squad too hurt for the raids after it.)
    #[test]
    fn the_side_quests_route_wins_chapter_one_over_fifteen_seeds() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        for barrow in ["seal", "leave"] {
            let mut won = 0;
            let mut fails = Vec::new();
            for seed in 1..=15u32 {
                let mut run = Run::new(&opened, content.clone(), seed, PREPARED).unwrap_or_else(|e| panic!("{e}"));
                match side_quests_route(&mut run, barrow, hire_all_keeping) {
                    Ok(()) if run.victory() => won += 1,
                    Ok(()) => fails.push(format!("{seed}: not won")),
                    Err(e) => fails.push(format!("{seed}: {e}")),
                }
            }
            eprintln!("side quests ({barrow}) won {won}/15: {fails:?}");
            assert!(won * 100 >= 70 * 15, "{barrow}: won {won}/15: {fails:?}");
        }
    }

    #[test]
    fn a_run_starts_with_the_arrival_dialog_answered() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let run = Run::new(&opened, content, 1, &[]).unwrap_or_else(|e| panic!("{e}"));
        assert!(run.log.iter().any(|l| l.contains("dialog arrival")), "{:?}", run.log);
        assert!(!run.done());
    }
}

