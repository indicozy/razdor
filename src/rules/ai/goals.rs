//! AI army goals and paces, set by a package's trigger actions (`army_goal`, `army_speed`).
//!
//! An army with a goal does not take the original AI's choice of destination. At each arrival
//! it plans its way to the goal's cell with the routes the AI uses ([`super::army_path`] on land,
//! the sea steps for a ship) and walks it on the same step clock, so the speed rules, ships,
//! bridges and the fog behave as before. On arrival it does what the goal says: it stands
//! (`go_to`, `go_to_building`), loops its patrol (`patrol`), or keeps next to its target
//! (`follow`, `attack`; an attack is the arrival's contact, as the AI's own).
//!
//! An army without a goal never reaches this module ([`super::AiMind::goal`] is none), and its
//! pace is 100 (`AiMind::pace` 0, not saved). A goal whose target is gone or cannot be reached
//! drops back to the original AI, and nothing fires. The goal's own arrivals (`go_to`,
//! `go_to_building`, each `patrol` point) are queued as [`Arrival`]s and run at the end of the
//! AI's tick ([`Game::flush_goal_arrivals`]), so a trigger never acts while the AI's loop holds
//! its armies.

use serde::{Deserialize, Serialize};

use super::{army_path, Contact, Defender};
use crate::rules::events::ArmyId;
use crate::rules::game::{Game, HeroCells};
use crate::rules::map::Tile;
use crate::rules::triggers::{When, Trace};

/// Routes the goal planner may search (as the AI's own routes, `game::AI_PATH_NODES`).
const GOAL_NODES: usize = 4000;

/// What an attack or a follow is aimed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalTarget {
    Hero,
    Army(ArmyId),
}

/// Where an army with a goal walks (`army_goal`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Goal {
    /// Walks to a cell, then stands there.
    GoTo(Tile),
    /// Walks into a building (by its scenario id), then stands in it.
    GoToBuilding(u16),
    /// Keeps next to the hero or an army as it moves.
    Follow(GoalTarget),
    /// Loops through its cells, one after another, for ever.
    Patrol(Vec<Tile>),
    /// Walks to the hero or an army and attacks it when next to it.
    Attack(GoalTarget),
}

/// A goal and its progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArmyGoal {
    pub goal: Goal,
    /// The patrol cell it walks to (an index of the patrol's cells); 0 for the other goals.
    #[serde(default)]
    pub leg: usize,
    /// Standing on its target (`go_to`, `go_to_building`): its arrival is announced once.
    #[serde(default)]
    pub done: bool,
}

/// A goal's arrival, queued until the AI's tick is over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arrival {
    pub army: ArmyId,
    pub cell: Tile,
    pub building: Option<u16>,
    /// The game minute of the arrival (whole minutes: the clock's day and time of day are all a
    /// trigger reads).
    pub at: u64,
}

/// The pace of an army at 100 percent is stored as 0 (so a save of an army without one is unchanged).
pub fn unpaced(pace: &u32) -> bool {
    *pace == 0
}

/// The most minutes an army banks at a pace (`AI_BUDGET_CAP` at the normal 100). A slower army
/// banks up to proportionally more: a step is paid in its own pace's minutes, and the largest
/// step (a marsh at the normal pace) would otherwise never fit the bank at 70 percent or less.
pub fn budget_cap(pace: u32) -> f32 {
    crate::rules::world::AI_BUDGET_CAP * 100.0 / pace_of(pace) as f32
}

/// `minutes` at a pace of `pace` percent: `minutes × 100 / pace` (`minutes` itself at 100).
pub fn paced_minutes(minutes: f32, pace: u32) -> f32 {
    if pace == 100 {
        minutes
    } else {
        minutes * 100.0 / pace as f32
    }
}

/// [`paced_minutes`] for centi-minutes.
pub fn paced_centi(centi: i64, pace: u32) -> i64 {
    if pace == 100 {
        centi
    } else {
        centi * 100 / i64::from(pace)
    }
}

/// The pace of a stored one (0 is the normal 100).
pub fn pace_of(stored: u32) -> u32 {
    if stored == 0 {
        100
    } else {
        stored
    }
}

fn adjacent(a: Tile, b: Tile) -> bool {
    (a.0 - b.0).abs() <= 1 && (a.1 - b.1).abs() <= 1
}

/// The index of the next patrol cell after `leg` that is not `here` (at most one loop of the
/// cells; a patrol whose cells all lie here stays on its leg).
fn next_leg(points: &[Tile], leg: usize, here: Tile) -> usize {
    let n = points.len().max(1);
    let mut k = leg;
    for _ in 0..n {
        k = (k + 1) % n;
        if points.get(k).is_some_and(|p| *p != here) {
            return k;
        }
    }
    k
}

impl Game {
    /// Gives army `id` a goal (`army_goal`), or takes its goal away (`None`: `clear`). The army
    /// plans its way at its next arrival; its path is dropped now. Returns false for an unknown
    /// army.
    pub(crate) fn set_army_goal(&mut self, id: ArmyId, goal: Option<Goal>) -> bool {
        let Some(a) = self.army_mut_any(id) else { return false };
        match goal {
            Some(g) => a.mind.goal = Some(ArmyGoal { goal: g, leg: 0, done: false }),
            None => {
                a.mind.goal = None;
                a.mind.countdown = 0;
            }
        }
        a.path.clear();
        true
    }

    /// Sets army `id`'s pace (`army_speed`): its steps take `percent` of their time.
    pub(crate) fn set_army_pace(&mut self, id: ArmyId, percent: u32) -> bool {
        let Some(a) = self.army_mut_any(id) else { return false };
        a.mind.pace = if percent == 100 { 0 } else { percent };
        true
    }

    /// The cell a goal walks to now (None: its target is gone, or the goal is no longer valid).
    fn goal_target(&self, g: &ArmyGoal, hero: &HeroCells) -> Option<Tile> {
        match &g.goal {
            Goal::GoTo(c) => self.world.map.in_bounds(*c).then_some(*c),
            Goal::GoToBuilding(b) => self.world.locations.iter().find(|l| l.id == *b).map(|l| l.tile),
            Goal::Patrol(points) => points.get(g.leg).copied(),
            Goal::Follow(t) | Goal::Attack(t) => match t {
                GoalTarget::Hero => Some(hero.at),
                GoalTarget::Army(id) => self.world.armies.iter().find(|a| a.id == *id).map(|a| a.tile(&self.world.map)),
            },
        }
    }

    /// The route of army `i` from `from` to `to` (empty: none). Ships sail the sea; others take
    /// the AI's land routes.
    fn goal_route(&self, i: usize, from: Tile, to: Tile) -> Vec<Tile> {
        let a = &self.world.armies[i];
        if a.sails() {
            let w = &self.world;
            w.map.path_by(from, to, GOAL_NODES, &|_, n| w.sea_step(n).filter(|&c| c > 0))
        } else {
            army_path(&self.world, a, from, to, GOAL_NODES)
        }
    }

    /// Sets army `i`'s path, with the step weights the AI's planner records for it.
    fn set_goal_path(&mut self, i: usize, path: Vec<Tile>) {
        let grid = self.world.map.grid;
        let here = self.world.armies[i].tile(&self.world.map);
        let a = &mut self.world.armies[i];
        a.mind.no_path = false;
        a.mind.walked = 0;
        let w = &mut a.mind.path_weights;
        if w.len() < path.len() + 1 {
            w.resize(path.len() + 1, 0);
        }
        let mut from = here;
        for (k, &t) in path.iter().enumerate() {
            w[k] = grid.weight(from, t) as u8;
            from = t;
        }
        a.path = path;
    }

    /// Army `i` loses its goal: the original AI chooses its next destination at its next arrival.
    fn drop_goal(&mut self, i: usize) {
        let a = &mut self.world.armies[i];
        a.mind.goal = None;
        a.mind.countdown = 0;
        a.path.clear();
    }

    /// Queues an arrival of army `i` at `cell` (in `building`, for a `go_to_building`).
    fn queue_arrival(&mut self, i: usize, cell: Tile, building: Option<u16>) {
        if self.story.is_none() {
            return;
        }
        let army = self.world.armies[i].id;
        let at = self.clock.total_minutes() as u64;
        self.triggers.arrivals.push(Arrival { army, cell, building, at });
    }

    /// The goal's walk at an arrival of army `uid` (the place of the AI's re-plan, see
    /// [`Game::ai_arrival`]): it announces its arrival, moves its patrol on, and plans the path to
    /// its target when it needs one. A goal with no target, or no route to it, is dropped.
    pub(crate) fn goal_walk(&mut self, uid: u32, hero: &HeroCells) {
        let Some(i) = self.army_by_uid(uid) else { return };
        let Some(mut g) = self.world.armies[i].mind.goal.clone() else { return };
        let here = self.world.armies[i].tile(&self.world.map);
        let Some(mut target) = self.goal_target(&g, hero) else {
            self.drop_goal(i);
            return;
        };
        let reached = match &g.goal {
            Goal::GoToBuilding(b) => self.world.location_covering(here).is_some_and(|l| self.world.locations[l].id == *b),
            Goal::GoTo(_) | Goal::Patrol(_) => here == target,
            Goal::Follow(_) | Goal::Attack(_) => false,
        };
        match g.goal.clone() {
            Goal::Patrol(points) if reached => {
                self.queue_arrival(i, here, None);
                g.leg = next_leg(&points, g.leg, here);
                target = points.get(g.leg).copied().unwrap_or(here);
            }
            Goal::GoTo(_) | Goal::GoToBuilding(_) => {
                if reached && !g.done {
                    let building = if let Goal::GoToBuilding(b) = g.goal { Some(b) } else { None };
                    self.queue_arrival(i, here, building);
                }
                g.done = reached;
            }
            _ => {}
        }
        let stand = here == target
            || match g.goal {
                Goal::GoTo(_) | Goal::GoToBuilding(_) => reached,
                Goal::Follow(_) | Goal::Attack(_) => adjacent(here, target),
                Goal::Patrol(_) => false,
            };
        if stand {
            self.set_goal_path(i, Vec::new());
        } else {
            let a = &self.world.armies[i];
            let kept = a.path.last() == Some(&target) && a.path.first().is_some_and(|&t| adjacent(here, t));
            if !kept {
                let route = self.goal_route(i, here, target);
                if route.is_empty() {
                    self.drop_goal(i);
                    return;
                }
                self.set_goal_path(i, route);
            }
        }
        self.world.armies[i].mind.goal = Some(g);
    }

    /// An attack goal next to its target: an attack on the hero (the arrival rules keep it to the
    /// end of his step, as the AI's own), or a battle with an army, as an AI army's own.
    pub(crate) fn goal_attack(&mut self, uid: u32, hero: &HeroCells) -> Option<Contact> {
        let i = self.army_by_uid(uid)?;
        let target = match &self.world.armies[i].mind.goal.as_ref()?.goal {
            Goal::Attack(t) => *t,
            _ => return None,
        };
        let here = self.world.armies[i].tile(&self.world.map);
        match target {
            GoalTarget::Hero => adjacent(here, hero.at).then_some(Contact::Attack),
            GoalTarget::Army(id) => {
                let map = &self.world.map;
                let j = self.world.armies.iter().position(|a| a.id == id && a.tile(map) != here && adjacent(here, a.tile(map)))?;
                if j != i {
                    self.ai_battle(i, Defender::Army(j));
                }
                None
            }
        }
    }

    /// Runs the `army_arrives` triggers for the arrivals queued in this AI tick, each at its
    /// minute, and hands their dialogs to the tick's events (`ai_events`). Each arrival is traced
    /// (`razdor-pack sim --trace`).
    /// The `building_lost` moments of the buildings the AI took from the player this tick, once
    /// the tick is over (their triggers may move armies, so they do not run inside the AI's loop).
    pub(crate) fn flush_lost_buildings(&mut self) {
        let lost = std::mem::take(&mut self.triggers.lost);
        for b in lost {
            let events = self.story_events(When::BuildingLost(b));
            self.ai_events.extend(events);
        }
    }

    pub(crate) fn flush_goal_arrivals(&mut self) {
        let arrivals = std::mem::take(&mut self.triggers.arrivals);
        if arrivals.is_empty() {
            return;
        }
        let end = self.clock.total_minutes();
        for a in arrivals {
            self.clock.set_total_minutes(a.at as f64);
            let army = self.story().and_then(|s| s.army_names.get(&a.army)).cloned().unwrap_or_else(|| a.army.to_string());
            let building = a.building.map(|b| self.story().and_then(|s| s.building_names.get(&b)).cloned().unwrap_or_else(|| format!("#{b}")));
            self.note(Trace::Arrive { army: army.clone(), cell: a.cell, building: building.clone() });
            let when = When::ArmyArrives { army: a.army, at: Some(a.cell), building: a.building };
            let events = self.story_events(when);
            self.ai_events.extend(events);
        }
        self.clock.set_total_minutes(end);
    }
}

#[cfg(test)]
mod tests;
