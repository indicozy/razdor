//! Razdor Studio's battle tester, on the real engine: two armies (a map's army, a garrison, the
//! hero's squad, or a custom list of unit types) fought N times over seeds 1..N by the game's
//! battle engine, both sides played by the battle AI as the game's auto battle is. The custom
//! sides are the custom battle's own setup (`rules::custom`); the map's sides are placed as the
//! game places them (`Battle::new` with the units' cells).
//!
//! The battle has no random draws (its damage is exact, and the AI's search is deterministic), so
//! a seed does not change a battle of fixed armies: every seed fights the same battle. The runs
//! report that (`Summary::same_every_seed`) rather than inventing a spread.
//!
//! The map's world is opened by [`open_map`] (the package compiled as the sim compiles it), on the
//! thread that uses it: a [`Game`] is not sent between threads.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::dt::install::DtInstall;
use crate::pack::diag::Report;
use crate::pack::load::Package;
use crate::pack::{compile, open};
use crate::rules::ai;
use crate::rules::battle::{Battle, Outcome, Team};
use crate::rules::content::{Content, HeroClass};
use crate::rules::custom::{self, Control, Pick, Setup};
use crate::rules::formation::{Row, Slot};
use crate::rules::game::{troop_unit, Game};
use crate::rules::units::Unit;

/// Where a side's units come from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// The map's army at this index of the world's armies (its living troops).
    Army(usize),
    /// The map's garrison of the building at this index of the world's locations.
    Garrison(usize),
    /// The hero's squad as the map sets it up (the hero first).
    Squad,
    /// Unit types with their levels, as the custom battle picks them.
    Custom(Vec<Pick>),
}

/// The two sides: `a` is the player's side of the battle (below), `b` the enemy's.
#[derive(Clone, Debug, PartialEq)]
pub struct Matchup {
    pub a: Source,
    pub b: Source,
}

/// A map opened for the testers: its content (the install's with the package's units), its
/// game at the start, and the hero class the game starts with.
pub struct MapGame {
    pub content: Arc<Content>,
    pub game: Game,
}

/// Opens `map` of `pkg` as the sim does (compiled and opened, the hero's class and the package's
/// units), and its game at the start.
pub fn open_map(pkg: &Package, install: &DtInstall, map: &str) -> Result<MapGame, Report> {
    let compiled = compile::compile(pkg, install, "en", map)?;
    let opened = open::build(pkg, open::package_hash(&pkg.files), std::path::Path::new(""), map, compiled, "en").map_err(|e| {
        let mut r = Report::default();
        r.error("", None, e);
        r
    })?;
    let class = opened.scenario.header.first_offered_class().map_or(HeroClass::Knight, |k| HeroClass::ALL[k]);
    let content = Arc::new(Content::from_dt(install).with_package(opened.units.clone(), opened.items.clone()));
    let game = Game::from_scenario(content.clone(), &opened.scenario, class);
    Ok(MapGame { content, game })
}

/// The kinds of choice a side's dropdown offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Army,
    Garrison,
    Squad,
    Custom,
}

/// One choice of a side: its kind, the army's or building's name (empty when it has none), its
/// living troops, and the source it stands for.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub kind: Kind,
    pub name: String,
    pub troops: usize,
    pub source: Source,
}

/// The choices the map offers a side: its armies with living troops, its garrisons, and the
/// hero's squad.
pub fn map_sources(g: &Game) -> Vec<Choice> {
    let mut out = Vec::new();
    for (i, a) in g.world.armies.iter().enumerate() {
        let troops = a.troops.iter().filter(|t| t.alive()).count();
        if troops > 0 {
            out.push(Choice { kind: Kind::Army, name: a.name.clone(), troops, source: Source::Army(i) });
        }
    }
    for (l, loc) in g.world.locations.iter().enumerate() {
        let troops = loc.garrison.iter().filter(|t| t.alive()).count();
        if troops > 0 {
            out.push(Choice { kind: Kind::Garrison, name: loc.name.clone(), troops, source: Source::Garrison(l) });
        }
    }
    out.push(Choice { kind: Kind::Squad, name: String::new(), troops: g.squad.iter().filter(|u| u.alive()).count(), source: Source::Squad });
    out
}

/// The units of one side: each with its squad index (the hero is index 0; the custom and the
/// army sides start at 1), and the building defence the side fights with.
fn side(content: &Arc<Content>, g: Option<&Game>, s: &Source) -> Result<(Vec<(usize, Unit)>, i32), String> {
    let map = || g.ok_or_else(|| "this side needs the map".to_string());
    match s {
        Source::Army(i) => {
            let g = map()?;
            let army = g.world.armies.get(*i).ok_or_else(|| format!("no army {i} on the map"))?;
            let units: Vec<(usize, Unit)> = ai::army_units(content, army).into_iter().enumerate().map(|(k, u)| (k + 1, u)).collect();
            Ok((units, 0))
        }
        Source::Garrison(l) => {
            let g = map()?;
            let loc = g.world.locations.get(*l).ok_or_else(|| format!("no building {l} on the map"))?;
            let units: Vec<(usize, Unit)> = loc.garrison.iter().filter(|t| t.alive()).map(|t| troop_unit(content, t)).enumerate().map(|(k, u)| (k + 1, u)).collect();
            Ok((units, loc.garrison_defence))
        }
        Source::Squad => {
            let g = map()?;
            Ok((g.squad.iter().enumerate().filter(|(_, u)| u.alive()).map(|(i, u)| (i, u.clone())).collect(), 0))
        }
        Source::Custom(picks) => {
            let start = Slot::new(Row::Front, 0);
            Ok((picks.iter().enumerate().map(|(k, p)| (k + 1, custom::unit(content, p, start))).collect(), 0))
        }
    }
}

/// The battle of `m` on `content` (`g` is the map's game, needed by the map's sides): both sides
/// placed, the battle AI at the improved level or not, not begun. Two custom sides are the custom
/// battle's own setup, so they fight as the custom battle does.
pub fn build(content: &Arc<Content>, g: Option<&Game>, m: &Matchup, improved: bool) -> Result<Battle, String> {
    if let (Source::Custom(a), Source::Custom(b)) = (&m.a, &m.b) {
        let setup = Setup { armies: [a.clone(), b.clone()], control: [Control::Ai, Control::Ai], wide: content.formation == crate::rules::formation::Formation::WIDE };
        if !setup.ready() {
            return Err("an army is empty".into());
        }
        return setup.battle(content, improved).ok_or_else(|| "an army is empty".to_string());
    }
    let (pa, da) = side(content, g, &m.a)?;
    let (pb, db) = side(content, g, &m.b)?;
    if pa.is_empty() || pb.is_empty() {
        return Err("an army is empty".into());
    }
    let player: Vec<(usize, &Unit)> = pa.iter().map(|(i, u)| (*i, u)).collect();
    let enemies: Vec<Unit> = pb.into_iter().map(|(_, u)| u).collect();
    let mut battle = Battle::new(content.clone(), &player, &enemies, Team::Player);
    battle.set_improved_ai(improved);
    if matches!(m.a, Source::Custom(_)) {
        battle.auto_arrange(Team::Player);
    }
    if matches!(m.b, Source::Custom(_)) {
        battle.auto_arrange(Team::Enemy);
    }
    if da > 0 {
        battle.set_building_defence(Team::Player, da);
    }
    if db > 0 {
        battle.set_building_defence(Team::Enemy, db);
    }
    Ok(battle)
}

/// The losses of one side of a battle: units that died, and the HP the side lost (its units'
/// HP at the start less what they have at the end, the dead at 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Losses {
    pub units: u32,
    pub hp: i64,
}

/// One battle of the tester.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub seed: u64,
    /// Who won (`Team::Player` is side A); `None` when the battle did not end (the step cap).
    pub winner: Option<Team>,
    pub rounds: u32,
    pub a: Losses,
    pub b: Losses,
}

/// A side's HP now (the dead at 0) and how many of its units are dead.
fn tally(b: &Battle, team: Team) -> (i64, u32) {
    let mut hp = 0i64;
    let mut dead = 0u32;
    for f in b.fighters.iter().filter(|f| f.team == team) {
        hp += i64::from(f.hp.max(0));
        if !f.alive() {
            dead += 1;
        }
    }
    (hp, dead)
}

/// Fights `b` to its end with the battle AI on both sides and reads the result.
pub fn fight(mut b: Battle, seed: u64) -> Run {
    let (hp_a0, _) = tally(&b, Team::Player);
    let (hp_b0, _) = tally(&b, Team::Enemy);
    let outcome = b.auto_play_to_end();
    let (hp_a, dead_a) = tally(&b, Team::Player);
    let (hp_b, dead_b) = tally(&b, Team::Enemy);
    let winner = match outcome {
        Outcome::Victory => Some(Team::Player),
        Outcome::Defeat => Some(Team::Enemy),
        Outcome::Ongoing => None,
    };
    Run { seed, winner, rounds: b.round, a: Losses { units: dead_a, hp: hp_a0 - hp_a }, b: Losses { units: dead_b, hp: hp_b0 - hp_b } }
}

/// The result of a set of runs: the win counts, the average rounds, and each side's average and
/// spread of losses.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub runs: usize,
    pub a_wins: usize,
    pub b_wins: usize,
    pub draws: usize,
    pub avg_rounds: f64,
    pub a: Spread,
    pub b: Spread,
    /// Every run gave the same result (seed aside): the battle has no random draws.
    pub same_every_seed: bool,
}

/// One side's losses over the runs: the average and the lowest and highest.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spread {
    pub avg_units: f64,
    pub min_units: u32,
    pub max_units: u32,
    pub avg_hp: f64,
    pub min_hp: i64,
    pub max_hp: i64,
}

fn spread(values: impl Iterator<Item = Losses> + Clone) -> Spread {
    let n = values.clone().count();
    if n == 0 {
        return Spread::default();
    }
    let units: Vec<u32> = values.clone().map(|l| l.units).collect();
    let hp: Vec<i64> = values.map(|l| l.hp).collect();
    Spread {
        avg_units: units.iter().map(|&u| f64::from(u)).sum::<f64>() / n as f64,
        min_units: *units.iter().min().unwrap_or(&0),
        max_units: *units.iter().max().unwrap_or(&0),
        avg_hp: hp.iter().map(|&h| h as f64).sum::<f64>() / n as f64,
        min_hp: *hp.iter().min().unwrap_or(&0),
        max_hp: *hp.iter().max().unwrap_or(&0),
    }
}

impl Summary {
    pub fn of(runs: &[Run]) -> Summary {
        let n = runs.len();
        let count = |w: Option<Team>| runs.iter().filter(|r| r.winner == w).count();
        let same = runs.windows(2).all(|p| (p[0].winner, p[0].rounds, p[0].a, p[0].b) == (p[1].winner, p[1].rounds, p[1].a, p[1].b));
        Summary {
            runs: n,
            a_wins: count(Some(Team::Player)),
            b_wins: count(Some(Team::Enemy)),
            draws: count(None),
            avg_rounds: if n == 0 { 0.0 } else { runs.iter().map(|r| f64::from(r.rounds)).sum::<f64>() / n as f64 },
            a: spread(runs.iter().map(|r| r.a)),
            b: spread(runs.iter().map(|r| r.b)),
            same_every_seed: same,
        }
    }

    /// The win share of side A in per cent (0 with no runs).
    pub fn a_percent(&self) -> f64 {
        self.pct(self.a_wins)
    }

    pub fn b_percent(&self) -> f64 {
        self.pct(self.b_wins)
    }

    pub fn draw_percent(&self) -> f64 {
        self.pct(self.draws)
    }

    fn pct(&self, n: usize) -> f64 {
        if self.runs == 0 {
            0.0
        } else {
            100.0 * n as f64 / self.runs as f64
        }
    }
}

/// Runs the battle of `m` for each seed in `seeds` (the first is the first run). Each run builds
/// its battle afresh, so the runs are independent. `progress` gets (done, total) after each run;
/// a set `cancel` stops the runs and gives `None`.
pub fn run_seeds(content: &Arc<Content>, g: Option<&Game>, m: &Matchup, improved: bool, seeds: std::ops::RangeInclusive<u64>, cancel: &AtomicBool, progress: &mut dyn FnMut(usize, usize)) -> Result<Option<Vec<Run>>, String> {
    let total = (seeds.end() + 1).saturating_sub(*seeds.start()) as usize;
    let mut out = Vec::with_capacity(total);
    for seed in seeds {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        // The seed names the run. The battle draws nothing, so it changes no result (see the
        // module's note).
        let battle = build(content, g, m, improved)?;
        out.push(fight(battle, seed));
        progress(out.len(), total);
    }
    Ok(Some(out))
}

/// The battle one seed fights, not begun: what «Посмотреть бой» hands the game's battle view
/// (its watch mode plays both sides by the AI).
pub fn watch_battle(content: &Arc<Content>, g: Option<&Game>, m: &Matchup) -> Result<Battle, String> {
    build(content, g, m, false)
}

/// A custom side from `(unit type id, level)` pairs, as the custom battle builds it.
pub fn custom_side(picks: &[(u32, i32)]) -> Source {
    Source::Custom(picks.iter().map(|&(id, level)| Pick { unit: crate::rules::content::UnitId(id), level, items: Vec::new() }).collect())
}

#[cfg(test)]
mod tests;
