//! The playthrough engine of a content package: a game of a package's map played headless, the way a
//! player plays it. The hero walks with the game's own route planner and clock (`set_destination`,
//! `tick`, `wait`), the AI moves the armies as it does in play, battles are fought by the battle engine
//! (`sim::fight_it_out`), questions are answered yes, and the package's dialogs are answered by the
//! choices the caller gives. Every dialog, battle and capture is logged with its day, so a failure says
//! what happened. Used by the tests in `pack::playthrough*` and by `razdor-pack test` (`pack::playtest`).
//! No macroquad.

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
use crate::rules::content::{Content, HeroClass, WageKind};
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

    pub(crate) fn army_name(&self, i: usize) -> String {
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
pub fn open_map(install: &DtInstall, id: &str) -> (Opened, Arc<Content>) {
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
pub struct Carried {
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

/// The player's hiring: the cheapest offer first, while the purse still holds `RUNWAY` days of the
/// squad's wage bill with the new unit. Returns the names of the units hired.
pub const RUNWAY: i32 = 3;

pub fn hire_within_means(run: &mut Run) -> Vec<String> {
    hire_within_means_keeping(run, 0)
}

/// [`hire_within_means`] with `keep` gold kept back (a rite's price, for one).
pub fn hire_within_means_keeping(run: &mut Run, keep: i32) -> Vec<String> {
    let mut hired = Vec::new();
    loop {
        let bill = run.g.daily_wages();
        let mut offers = run.g.recruits_here();
        offers.sort_by_key(|&k| run.g.hire_price(k).amount);
        let pick = offers.into_iter().find(|&k| {
            let price = run.g.hire_price(k).amount;
            let wage = run.g.content.wage_for(k, WageKind::Recruit);
            run.g.gold - price - keep >= RUNWAY * (bill + wage)
        });
        let Some(kind) = pick else { break };
        let price = run.g.hire_price(kind).amount;
        if run.g.hire(kind).is_err() {
            break;
        }
        let name = run.g.content.unit(kind).name.clone();
        run.note(format!("hired {name} for {price}"));
        hired.push(name);
    }
    hired
}

/// The player's healing at a town's service: each wounded unit is healed while the purse still holds
/// `RUNWAY` days of the wage bill after the payment. Returns the number of units healed.
pub fn heal_within_means(run: &mut Run) -> usize {
    let mut healed = 0;
    for i in 0..run.g.squad.len() {
        let Some(price) = run.g.heal_price(i) else { continue };
        if run.g.gold - price.amount < RUNWAY * run.g.daily_wages() {
            continue;
        }
        if run.g.heal(i).is_ok() {
            healed += 1;
        }
    }
    if healed > 0 {
        run.note(format!("healed {healed} unit(s) at the service"));
    }
    healed
}

/// The player's raising of the dead at a town's service (a town or a church): each fallen unit is raised while
/// the purse still holds `RUNWAY` days of the wage bill after the payment. Returns the number of units raised.
pub fn raise_within_means(run: &mut Run) -> usize {
    let mut raised = 0;
    for i in 0..run.g.squad.len() {
        let Some(price) = run.g.resurrect_price(i) else { continue };
        if run.g.gold - price.amount < RUNWAY * run.g.daily_wages() {
            continue;
        }
        if run.g.resurrect(i).is_ok() {
            raised += 1;
        }
    }
    if raised > 0 {
        run.note(format!("raised {raised} unit(s) at the service"));
    }
    raised
}

/// Drinks a potion of the pack for each wounded living unit, until none is wounded or the pack has
/// no potion left (a player drinks from the army screen).
pub fn drink_for_wounded(run: &mut Run) {
    loop {
        let wounded = (0..run.g.squad.len()).find(|&i| run.g.squad[i].alive() && run.g.squad[i].hp < run.g.squad[i].max_hp(&run.g.content));
        let Some(unit) = wounded else { return };
        let drunk = (0..24).find_map(|k| run.g.drink(unit, k).ok().map(|healed| (k, healed)));
        let Some((_, healed)) = drunk else { return };
        run.log.push(format!("drank a potion: {} +{healed} HP", run.squad_names()[unit]));
    }
}

/// The nearest raid (an army named `raid_*`) within `reach` cells of the hero, if any.
pub(crate) fn raid_near(run: &Run, reach: i32) -> Option<Tile> {
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
