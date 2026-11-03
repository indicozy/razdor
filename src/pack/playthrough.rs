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
use crate::rules::sim::fight_it_out;
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
    start: f64,
}

impl Run {
    /// A game of `opened` (its story seeded with `seed`, the game's generator at `seed`), the map
    /// revealed (the planner needs explored cells; no route waits for the fog), its opening moment
    /// answered. Campaign-free: the package's first map.
    pub fn new(opened: &Opened, content: Arc<Content>, seed: u32, choices: &[(&str, &str)]) -> Result<Run, Stuck> {
        let mut o = opened.clone();
        o.story.seed = seed | 1;
        let mut g = Game::from_scenario(content, &o.scenario, HeroClass::Knight);
        g.rng = Rng::new(seed);
        g.seed_events(seed);
        cheats::run(&Cheat::Reveal, Some(&mut g), None).expect("reveal the map");
        g.triggers.trace = Some(Vec::new());
        o.start(&mut g);
        let start = g.clock.total_minutes();
        let mut run = Run { g, log: Vec::new(), choices: choices.iter().map(|(d, c)| (d.to_string(), c.to_string())).collect(), battles: Vec::new(), start };
        run.service()?;
        Ok(run)
    }

    /// The day after the start.
    pub fn day(&self) -> f64 {
        (self.g.clock.total_minutes() - self.start) / MINUTES_PER_DAY as f64
    }

    /// The chapter is over (a victory, a defeat or an ending).
    pub fn done(&self) -> bool {
        self.g.story_end().is_some() || self.g.script_end().is_some()
    }

    /// The story's victory was reached.
    pub fn victory(&self) -> bool {
        self.g.story_end() == Some(Ending::Victory)
    }

    /// The story's value of variable `name` (`None` when it is not declared).
    pub fn var(&self, name: &str) -> Option<VarValue> {
        self.g.story_var(name)
    }

    fn note(&mut self, what: impl AsRef<str>) {
        self.log.push(format!("day {:>5.2}: {}", self.day(), what.as_ref()));
    }

    /// Answers what is waiting: a question (yes), a battle the hero is in, the events the game
    /// has raised (dialogs, captures, encounters). Runs until nothing waits.
    pub fn service(&mut self) -> Result<(), Stuck> {
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

    /// Answers the dialogs among `events` (and those the answers open), notes the captures and the
    /// encounters.
    fn settle(&mut self, events: Vec<Event>) -> Result<(), Stuck> {
        let mut queue: VecDeque<Event> = events.into();
        while let Some(e) = queue.pop_front() {
            match e {
                Event::Package(d) => {
                    let choice = self.choice_for(&d)?;
                    self.note(format!("dialog {} -> {}", d.id, choice.as_deref().unwrap_or("(next)")));
                    queue.extend(self.g.answer_story_dialog(&d.id, choice.as_deref()));
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
        let outcome = fight_it_out(&mut self.g, &mut Vec::new());
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
        for _ in 0..2_000_000 {
            self.service()?;
            if self.done() {
                return Ok(());
            }
            if self.g.foe.is_none() && !self.g.moving() {
                if self.g.tile() == to || self.battles.len() > before {
                    return Ok(());
                }
                if self.g.clock.total_minutes() >= deadline {
                    return Err(Stuck::Late { to, day: self.day() });
                }
                if !self.g.set_destination(to) {
                    return Err(Stuck::NoRoute { to, day: self.day() });
                }
                self.note(format!("walks to {to:?}"));
            }
            let ev = self.g.tick(STEP_SECONDS);
            self.settle(ev)?;
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

/// The chapter «Брод» of the reference package, opened on `install` (skipped without
/// `RAZDOR_DT_DIR`): its content and its opened map, shared by every playthrough.
pub(crate) fn brod(install: &DtInstall) -> (Opened, Arc<Content>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
    let opened = open(&root, Some("brod"), "en", install).unwrap_or_else(|e| panic!("{e}"));
    let content = Arc::new(Content::from_dt(install).with_package(opened.units.clone(), opened.items.clone()));
    (opened, content)
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

    /// The choices the prepared hero makes: Varga joins, the toll is talked away, the marsh folk
    /// are helped.
    const PREPARED: &[(&str, &str)] = &[("varga_meet", "join"), ("toll", "talk"), ("marsh_plea", "help")];

    /// The prepared route up to the wraiths: the tower taken and garrisoned from its recruits,
    /// Varga met, the toll talked away, the marsh folk helped, then the wraiths fought.
    fn prepared_to_wraiths(run: &mut Run) -> Result<(), Stuck> {
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
    fn meet_host(run: &mut Run, days: u64) -> Result<(), Stuck> {
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

    #[test]
    fn nothing_attacks_the_hero_before_the_second_week() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, PREPARED).unwrap_or_else(|e| panic!("{e}"));
        run.wait_days(20).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert!(run.battles.iter().all(|b| b.day >= 14.0), "{:?}", run.battles);
    }

    /// Chapter 1 won on the prepared path: the host rises at the ford, comes for the hero and is
    /// beaten, and the chapter's victory fires.
    #[test]
    fn chapter_one_is_won_on_the_prepared_path() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let mut run = Run::new(&opened, content, 1, PREPARED).unwrap_or_else(|e| panic!("{e}"));
        prepared_to_wraiths(&mut run).unwrap_or_else(|e| panic!("{e}"));
        meet_host(&mut run, 30).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert!(run.victory(), "{:?}", run.log);
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

    /// The host over 30 seeds: the prepared route (the wraiths beaten, then the host met) wins the
    /// chapter, and the start army, waiting at its start (40, 28) for the host, loses to it.
    #[test]
    fn the_host_over_thirty_seeds() {
        let Some(install) = install() else { return };
        let (opened, content) = brod(&install);
        let started = std::time::Instant::now();
        let (mut won_prepared, mut lost_unprepared) = (0, 0);
        for seed in 1..=30u32 {
            let mut run = Run::new(&opened, content.clone(), seed, PREPARED).unwrap_or_else(|e| panic!("{e}"));
            if prepared_to_wraiths(&mut run).is_ok() && meet_host(&mut run, 30).is_ok() && run.victory() {
                won_prepared += 1;
            }
            let mut run = Run::new(&opened, content.clone(), seed, PREPARED).unwrap_or_else(|e| panic!("{e}"));
            if meet_host(&mut run, 30).is_err() && fight_against(&run, "host").is_some_and(|b| b.outcome == Outcome::Defeat) {
                lost_unprepared += 1;
            }
        }
        eprintln!("host over 30 seeds: prepared won {won_prepared}/30, start army lost {lost_unprepared}/30, in {:.2}s", started.elapsed().as_secs_f64());
        assert!(won_prepared * 100 >= 70 * 30, "prepared won {won_prepared}/30");
        assert!(lost_unprepared * 100 >= 60 * 30, "start army lost {lost_unprepared}/30");
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
