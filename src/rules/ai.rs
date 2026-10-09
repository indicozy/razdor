//! AI armies (`original-mechanics/ai.md`): the scenario's armies as the original steers them,
//! one to one, its slips included.
//!
//! - **Step clock** (ai.md §2, 0x4a399c): an army banks the minutes of every hero step and
//!   wait tick (up to 200) and takes a step when they cover `cost(cell left) × speed`, ×1.5
//!   diagonally; with no path it "steps in place" on its own cell's cost. Every step ends in
//!   an **arrival**: the talk counters grow, it may **re-plan**, then its arrival rules run.
//!   Stationary guards (patrol radius 0) do nothing at all.
//! - **Planner** (§7, [`Game::ai_plan`]): no goal is kept. Each plan seeds every target at
//!   once (buildings by their score, armies by their cached battle score or talk counter,
//!   four wander points) into one flood over the map (`TileMap::flood_field`), laid over a
//!   multiplier map where losing matchups push **repulsion cones** and forbidden buildings and
//!   guards are closed; the path is read back by steepest descent.
//! - **Scores**: an army's score against another comes from a simulated battle
//!   ([`Game::army_score`], cached per pair with dirty flags); a building's from four parts
//!   ([`Game::building_score`]); lower is more attractive, 0 no interest, below 0 danger.
//! - **Arrival** ([`Game::ai_arrive`]): contacts with neighbours (attack, triple the scores,
//!   greet), then in a building: assault and capture, village gold, shopping, healing,
//!   resurrection, hiring, garrison buying and the garrison reshuffle; the visited building's
//!   score is zeroed until the next rescoring.
//! - **AI battles** ([`Game::ai_battle`]): the battle engine plays both sides; HP, deaths, XP
//!   with promotion rolls, the asymmetric loot and the pooled items handed out by tactical
//!   gain. Beaten armies leave the map and **respawn** at their home after their days.
//! - **Noon and midnight** (§14): an army's noon is run at its first arrival after 12:00;
//!   midnight updates the village average and rescores every building.
//!
//! Everything the original leaves open is marked *(guess)*.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::dt::dtm::Army as DtArmy;

use super::battle::{Battle, Outcome, Team, RECORDS};
use super::clock::MINUTES_PER_DAY;
use super::content::{Bonus, Content, GlobalOptions, ItemId, Nature, UnitId, WageKind};
use super::economy::{delphi_round, rear_service, relation_price};
use super::events::ArmyId;
use super::fog;
use super::game::{troop_unit, troop_unit_stats, Event, Foe, Game, HeroCells, TALKED};
use super::items;
use super::map::{step_minutes, Tile};
use super::rng::Rng;
use super::units::Unit;
use super::world::{Army, Location, LocationKind, Owner, Troop, World};

/// Unit records of an army or a garrison.
pub const MAX_UNITS: usize = 12;
/// Items an army's pack holds.
pub const MAX_ARMY_ITEMS: usize = 12;
/// Key of the hero in an army's score and talk maps (scenario armies are 1-based).
pub const HERO: u32 = 0;
/// Battle reports kept in [`Game::ai_log`].
pub const LOG_KEPT: usize = 30;
/// Steps after which an auto-played battle is cut off (the engine's turn limit ends it well
/// before).
const MAX_BATTLE_STEPS: usize = 20_000;
/// Wander points an army keeps (0x4a2550).
const WANDER_POINTS: usize = 4;
/// A building or an army it may not go for.
const FORBIDDEN: i32 = -100_000;
/// The respawn time of a beaten army that never comes back by itself (no home, no delay,
/// or a feudal lord owning none of its buildings): it stays destroyed off the map.
pub const NEVER: f64 = f64::MAX;
/// Pair scores tripled at a sheltered neighbour stop here.
const TRIPLED_CAP: i32 = 10_000;
/// Armies closer than this (octile) close their cell to an ignored army's plan and the other
/// way round (0x4a2d88).
const NEAR_IGNORED: i32 = 11;
/// After more idle plans in a row than this, an army takes random targets anyway.
const IDLE_FOR_RANDOM: i32 = 10;
/// Repulsion slopes: the steep cone's factor, and a stationary guard's narrower cones.
const STEEP: f32 = 25.0;
const GUARD_SLOPE: f32 = 5.0;
/// A talk counter at or above this no longer draws an army to its friend.
const TALK_COOLED: i32 = 800;
/// A unit hired or bought beyond this while filling the third role is not wanted.
const LATE_CAP: usize = 8;
/// Rounds of the garrison reshuffle (the counter is tested before it is raised).
const RESHUFFLE_ROUNDS: i32 = 201;
/// A tactical gain an item must beat to be bought or worn.
const WORTH: i32 = 5;
/// What a cell the reshuffle cannot fill is charged.
const FULL: i32 = 100_000;

/// Behaviour style (army byte 59).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Style {
    /// Collects village gold, pays wages, keeps `NeedUpkeepDay` days of them.
    #[default]
    Feudal,
    /// Pays no wages; spends a day's worth.
    Rogue,
    /// Never assaults, shops, heals or hires; collects nothing.
    Peasant,
}

impl Style {
    /// Byte 59 (0 feudal, 1 rogue, 2 peasant); other values fall back to the map model
    /// (4 feudal, 5 bandits, 6 peasants).
    pub fn of(behaviour: u8, model: u8) -> Style {
        match (behaviour, model) {
            (0, _) => Style::Feudal,
            (1, _) => Style::Rogue,
            (2, _) => Style::Peasant,
            (_, 5) => Style::Rogue,
            (_, 6) => Style::Peasant,
            _ => Style::Feudal,
        }
    }

    /// The byte the original compares (`style < 2`, `style == 0`) and indexes
    /// `AIDistance0..2` with.
    pub fn byte(self) -> usize {
        match self {
            Style::Feudal => 0,
            Style::Rogue => 1,
            Style::Peasant => 2,
        }
    }

    /// Feudal or rogue: the styles that assault, shop, heal and hire (`style < 2`).
    pub fn lordly(self) -> bool {
        self != Style::Peasant
    }
}

/// Target models (army byte 85): index into the `_Global.ini` priority lists.
pub mod model {
    pub const STANDARD: usize = 0;
    pub const AGGRESSIVE: usize = 1;
    pub const PASSIVE: usize = 2;
    pub const HOARDING: usize = 3;
    pub const TRADING: usize = 4;
}

/// What the scenario says about an army's behaviour (ai.md §1).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AiProfile {
    /// Steered by the AI. False for the demo's gangs.
    pub enabled: bool,
    pub style: Style,
    /// Target model, 0..=4 ([`model`]).
    pub model: usize,
    /// Byte 69, signed, in percent: shifts the simulated battles it scores.
    pub aggression: i32,
    /// Attitude towards the four factions (bytes 65–68).
    pub relations: [i8; 4],
    pub respawn_days: u32,
    /// Byte 83: a beaten army comes back whole, not only its leader, when the player beat it.
    pub respawn_all: bool,
    /// Base daily income: byte 80 × 10.
    pub extra_income: i32,
    /// Byte 82: the garrison level its own towns, castles and forts are kept at (0 none).
    /// Saves from before it read −1 and take the scenario's on load.
    #[serde(default = "unknown_level")]
    pub garrison_level: i32,
    /// Byte 71: experience correction in percent. It scales the XP the player gains by
    /// beating this army (experience.md §3); 0 pays nothing, as in the original.
    pub exp_correction: i32,
    /// Byte 14, "add experience like the player": units it hires start with XP taken from
    /// the player's army (experience.md §5).
    #[serde(default)]
    pub exp_like_player: bool,
    /// Byte 19: bonus XP for the units it hires.
    #[serde(default)]
    pub hire_bonus_exp: i32,
    /// Its units carry no money: no gold to take.
    pub no_money: bool,
    /// Flags (bytes 76–81).
    pub ignored: bool,
    pub player_only: bool,
    pub no_random: bool,
    pub no_talk: bool,
    pub no_buildings: bool,
}

fn unknown_level() -> i32 {
    -1
}

impl AiProfile {
    /// The profile of a scenario army.
    pub fn from_dt(a: &DtArmy) -> AiProfile {
        AiProfile {
            enabled: true,
            style: Style::of(a.behaviour, a.model),
            model: (a.target_model as usize).min(model::TRADING),
            aggression: a.aggression as i32,
            relations: a.relations,
            respawn_days: a.respawn_days as u32,
            respawn_all: a.respawn_all != 0,
            extra_income: a.unknown_80 as i32 * 10,
            garrison_level: a.garrison_strength as i32,
            // As it is: 0 makes the army pay the player no XP (experience.md §3).
            exp_correction: a.exp_correction as i32,
            exp_like_player: a.exp_like_player != 0,
            hire_bonus_exp: a.hire_bonus_exp as i32,
            no_money: a.no_money != 0,
            ignored: a.ignored_by_ai != 0,
            player_only: a.hunts_player_only != 0,
            no_random: a.no_random_targets != 0,
            no_talk: a.no_socialising != 0,
            no_buildings: a.no_building_interest != 0,
        }
    }
}

/// What the AI keeps of an army between its arrivals (ai.md §1's record fields).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AiMind {
    /// Steps until it plans again.
    pub countdown: i32,
    /// Plans in a row that found it standing with nothing to do.
    pub idle: i32,
    /// Its four wander points; a point in column 0 (a cleared one is (0, 0)) is never a
    /// target.
    pub wander: [Tile; WANDER_POINTS],
    /// Came back from a respawn and has not finished a path since: it seeds no armies.
    pub just_respawned: bool,
    /// The original's place on its path: the steps taken since the path was read (its path
    /// index), and whether it has no path at all (length 0: a fresh record, a respawn, an
    /// activation). Only a step in place that the hero's cell bars tells them apart.
    pub walked: i32,
    pub no_path: bool,
    /// Its stored step cost is 0 (a respawn or an activation sets it so): its next step costs
    /// nothing, so it arrives at once (0x4a399c charges the stored cost).
    pub free_step: bool,
    /// The original's direction of its next step (+0x1710) is 8, "none": its path has no
    /// next cell. The map load writes 5 there (so false), the AI's setup 8 for a stationary
    /// guard, and the step clock the next step's direction (8 with no next cell) after every
    /// call that starts or ends a step (0x4a399c). Only the stop's idle draw reads it
    /// ([`Game::armies_snap`]).
    pub facing_none: bool,
    /// Where that direction points while it has no path: the offset of the cell the step
    /// clock prices as "the step after" a step in place (0x4a399c reads the cell one step
    /// along +0x1710 from where it stands, 8 being no offset). The map load's 5 points south,
    /// so a fresh record's first step in place is followed by the cell south of it; every
    /// arrival sets it from its path (none with no next cell), and a respawn keeps it.
    pub stand_facing: Option<Tile>,
    /// The building defence its units' cached strengths (+0x1ae) were last worked out with:
    /// the recount (0x4a16d4) runs at its arrivals in a building (not a bridge), on leaving
    /// one, after its AI battles and at a respawn, with the defence (+0x378c) of that moment.
    /// The map load recounts before it writes the defence (0x4a1ff0), so an army standing
    /// in a building at the start scores with 0 until its next recount. Its battle sides
    /// copy those strengths (49855c), so its simulated battles and its XP pools count them.
    pub strength_bd: i32,
    /// Its gold wage bill (+0x16e0) as the same recount (0x4a16d4) last worked it out: the
    /// wages of the units living then. The player's loot adds it (0x4c50ec), and his battle
    /// recounts nobody, so a gang he wipes out still pays the bill of its last recount.
    pub wage_bill: i32,
    /// The step weights its path buffer holds, node by node (the original's direction field
    /// of each path point read through the table 0x4ecfd4): a path read writes the weight of
    /// every step out of a node but the last, whose entry keeps what an earlier, longer path
    /// left there (0 = direction 0, weight 3, in the zeroed buffer of a fresh record). Only
    /// the step clock's play time reads it ([`Game::ai_start`]).
    #[serde(skip)]
    pub path_weights: Vec<u8>,
    /// Healing keeps it standing until this minute.
    pub busy_until: f64,
    /// Game minute of its next noon.
    pub next_noon: f64,
    /// Today's income as its noon counted it (for its spare gold).
    pub income: i32,
    /// Village gold it collects a day on average, and what it collected today.
    pub village_avg: i32,
    pub village_today: i32,
    /// The building it stands in and the defence its battles get from it.
    pub standing: Option<usize>,
    pub defence: i32,
    /// Battle scores against other armies (by uid, [`HERO`] the hero) and the pairs not to be
    /// rescored before something changes them.
    pub scores: BTreeMap<u32, i32>,
    pub clean: BTreeSet<u32>,
    /// Talk counters towards other armies (the one towards the hero is [`Army::talk`]).
    pub talk: BTreeMap<u32, i32>,
    /// Its score of every building (by index).
    pub buildings: Vec<i32>,
    /// What its last arrival did to the hero, read after his step ([`Game::ai_contact`]).
    #[serde(skip)]
    pub contact: Option<Contact>,
    /// Beaten in a battle of its own arrival: off the map, but the rest of that arrival runs
    /// with its record, as the original's does; it leaves at the arrival's end.
    #[serde(skip)]
    pub fallen: bool,
    /// Tests walk it along the path they give it: it never plans.
    #[cfg(test)]
    #[serde(skip)]
    pub scripted: bool,
}

/// The direction the map load writes into every army record (+0x1710 := 5, south, as the
/// offset of the cell it points at; tables 0x4ecf8c / 0x4ecfb0).
const LOAD_FACING: Tile = (0, 1);

/// A step the step clock started, arriving at the end of its play time.
struct Pending {
    uid: u32,
    next: Option<Tile>,
    moves: bool,
    minutes: f32,
}

/// Game minutes in the original's centi-minutes.
fn cmin(minutes: f32) -> i64 {
    (minutes as f64 * 100.0).round() as i64
}

/// What an AI army's arrival does to the hero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contact {
    Attack,
    Greet,
}

/// A beaten army waiting to come back.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Respawn {
    /// Game minute after which it comes back (time of defeat + its respawn days).
    pub due: f64,
    pub army: Army,
}

/// A battle between AI armies (or an AI army and a garrison), for the player's log.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AiNews {
    pub text: String,
    pub tile: Tile,
    /// Game minute.
    pub at: u64,
}

/// Counts for the simulation reports and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AiStats {
    pub battles: u32,
    pub captures: u32,
    pub respawns: u32,
    pub hired: u32,
    pub bought: u32,
    /// Plans made.
    pub paths: u32,
}

/// The AI steers this army: a scenario army. The demo's gangs keep the simple rules of
/// `Game::move_armies`.
pub fn managed(a: &Army) -> bool {
    a.ai.enabled
}

/// A stationary guard: patrols with radius 0. It never steps, plans or arrives.
pub fn stationary(a: &Army) -> bool {
    a.patrols && a.patrol_radius == 0
}

/// Priorities of one target model (`_Global.ini`, ai.md §1): lower is more urgent.
/// Pairs are (Min, Max).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Priorities {
    pub attack_army: i32,
    pub attack_castle: i32,
    pub random: i32,
    pub talk: i32,
    pub heal: (i32, i32),
    pub garrison: (i32, i32),
    pub purchase: (i32, i32),
    pub gold_purchase: i32,
    pub village: (i32, i32),
    pub gold_village: i32,
}

impl Priorities {
    /// The priorities of target model `m`. A missing key reads as 0, as the exe reads it,
    /// so the shipped file's misspelt `MixHealingTarget` leaves the healing minimum at 0.
    pub fn of(o: &GlobalOptions, m: usize) -> Priorities {
        let t = &o.ai_targets;
        let m = m.min(model::TRADING);
        let g = |p: Option<[i32; 5]>| p.map_or(0, |v| v[m]);
        Priorities {
            attack_army: g(t.min_attack_army),
            attack_castle: g(t.min_attack_castle),
            random: g(t.min_random),
            talk: g(t.min_talking),
            heal: (g(t.min_healing), g(t.max_healing)),
            garrison: (g(t.min_garrison), g(t.max_garrison)),
            purchase: (g(t.min_purchase), g(t.max_purchase)),
            gold_purchase: g(t.gold_purchase),
            village: (g(t.min_village), g(t.max_village)),
            gold_village: g(t.gold_village),
        }
    }
}

/// `AIDistance0..2` by behaviour style: how far an army rescores other armies.
pub fn target_range(o: &GlobalOptions, style: Style) -> i32 {
    o.ai_distance[style.byte()]
}

/// Cell `t` lies inside army `a`'s patrol box (`centre ± radius`, clamped to the map); an
/// army that does not patrol has no box.
pub fn in_patrol(a: &Army, t: Tile) -> bool {
    let c = a.patrol_centre();
    !a.patrols || ((t.0 - c.0).abs() <= a.patrol_radius && (t.1 - c.1).abs() <= a.patrol_radius)
}

/// The original's `Round` of a float stored into an integer: half to even, and a value
/// beyond 64 bits (an infinity or NaN from a division by 0) gives the FPU's "indefinite"
/// integer, whose low 32 bits the callers keep: 0.
pub fn fpu_round(x: f64) -> i32 {
    if !x.is_finite() || x.abs() >= 9.2e18 {
        return 0;
    }
    delphi_round(x) as i32
}

/// Attitude `atts` holds towards faction `f` (1–4); another value reads as 0.
pub fn attitude_of(atts: &[i8; 4], f: u8) -> i8 {
    match f {
        1..=4 => atts[f as usize - 1],
        _ => 0,
    }
}

/// The original's relation of side A to side B (0x4a0868, ai.md §3), each side given by its
/// faction and attitudes: A's attitude to B's faction `a` and B's to A's `b`; both ≥ 0 →
/// `(a + b) div 2`, `a` < 0 → `a`, else −1. The factions are not compared.
pub fn relation_between(a: (u8, &[i8; 4]), b: (u8, &[i8; 4])) -> i8 {
    super::world::relation(attitude_of(a.1, b.0), attitude_of(b.1, a.0))
}

/// Army `a` is welcome in building `l`: its own, or the building's attitude to its faction
/// is not below 0 (what shopping and the services test).
fn welcome(a: &Army, l: &Location) -> bool {
    l.owner == Owner::Army(a.id) || l.attitude_to(a.faction) >= 0
}

/// Building `l` is one army `a` assaults when it stands in it (ai.md §9.1): the building's
/// attitude to its faction or its attitude to the building's faction is negative, it does
/// not own it and it is feudal or rogue; a town only at its attitude −3.
fn assaults(a: &Army, l: &Location) -> bool {
    let mine = attitude_of(&a.ai.relations, l.faction);
    let hostile = (l.attitude_to(a.faction) < 0 || mine < 0) && l.owner != Owner::Army(a.id) && a.ai.style.lordly();
    hostile && !(l.kind == LocationKind::Town && mine > -3)
}

/// Kinds with a garrison record (0x4b5291): towns, castles, forts and ruins.
fn has_garrison(kind: LocationKind) -> bool {
    matches!(kind, LocationKind::Town | LocationKind::Castle | LocationKind::Fort | LocationKind::Ruins)
}

/// Army `a`'s route from `from` to `to` around the buildings it may not walk through
/// ([`bars_army`]); the event that sends an army somewhere uses it.
pub fn army_path(world: &World, a: &Army, from: Tile, to: Tile, max_nodes: usize) -> Vec<Tile> {
    let (start, end) = (world.location_covering(from), world.location_covering(to));
    world.map.path_where(from, to, max_nodes, &|t| match world.location_covering(t) {
        Some(l) if Some(l) != start && Some(l) != end => !bars_army(a, &world.locations[l]),
        _ => true,
    })
}

/// Army `a` may not walk through building `l`, on its own plans (Razdor keeps AI armies to
/// the hero's roads) and on an event's errand: castles and forts that are not its own or a
/// friend's, ruins not its own and not cleared, and any other building it would assault.
pub fn bars_army(a: &Army, l: &Location) -> bool {
    let own = l.owner == Owner::Army(a.id);
    let friend = own || l.attitude_to(a.faction) > 0;
    match l.kind {
        LocationKind::Castle | LocationKind::Fort => !friend,
        LocationKind::Ruins => !own && !l.cleared,
        k if k.is_bridge() => false,
        _ => assaults(a, l),
    }
}

// ----------------------------------------------------------------------------------------
// Unit records
// ----------------------------------------------------------------------------------------

/// A troop's hit points and maximum (worn items included); 0 for a corpse.
pub fn troop_hp(c: &Content, t: &Troop) -> (i32, i32) {
    let (u, stats) = troop_unit_stats(c, t);
    (u.hp, stats.max_hp().max(1))
}

/// Maximum HP of a troop.
pub fn troop_max_hp(c: &Content, t: &Troop) -> i32 {
    troop_hp(c, t).1
}

/// The two tactical costs of a unit record (experience.md §1): mode 0 of its level stats,
/// mode 1 of its current stats, worn items included; both with the building defence `bd`
/// its record stands in, both at least 1.
pub fn tactical_modes(c: &Content, t: &Troop, bd: i32) -> (i32, i32) {
    let u = troop_unit(c, t);
    (super::experience::tactical(c, t.unit, &u.base_stats(c), bd), tactical_now(c, t, bd))
}

/// Mode 1 of [`tactical_modes`] alone: the tactical cost of its current stats.
pub fn tactical_now(c: &Content, t: &Troop, bd: i32) -> i32 {
    let (_, stats) = troop_unit_stats(c, t);
    super::experience::tactical(c, t.unit, &stats, bd)
}

/// The "gain" of a unit's items (the original's tactical cost mode 2): its current tactical
/// cost over the one of its level stats.
fn item_gain(c: &Content, t: &Troop, bd: i32) -> i32 {
    let (base, now) = tactical_modes(c, t, bd);
    now - base
}

/// The attack role of a unit type (record +0xC4, saves-data.md): 4 melee when `AB > AS` and
/// `AB > MP`, then 7 shooter when `AS > AB` and `AS > MP`, then 0x11 caster when `AB/2 < MP`
/// and `AS/2 < MP`; a later test overrides an earlier one, none gives 0.
pub fn attack_kind(c: &Content, unit: UnitId) -> u8 {
    let d = c.unit(unit);
    let (ab, sh, mp) = (d.attack_blow, d.attack_shot, d.magic_power);
    let mut k = 0;
    if ab > sh && ab > mp {
        k = 4;
    }
    if sh > ab && sh > mp {
        k = 7;
    }
    if (ab as f64 / 2.0) < mp as f64 && (sh as f64 / 2.0) < mp as f64 {
        k = 0x11;
    }
    k
}

/// What the army-totals routine (0x4a16d4) keeps of a record (ai.md §5, §7.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// The gold wage bill of the living units (a kind-1 Elemental's wage goes to the mana
    /// bill instead).
    pub wages: i32,
    /// `Σ Round(Cost / CostRecrutDiv)` over the living units, untiered, the leader included.
    pub recruit_sum: i32,
    /// `Σ Round(HP × Cost / maxHP × HealingConst/100)` over the wounded: their current HP.
    pub heal_bill: i32,
    /// `Σ Round(Cost × ResurectConst/100)` over the dead.
    pub res_bill: i32,
    /// Σ max HP and Σ missing HP of the living.
    pub max_living: i32,
    pub missing: i32,
    /// Σ mode-1 tactical cost over every unit, the dead included.
    pub strength: i32,
}

/// The totals of `troops` standing in a building of defence `bd`.
pub fn totals(c: &Content, troops: &[Troop], bd: i32) -> Totals {
    let o = &c.options;
    let mut t = Totals::default();
    for tr in troops {
        // One rebuild of the record serves its strength and its HP.
        let (u, stats) = troop_unit_stats(c, tr);
        t.strength += super::experience::tactical(c, tr.unit, &stats, bd);
        let cost = c.unit(tr.unit).cost;
        if !tr.alive() {
            t.res_bill += fpu_round(cost as f64 * (o.resurect_const as f64 / 100.0));
            continue;
        }
        if tr.kind.is_paid() && !(tr.kind == WageKind::Recruit && c.paid_in_mana(tr.unit)) {
            t.wages += c.wage_for(tr.unit, tr.kind);
        }
        t.recruit_sum += fpu_round(cost as f64 / o.cost_recrut_div as f64);
        let (hp, max) = (u.hp, stats.max_hp().max(1));
        t.max_living += max;
        if hp < max {
            t.missing += max - hp;
            t.heal_bill += fpu_round((hp as i64 * cost as i64) as f64 / max as f64 / (100.0 / o.healing_const as f64));
        }
    }
    t
}

impl Game {
    /// The recount (0x4a16d4) of army `i`'s wage bill (+0x16e0): the wages of its living
    /// units now.
    pub(crate) fn recount_bill(&mut self, i: usize) {
        let bill = army_wages(&self.content, &self.world.armies[i].troops);
        self.world.armies[i].mind.wage_bill = bill;
    }
}

/// Daily gold wages of an AI army's living troops (the leader and event units draw none).
pub fn army_wages(c: &Content, troops: &[Troop]) -> i32 {
    totals(c, troops, 0).wages
}

/// The units an army fights with: its living troops, their worn items on.
pub fn army_units(c: &Content, a: &Army) -> Vec<Unit> {
    a.troops.iter().filter(|t| t.alive()).map(|t| troop_unit(c, t)).collect()
}

/// Puts `item` on troop `t` if it can wear it (`items::slot_for`'s rules), and rebuilds it:
/// its HP follows its maximum (`items::put_on`). Returns the slot.
fn wear(c: &Content, t: &mut Troop, item: ItemId) -> Option<usize> {
    let mut u = troop_unit(c, t);
    let slot = items::slot_for(c, &u, item).ok()?;
    items::put_on(c, &mut u, slot, item);
    t.worn[slot] = Some(item);
    if u.alive() {
        t.hurt = (u.max_hp(c) - u.hp).max(0);
        t.carry = u.carry;
    }
    Some(slot)
}

/// The tactical gain of troop `t` with `item` tried on (mode 2), if it can wear it.
fn gain_with(c: &Content, t: &Troop, item: ItemId, bd: i32) -> Option<i32> {
    let mut tried = *t;
    wear(c, &mut tried, item)?;
    Some(item_gain(c, &tried, bd))
}

/// An item of a scenario army at map load (0x4a273c): it goes to the unit whose tactical
/// gain it raises most (strictly above 0, the first of equals), else into the pack (lost
/// when the pack is full).
pub fn give_item(c: &Content, a: &mut Army, item: ItemId) {
    give_item_to(c, &mut a.troops, &mut a.items, item);
}

/// [`give_item`] for any record, an army's or a garrison's (0x4a273c takes either): its
/// troops and its pack.
pub fn give_item_to(c: &Content, troops: &mut [Troop], pack: &mut Vec<ItemId>, item: ItemId) {
    let mut best: Option<(usize, i32)> = None;
    for (k, t) in troops.iter().enumerate() {
        if let Some(v) = gain_with(c, t, item, 0) {
            if v > best.map_or(0, |b| b.1) {
                best = Some((k, v));
            }
        }
    }
    match best {
        Some((k, _)) => {
            wear(c, &mut troops[k], item);
        }
        None if pack.len() < MAX_ARMY_ITEMS => pack.push(item),
        None => {}
    }
}

/// Hands out a pool of items (0x4a473c, ai.md §10.1): the pack joins the pool; then, while
/// some unit can wear some pool item with a tactical gain above 5, the largest gain (units in
/// order, items in order, the first of equals) is worn and the item leaves the pool (the
/// last one takes its place). The pack is then refilled with up to 12 of the rest, the
/// dearest first; the others are lost.
pub fn redistribute(c: &Content, troops: &mut [Troop], pack: &mut Vec<ItemId>, mut pool: Vec<ItemId>, bd: i32) {
    pool.append(pack);
    loop {
        let mut best: Option<(usize, usize, i32)> = None;
        for (u, t) in troops.iter().enumerate() {
            for (k, &item) in pool.iter().enumerate() {
                if let Some(v) = gain_with(c, t, item, bd) {
                    if v > WORTH && v > best.map_or(0, |b| b.2) {
                        best = Some((u, k, v));
                    }
                }
            }
        }
        let Some((u, k, _)) = best else { break };
        wear(c, &mut troops[u], pool[k]);
        pool.swap_remove(k);
        if pool.is_empty() {
            break;
        }
    }
    while pack.len() < MAX_ARMY_ITEMS {
        let mut best: Option<(usize, i32)> = None;
        for (k, &item) in pool.iter().enumerate() {
            let price = c.try_item(item).map_or(0, |d| d.cost.abs());
            if price > best.map_or(0, |b| b.1) {
                best = Some((k, price));
            }
        }
        let Some((k, _)) = best else { break };
        pack.push(pool.remove(k));
    }
}

// ----------------------------------------------------------------------------------------
// Simulated battles
// ----------------------------------------------------------------------------------------

/// Hit points of both sides before and after a simulated battle, and its last turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SimResult {
    pub own: i64,
    pub own_left: i64,
    pub theirs: i64,
    pub theirs_left: i64,
    pub turn: u32,
    /// The scoring side ended with fewer units than it started with (its living count, side
    /// +0 after the end's copy, below its start count +4; a surrendered side counts none).
    pub own_lost_units: bool,
}

/// One side of a battle between AI sides: its units (their spells in their slots) and its
/// building defence.
pub struct Side {
    pub units: Vec<Unit>,
    pub defence: i32,
    /// The defence its unit strengths count (an army's last recount, [`AiMind::strength_bd`];
    /// a garrison's and the hero's are their defence).
    pub strength_defence: i32,
    /// Its strengths count the units' level stats, not their items (a ruins' garrison not
    /// recounted since the map load, [`Location::strengths_bare`]).
    pub bare: bool,
}

/// Sets up the off-screen battle between `a` (attacking) and `b` (0x4a0710): the battle
/// engine on both sides, AI mode 0, both sides auto-arranged, each with its own building
/// defence; played to its end.
fn fight(c: &Arc<Content>, a: &Side, b: &Side, predict: bool, records: SideRecords) -> Battle {
    let side: Vec<(usize, &Unit)> = a.units.iter().enumerate().map(|(k, u)| (k + 1, u)).collect();
    let mut bt = Battle::new(c.clone(), &side, &b.units, Team::Player);
    bt.set_simulation();
    if !predict {
        bt.skip_prediction();
    }
    if a.defence > 0 {
        bt.set_building_defence(Team::Player, a.defence);
    }
    if b.defence > 0 {
        bt.set_building_defence(Team::Enemy, b.defence);
    }
    bt.set_strength_defence(Team::Player, a.strength_defence);
    bt.set_strength_defence(Team::Enemy, b.strength_defence);
    for (team, s) in [(Team::Player, a), (Team::Enemy, b)] {
        if s.bare {
            bt.set_bare_strengths(team);
        }
    }
    bt.auto_arrange(Team::Player);
    bt.auto_arrange(Team::Enemy);
    bt.set_side_records(records);
    bt.begin();
    let mut steps = 0;
    while bt.outcome() == Outcome::Ongoing && steps < MAX_BATTLE_STEPS {
        bt.ai_step();
        steps += 1;
    }
    bt
}

/// The side strengths of a played battle (483ecc, experience.md §3): each side's at the
/// start (+0x7ec, as the battle's set-up worked it out) and at the end (+0x7e8, the end's
/// recount, 48bb10), and its last turn. The score's caps (end at most start) are the
/// reader's.
fn sim_result(bt: &Battle) -> SimResult {
    SimResult {
        own: bt.start_of(Team::Player).strength,
        own_left: bt.strength_now(Team::Player),
        theirs: bt.start_of(Team::Enemy).strength,
        theirs_left: bt.strength_now(Team::Enemy),
        turn: bt.round,
        own_lost_units: bt.fighters.iter().filter(|f| f.team == Team::Player && f.alive() && !f.surrendered).count() < bt.start_of(Team::Player).count,
    }
}

/// Plays a simulated battle (the AI's scoring, 0x4a0710) and returns its side strengths.
pub fn simulate(c: &Arc<Content>, a: &Side, b: &Side) -> SimResult {
    let bt = fight(c, a, b, false, [[0; RECORDS]; 2]);
    sim_result(&bt)
}

/// The two static sides of the off-screen battles (0xc081ac attacker, 0xc08a00 defender):
/// the HP of each of their 12 unit records as their last use left them. 49855c writes an
/// army's units into records 1..n and leaves the rest; a battle copies its sides back out
/// (48bb10). The killable test's wrong-side read (486d03) reads the records beyond a side's
/// units, so an off-screen battle depends on what came before it (never cleared, not
/// saved).
pub type SideRecords = [[i32; RECORDS]; 2];

/// Writes `hps` into records 1..n of side `k` (49855c), the rest left as they were.
fn fill_records(r: &mut SideRecords, k: usize, hps: impl IntoIterator<Item = i32>) {
    for (slot, hp) in r[k].iter_mut().zip(hps) {
        *slot = hp;
    }
}

/// What a simulated battle depends on: each side's units (type, level, HP, worn items),
/// building defence and spells. The engine plays the AI on both sides with no randomness, so
/// the same key gives the same result ([`SimCache`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SimKey {
    sides: [SideKey; 2],
}

/// A side of a [`SimKey`]: (type, level, HP, worn items, spells, drain) of each unit, the
/// defence and the defence its strengths were counted with.
type UnitKey = (u32, i32, i32, [Option<ItemId>; items::SLOTS], [Option<crate::rules::units::SpellSlot>; crate::rules::units::SPELL_SLOTS], i32);
type SideKey = (Vec<UnitKey>, i32, i32, bool);

impl SimKey {
    fn of(a: &Side, b: &Side) -> SimKey {
        let side = |s: &Side| (s.units.iter().map(|u| (u.def.0, u.level, u.hp, u.items, u.spells, u.drain)).collect(), s.defence, s.strength_defence, s.bare);
        SimKey { sides: [side(a), side(b)] }
    }
}

/// Simulated battles already played (not saved): the AI rescores the same matchups often.
/// A result depends on the side records it starts from too ([`SideRecords`]), which it
/// leaves changed: both are kept with it.
#[derive(Clone, Debug, Default)]
pub struct SimCache {
    /// By the sides' units: the battles played, each with the old records it read (side,
    /// record, HP) and the records it left in place of its units.
    played: std::collections::HashMap<SimKey, Vec<Played>, std::hash::BuildHasherDefault<KeyHasher>>,
    entries: usize,
    /// The static sides as the last off-screen battle or side pass left them.
    pub(crate) records: SideRecords,
}

/// The cache's hasher: the multiply-rotate hash of the Rust compiler's own tables (FxHash),
/// far cheaper than the default SipHash on the long unit lists of a [`SimKey`]; the cache
/// is only looked up, never walked, so the order of its table does not matter.
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyHasher(u64);

impl std::hash::Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.write_u64(u64::from_le_bytes(c.try_into().expect("8 bytes")));
        }
        for &b in chunks.remainder() {
            self.write_u64(b as u64);
        }
    }

    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn write_u8(&mut self, v: u8) {
        self.write_u64(v as u64);
    }

    fn write_u16(&mut self, v: u16) {
        self.write_u64(v as u64);
    }

    fn write_u32(&mut self, v: u32) {
        self.write_u64(v as u64);
    }

    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }
}

/// A simulated battle in the cache: it plays the same from any side records that hold the
/// same values where it read them.
#[derive(Clone, Debug)]
struct Played {
    reads: Vec<(usize, usize, i32, bool)>,
    result: SimResult,
    counts: [usize; 2],
    left: SideRecords,
}

/// Entries the cache holds before it starts afresh.
const SIM_CACHE_SIZE: usize = 50_000;

impl SimCache {
    fn get(&mut self, c: &Arc<Content>, a: &Side, b: &Side) -> SimResult {
        self.stage(a, b);
        let key = SimKey::of(a, b);
        let records = self.records;
        if let Some(p) = self.played.get(&key).and_then(|v| v.iter().find(|p| p.reads.iter().all(|&(k, i, dmg, killable)| (records[k][i] <= dmg) == killable))) {
            let (r, counts, left) = (p.result, p.counts, p.left);
            for k in 0..2 {
                self.records[k][..counts[k]].copy_from_slice(&left[k][..counts[k]]);
            }
            return r;
        }
        if self.entries >= SIM_CACHE_SIZE {
            self.played.clear();
            self.entries = 0;
        }
        let bt = fight(c, a, b, false, records);
        let r = sim_result(&bt);
        let left = bt.side_records();
        let reads = bt.stale_reads();
        self.played.entry(key).or_default().push(Played { reads, result: r, counts: bt.start_counts(), left });
        self.entries += 1;
        self.records = left;
        r
    }

    /// The sides of a battle are written into the static sides: the attacker's units into
    /// the first, the defender's into the second (0x4a0710 → 49855c).
    fn stage(&mut self, a: &Side, b: &Side) {
        fill_records(&mut self.records, 0, a.units.iter().map(|u| u.hp));
        fill_records(&mut self.records, 1, b.units.iter().map(|u| u.hp));
    }

    /// An army passed through the first static side and back to rearrange it (49855c,
    /// 4988c0: a respawn, an arrival's garrison reshuffle).
    pub(crate) fn pass_through(&mut self, hps: impl IntoIterator<Item = i32>) {
        fill_records(&mut self.records, 0, hps);
    }
}

/// The score of a simulated battle of army A (aggression `g`, speed `speed_a`) against side
/// C (speed `speed_c`) at relation `r` (0x4a08f8, ai.md §4). Lower is better, 0 nothing to
/// gain, below 0 a danger.
/// - Nothing happened, or the battle ran to `BattleEndTurn` (even one wiped out on that very
///   turn): 0.
/// - The aggression shifts both results: `B1 −= Round(g·B0/100)` (not below 0) and
///   `A1 += Round(g·A0/100)`, a tenth of that (÷1000) for a negative `g` when A lost no unit
///   (its living count at the end is not below its start count); A1 not below 0.
/// - A win (`A1 > 0` and `A1 > B1`): `Round((1 − A1/A0)·30·ZeroDensity·A0/B0 + 1)` after a
///   loss, `Round(A0/B0 + 1)` without; at least 1.
/// - A loss: `−5 − Round(√(B0/A0)·ZeroDensity·speedA/speedC)`, not below −50.
/// - By relation: `s div (r + 1)` at r ≥ 0; at r < 0 a danger grows `((2 − r)·s) div 3`, a
///   target becomes `(AtackArmy + s)·(r + 4)`.
#[allow(clippy::too_many_arguments)]
pub fn army_score(s: SimResult, g: i32, r: i8, attack_army: i32, zero_density: i32, speed_a: u32, speed_c: u32, end_turn: i32) -> i32 {
    let (a0, b0) = (s.own, s.theirs);
    let (mut a1, mut b1) = (s.own_left.min(a0), s.theirs_left.min(b0));
    if (a1 == a0 && b1 == b0) || s.turn as i32 == end_turn {
        return 0;
    }
    let g = g as i64;
    b1 = (b1 - fpu_round((g * b0) as f64 / 100.0) as i64).max(0);
    // A negative aggression counts a tenth unless the side lost a unit (0x4a08f8 compares
    // the side's living count with its start count).
    let div = if g >= 0 || s.own_lost_units { 100.0 } else { 1000.0 };
    a1 = (a1 + fpu_round((g * a0) as f64 / div) as i64).max(0);
    let zd = zero_density as f64;
    let mut v = if a1 > 0 && a1 > b1 {
        let v = if a1 < a0 {
            fpu_round((1.0 - a1 as f64 / a0 as f64) * (zd * 30.0) * a0 as f64 / b0 as f64 + 1.0)
        } else {
            fpu_round(a0 as f64 / b0 as f64 + 1.0)
        };
        v.max(1)
    } else {
        let v = -5 - fpu_round((b0 as f64 / a0 as f64).sqrt() * (speed_a as f64 * zd) / speed_c.max(1) as f64);
        v.max(-50)
    };
    let r = r as i32;
    if r >= 0 {
        v /= r + 1;
    } else if v < 0 {
        v = (2 - r) * v / 3;
    } else {
        v = (attack_army + v) * (r + 4);
    }
    v
}

// ----------------------------------------------------------------------------------------
// The planner's repulsion cone
// ----------------------------------------------------------------------------------------

/// Adds a repulsion cone of strength `s` and slope `f` around `(x, y)` to the multiplier map
/// `mult` (`w × h`, 0x482e4c, ai.md §7.4): every scanned cell gets
/// `s − floor(f·(max(|dx|,|dy|) + min/2))` when that is above 1. The scan box is the
/// original's: `r = ((s − 2)·32768) div Round(16384·f)`, columns `x − r − 1 ..` for `2r + 1`
/// (or to the map's edge), rows likewise, so it sits a cell up-left of the centre and a weak
/// or steep cone (r = 0) adds nothing. The cells are walked in the bitmap's row order, so a
/// box wider than the map wraps into the next row.
pub fn repulsion(mult: &mut [u16], w: i32, h: i32, (x, y): Tile, f: f32, s: i32) {
    let k = delphi_round(16384.0 * f as f64) as i32;
    if k == 0 {
        return;
    }
    let r = (s - 2).wrapping_mul(0x8000) / k;
    let x0 = (x - r - 1).max(0);
    let cols = if w < x + r { w - x0 } else { 2 * r + 1 };
    let y0 = (y - r - 1).max(0);
    let rows = if h < y + r { h - y0 } else { 2 * r + 1 };
    if cols <= 0 || rows <= 0 {
        return;
    }
    let mut at = (w * y0 + x0) as i64;
    for cy in y0..y0 + rows {
        for cx in x0..x0 + cols {
            let (dx, dy) = ((x - cx).abs(), (y - cy).abs());
            let d2 = if dx < dy { dy * 2 + dx } else { dx * 2 + dy };
            let mut p = d2.wrapping_mul(k);
            if p < 0 {
                p += 0x7fff;
            }
            let v = s - (p >> 15);
            if v > 1 {
                if let Some(m) = usize::try_from(at).ok().and_then(|i| mult.get_mut(i)) {
                    *m = m.wrapping_add(v as u16);
                }
            }
            at += 1;
        }
        at += (w - cols) as i64;
    }
}

/// Who an army deals with: the hero or another army (by index).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Party {
    Hero,
    Army(usize),
}

/// Who beat an army.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beaten {
    ByPlayer,
    ByAi,
}

/// The other side of an AI battle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Defender {
    Army(usize),
    Garrison(usize),
}

impl Game {
    /// Anyone (the player or an AI army) beat this scenario army.
    pub fn army_beaten_by_anyone(&self, id: ArmyId) -> bool {
        self.beaten_armies.contains(&id) || self.ai_beaten.contains(&id)
    }

    fn army_by_uid(&self, uid: u32) -> Option<usize> {
        self.world.armies.iter().position(|a| a.uid == uid)
    }

    /// The pending foe is army `i` (a battle with the player is about to start).
    fn is_foe(&self, i: usize) -> bool {
        self.foe == Some(Foe::Army(i))
    }

    /// Removes army `i` from the map, keeping the pending foe pointing at the right army.
    fn remove_army(&mut self, i: usize) -> Army {
        match self.foe {
            Some(Foe::Army(j)) if j == i => self.foe = None,
            Some(Foe::Army(j)) if j > i => self.foe = Some(Foe::Army(j - 1)),
            _ => {}
        }
        self.world.armies.remove(i)
    }

    /// Puts an army back on the map among the others in id order, keeping the pending foe
    /// pointing at the right army.
    pub(crate) fn insert_army(&mut self, a: Army) {
        let k = self.world.armies.iter().position(|b| b.id > a.id).unwrap_or(self.world.armies.len());
        if let Some(Foe::Army(j)) = self.foe {
            if j >= k {
                self.foe = Some(Foe::Army(j + 1));
            }
        }
        self.world.armies.insert(k, a);
    }

    // ------------------------------------------------------------------------------------
    // Parties: the hero and the armies
    // ------------------------------------------------------------------------------------

    /// The managed armies' indices with the hero first: the order of the original's loops
    /// over army records 0..N.
    fn parties(&self) -> Vec<Party> {
        let on_map = |a: &Army| managed(a) && !a.mind.fallen;
        std::iter::once(Party::Hero).chain((0..self.world.armies.len()).filter(|&j| on_map(&self.world.armies[j])).map(Party::Army)).collect()
    }

    /// Faction and attitudes: the hero is faction 1 with the header's first row.
    fn sides_of(&self, p: Party) -> (u8, [i8; 4]) {
        match p {
            Party::Hero => (1, self.world.relations[0]),
            Party::Army(j) => {
                let a = &self.world.armies[j];
                (a.faction, a.ai.relations)
            }
        }
    }

    /// The relation of army `i` to `p` (ai.md §3).
    fn relation_to(&self, i: usize, p: Party) -> i8 {
        let (fa, aa) = self.sides_of(Party::Army(i));
        let (fb, ab) = self.sides_of(p);
        relation_between((fa, &aa), (fb, &ab))
    }

    fn key_of(&self, p: Party) -> u32 {
        match p {
            Party::Hero => HERO,
            Party::Army(j) => self.world.armies[j].uid,
        }
    }

    /// Where `p` stands for the AI: the hero's logical cell, the cell he leaves while he
    /// steps (his record's cell 0x75c064 until the walk timer ends the step, 0x4ae8cc).
    fn cell_of(&self, p: Party, hero: &HeroCells) -> Tile {
        match p {
            Party::Hero => hero.at,
            Party::Army(j) => self.world.armies[j].tile(&self.world.map),
        }
    }

    fn ignored(&self, p: Party) -> bool {
        matches!(p, Party::Army(j) if self.world.armies[j].ai.ignored)
    }

    fn guard(&self, p: Party) -> bool {
        matches!(p, Party::Army(j) if stationary(&self.world.armies[j]))
    }

    /// At sea: a ship army, or the hero aboard.
    fn afloat(&self, p: Party) -> bool {
        match p {
            Party::Hero => self.aboard(),
            Party::Army(j) => self.world.armies[j].sails(),
        }
    }

    /// The building `p` stands in (the hero: the one he entered).
    fn standing_in(&self, p: Party) -> Option<usize> {
        match p {
            Party::Hero => self.location,
            Party::Army(j) => self.world.armies[j].mind.standing,
        }
    }

    /// The cell `p` steps to next (its own when it has no path).
    fn next_cell(&self, p: Party, hero: &HeroCells) -> Tile {
        match p {
            Party::Hero => hero.cells[1].unwrap_or(hero.at),
            Party::Army(j) => {
                let a = &self.world.armies[j];
                a.path.first().copied().unwrap_or_else(|| a.tile(&self.world.map))
            }
        }
    }

    /// The defence the hero's side fights with: his own building's, or a friend's
    /// (`Game::start_battle`).
    fn hero_defence(&self) -> i32 {
        let here = self.location.or_else(|| self.world.location_covering(self.tile()));
        here.map(|l| &self.world.locations[l]).filter(|l| l.owned() || l.attitude > 0).map_or(0, |l| l.garrison_defence.max(0))
    }

    /// Army `i`'s side in its battles: its living units (only the paid ones when it
    /// attacks, 0x49855c), its building defence and its spells.
    fn army_side(&self, i: usize, attacking: bool) -> (Side, Vec<usize>) {
        let c = &self.content;
        let a = &self.world.armies[i];
        let fought: Vec<usize> = (0..a.troops.len()).filter(|&k| a.troops[k].alive() && (!attacking || !a.troops[k].unpaid)).collect();
        let units = fought.iter().map(|&k| troop_unit(c, &a.troops[k])).collect();
        (Side { units, defence: a.mind.defence, strength_defence: a.mind.strength_bd, bare: false }, fought)
    }

    /// The hero's side as a target: all his living units, their strengths as his last recount
    /// counted them ([`Game::recount_hero`]: the defence he stood in then).
    fn hero_side(&self) -> Side {
        let units = self.squad.iter().enumerate().filter(|(k, u)| *k == 0 || u.alive()).map(|(_, u)| u.clone()).collect();
        let defence = self.hero_defence();
        Side { units, defence, strength_defence: self.hero_strength_bd, bare: false }
    }

    // ------------------------------------------------------------------------------------
    // Scores
    // ------------------------------------------------------------------------------------

    fn army_totals(&self, i: usize) -> Totals {
        let a = &self.world.armies[i];
        totals(&self.content, &a.troops, a.mind.defence)
    }

    /// The gold army `i` may spend (0x4a0530, ai.md §5): with `W = (wage bill + 2 ×
    /// Σ Round(Cost/CostRecrutDiv)) div 3`, a feudal army keeps `NeedUpkeepDay` days of `W`
    /// minus the income of the days between, the others one day; within `0..gold`.
    pub fn spare_gold(&self, i: usize) -> i32 {
        let a = &self.world.armies[i];
        let t = self.army_totals(i);
        let cost = (t.wages + 2 * t.recruit_sum) / 3;
        let n = self.content.options.need_upkeep_day;
        let m = &a.mind;
        let v = match a.ai.style {
            Style::Feudal => a.gold - n * cost + (m.income + m.village_avg) * (n - 1),
            _ => a.gold - cost + m.income + m.village_avg,
        };
        v.min(a.gold).max(0)
    }

    /// A fresh score of army `i` against `p` from a simulated battle ([`army_score`]).
    pub fn army_score_of(&self, i: usize, p: Party) -> i32 {
        let c = &self.content;
        let (side, _) = self.army_side(i, true);
        let (theirs, speed_c) = match p {
            Party::Hero => (self.hero_side(), self.hero_speed()),
            Party::Army(j) => (self.army_side(j, false).0, self.world.armies[j].speed.max(1)),
        };
        let s = self.sims.borrow_mut().get(c, &side, &theirs);
        let a = &self.world.armies[i];
        let pr = Priorities::of(&c.options, a.ai.model);
        let o = &c.options;
        army_score(s, a.ai.aggression, self.relation_to(i, p), pr.attack_army, o.zero_density, a.speed.max(1), speed_c, o.battle_end_turn)
    }

    /// Army `i`'s cached score against the party of key `key` (0 when never scored).
    fn cached(&self, i: usize, key: u32) -> i32 {
        self.world.armies[i].mind.scores.get(&key).copied().unwrap_or(0)
    }

    /// Scores army `i` against `p` afresh and marks the pair clean.
    fn rescore_pair(&mut self, i: usize, p: Party) {
        let v = self.army_score_of(i, p);
        let key = self.key_of(p);
        let m = &mut self.world.armies[i].mind;
        m.scores.insert(key, v);
        m.clean.insert(key);
    }

    /// Every pair with the army (or hero) of key `key` is to be rescored, both ways
    /// (0x4a26e8).
    pub(crate) fn mark_dirty(&mut self, key: u32) {
        for a in self.world.armies.iter_mut() {
            if a.uid == key {
                a.mind.clean.clear();
            } else {
                a.mind.clean.remove(&key);
            }
        }
    }

    /// Army `i`'s score of building `l` (0x4a0ba4, ai.md §6): the smallest positive of its
    /// village, purchase, attack and garrison parts, 0 for none, −1 when the attack part
    /// forbids it.
    pub fn building_score(&self, i: usize, l: usize) -> i32 {
        self.building_score_with(i, l, self.spare_gold(i), self.army_totals(i).strength)
    }

    /// [`Game::building_score`] with the army's spare gold and strength worked out.
    fn building_score_with(&self, i: usize, l: usize, spare: i32, strength: i32) -> i32 {
        let c = &self.content;
        let o = &c.options;
        let w = &self.world;
        let a = &w.armies[i];
        let b = &w.locations[l];
        // Bridges score 0, and so does every building for an army off the map.
        if b.kind.is_bridge() || a.mind.fallen {
            return 0;
        }
        let pr = Priorities::of(o, a.ai.model);
        let own = b.owner == Owner::Army(a.id);
        let leader = a.troops.first().map_or(Nature::Normal, |t| c.unit(t.unit).nature);
        // Village: gold in stock, a village or its own building.
        let stock = b.tribute_gold;
        let village = if stock != 0 && (b.kind == LocationKind::Village || own) {
            let (lo, hi) = pr.village;
            let v = fpu_round((1.0 - stock as f64 / (pr.gold_village + spare) as f64) * (hi - lo) as f64);
            let v = if v < 0 { lo } else { v + lo };
            if a.ai.style != Style::Feudal {
                v * 3
            } else {
                v
            }
        } else {
            0
        };
        // Purchase: recruits of its leader's Nature, goods it can afford.
        let purchase = if spare < 1 || b.attitude_to(a.faction) < 0 {
            0
        } else {
            let units = a.troops.len();
            let recruits = if units < MAX_UNITS {
                let (mut sum, mut n) = (0i64, 0i64);
                for r in b.recruits.iter().filter(|r| c.try_unit(r.unit).is_some_and(|d| d.nature == leader)) {
                    let count = r.stock.unwrap_or(0).max(0) as i64;
                    sum += count * c.unit(r.unit).cost as i64;
                    n += count;
                }
                if n < 1 {
                    0
                } else {
                    (sum / n * n.min((MAX_UNITS - units) as i64)) as i32
                }
            } else {
                0
            };
            let goods: Vec<i32> = random_goods(b).iter().filter_map(|&(_, item)| c.try_item(item)).map(|d| d.cost.abs()).filter(|&p| p <= spare).collect();
            let mut v = recruits;
            if !goods.is_empty() {
                let n = goods.len() as i64;
                let free = a.troops.iter().map(|t| t.worn.iter().filter(|s| s.is_none()).count() as i64).sum::<i64>();
                let wares = (goods.iter().map(|&p| p as i64).sum::<i64>() / n * n.min(free)) as i32;
                if v == 0 || (0 < v && v < wares) {
                    v = wares;
                }
            }
            if v < 1 {
                0
            } else {
                let (lo, hi) = pr.purchase;
                fpu_round((lo as i64 * pr.gold_purchase as i64) as f64 / v as f64 + (hi as i64 * pr.gold_purchase as i64) as f64 / spare as f64)
            }
        };
        // Attack: a hostile town (at −3 only), castle, fort or ruins it can beat.
        let attackable = assaults(a, b) && !matches!(b.kind, LocationKind::Tavern | LocationKind::Church | LocationKind::Smithy | LocationKind::Obelisk);
        let mut attack = 0;
        if has_garrison(b.kind) && attackable {
            attack = fpu_round((pr.attack_castle * 50) as f64 / (b.gold_income + 1) as f64);
            if attack == 0 {
                attack = 1;
            }
            let garrison = b.garrison.len() + b.stationed.len();
            if garrison < 1 {
                attack = attack / 4 + 1;
            } else {
                let (side, _) = self.army_side(i, true);
                let s = self.sims.borrow_mut().get(c, &side, &self.garrison_side(l));
                let (a0, b0) = (s.own, s.theirs);
                let (a1, b1) = (s.own_left.min(a0), s.theirs_left.min(b0));
                if !((a1 == a0 && b1 == b0) || s.turn as i32 == o.battle_end_turn) {
                    // A feudal army wins when its HP left, shifted by its aggression (÷100,
                    // never the ÷1000 of the army score), is still 1 or more; the garrison's
                    // result is not compared (the original's). A rogue when the garrison kept
                    // at most half its HP.
                    let wins = match a.ai.style {
                        Style::Feudal => a1 + fpu_round((a.ai.aggression as i64 * a0) as f64 / 100.0) as i64 >= 1,
                        _ => b1 <= b0 / 2,
                    };
                    if wins {
                        let gain = fpu_round(((1.0 - a1 as f64 / a0 as f64) + b1 as f64 / b0 as f64) * (o.zero_density as f64 * 30.0) + 1.0);
                        attack = (attack + gain).max(1);
                    } else {
                        attack = FORBIDDEN;
                    }
                }
            }
            if b.gold_income == 0 && garrison == 0 {
                attack *= 50;
            }
            // A stationary guard in it that it cannot beat forbids it.
            for p in self.parties() {
                if self.guard(p) && self.standing_in(p) == Some(l) {
                    let key = self.key_of(p);
                    if self.cached(i, key) < 0 || self.army_score_of(i, p) < 0 {
                        attack = FORBIDDEN;
                    }
                }
            }
        }
        // Garrison: its own building, its strength against the garrison's.
        let garrison = if own && has_garrison(b.kind) {
            let s = strength;
            let g = totals(c, &b.garrison, b.garrison_defence).strength;
            let (lo, hi) = pr.garrison;
            match s.cmp(&g) {
                std::cmp::Ordering::Less => fpu_round(((hi - lo) as i64 * s as i64) as f64 / g as f64) + lo,
                std::cmp::Ordering::Greater => fpu_round(((hi - lo) as i64 * g as i64) as f64 / s as f64) + lo,
                std::cmp::Ordering::Equal => 0,
            }
        } else {
            0
        };
        let mut best = 0;
        for v in [village, purchase, attack, garrison] {
            if v > 0 && (best == 0 || v < best) {
                best = v;
            }
        }
        if attack < 0 {
            -1
        } else {
            best
        }
    }

    /// The garrison of building `l` as a battle side: its troops and the player's units left
    /// there, living, with the building's defence.
    fn garrison_side(&self, l: usize) -> Side {
        let c = &self.content;
        let loc = &self.world.locations[l];
        let mut units: Vec<Unit> = loc.garrison.iter().filter(|t| t.alive()).map(|t| troop_unit(c, t)).collect();
        units.extend(loc.stationed.iter().filter(|s| s.unit.alive()).map(|s| s.unit.clone()));
        Side { units, defence: loc.garrison_defence, strength_defence: loc.garrison_defence, bare: loc.strengths_bare }
    }

    /// Army `i` scores every building afresh.
    pub(crate) fn rescore_buildings(&mut self, i: usize) {
        let (spare, strength) = (self.spare_gold(i), self.army_totals(i).strength);
        let scores: Vec<i32> = (0..self.world.locations.len()).map(|l| self.building_score_with(i, l, spare, strength)).collect();
        self.world.armies[i].mind.buildings = scores;
    }

    fn rescore_building(&mut self, i: usize, l: usize) {
        let v = self.building_score(i, l);
        let n = self.world.locations.len();
        let m = &mut self.world.armies[i].mind;
        m.buildings.resize(n, 0);
        m.buildings[l] = v;
    }

    fn stored_building(&self, i: usize, l: usize) -> i32 {
        self.world.armies[i].mind.buildings.get(l).copied().unwrap_or(0)
    }

    fn set_stored_building(&mut self, i: usize, l: usize, v: i32) {
        let n = self.world.locations.len();
        let m = &mut self.world.armies[i].mind;
        m.buildings.resize(n, 0);
        m.buildings[l] = v;
    }

    // ------------------------------------------------------------------------------------
    // Map load
    // ------------------------------------------------------------------------------------

    /// The AI's setup at map load and at every save load (0x4a1ff0, ai.md §7.1): every
    /// army stands in the building under it, all its pairs are dirty and it scores every
    /// building; at map load also its building defence, its first noon (today's 12:00, or
    /// tomorrow's when that has passed), its income and a village average of 50 (the
    /// building scores come first, so they see no income and no average). Then the owners of
    /// castles and forts count their income towards today's (on top of a loaded one too),
    /// the talk counters are zeroed, and every pair of AI armies within the first one's
    /// `AIDistance`, neither of them a stationary guard, is scored once.
    pub(crate) fn ai_init(&mut self, from_save: bool) {
        let now = self.clock.total_minutes();
        let day = MINUTES_PER_DAY as f64;
        let ids: Vec<usize> = (0..self.world.armies.len()).filter(|&i| managed(&self.world.armies[i])).collect();
        for &i in &ids {
            let cell = self.world.armies[i].tile(&self.world.map);
            let standing = self.world.location_covering(cell);
            let bill = army_wages(&self.content, &self.world.armies[i].troops);
            {
                let m = &mut self.world.armies[i].mind;
                m.standing = standing;
                m.clean.clear();
                // The recount comes before the defence is written (0x4a1ff0): a fresh record
                // counts none; a save's its loaded one.
                m.strength_bd = if from_save { m.defence } else { 0 };
                // A save keeps its bill; one from before Razdor kept it counts it afresh.
                if !from_save || m.wage_bill == 0 {
                    m.wage_bill = bill;
                }
                if !from_save {
                    m.income = 0;
                    m.village_avg = 0;
                }
            }
            self.rescore_buildings(i);
            if !from_save {
                let defence = standing.map_or(0, |l| self.world.locations[l].garrison_defence);
                let a = &mut self.world.armies[i];
                let m = &mut a.mind;
                m.defence = defence;
                m.idle = 0;
                // A fresh record has no path.
                m.walked = 0;
                m.no_path = true;
                a.budget = 0.0;
                let mut noon = (now / day).floor() * day + day / 2.0;
                if noon < now {
                    noon += day;
                }
                m.next_noon = noon;
                m.income = a.ai.extra_income;
                m.village_avg = 50;
                m.village_today = 50;
                // A stationary guard faces no direction (0x4a1ff0); the others face the map
                // load's direction 5, south.
                if stationary(a) {
                    a.mind.facing_none = true;
                } else {
                    a.mind.stand_facing = Some(LOAD_FACING);
                }
            }
        }
        // The armies waiting off the map get their records set up too (a building score is 0
        // for an army off the map).
        if !from_save {
            let map = &self.world.map;
            let locations = &self.world.locations;
            let footprints = |t: Tile| self.world.location_covering(t);
            let waiting: Vec<(usize, Option<usize>)> = self.world.inactive.iter().enumerate().map(|(k, a)| (k, footprints(a.tile(map)))).collect();
            for (k, standing) in waiting {
                let defence = standing.map_or(0, |l| locations[l].garrison_defence);
                let a = &mut self.world.inactive[k];
                let mut noon = (now / day).floor() * day + day / 2.0;
                if noon < now {
                    noon += day;
                }
                a.mind = AiMind { standing, defence, next_noon: noon, income: a.ai.extra_income, village_avg: 50, village_today: 50, buildings: vec![0; locations.len()], no_path: true, facing_none: stationary(a), stand_facing: (!stationary(a)).then_some(LOAD_FACING), ..AiMind::default() };
            }
        }
        let incomes: Vec<(u8, i32)> = self.world.locations.iter().filter(|l| l.kind.capturable()).filter_map(|l| match l.owner {
            Owner::Army(id) => Some((id, l.gold_income)),
            _ => None,
        }).collect();
        for (id, income) in incomes {
            if let Some(a) = self.world.armies.iter_mut().chain(self.world.inactive.iter_mut()).find(|a| a.id == id && managed(a)) {
                a.mind.income += income;
            }
        }
        for a in self.world.armies.iter_mut().chain(self.world.inactive.iter_mut()) {
            a.talk = 0;
            a.mind.talk.clear();
        }
        if !from_save {
            // Everyone counts as paid at the start (a save from before the pay records too).
            let stamp = now as u64;
            let troops = self.world.armies.iter_mut().chain(self.world.inactive.iter_mut()).filter(|a| managed(a)).flat_map(|a| a.troops.iter_mut());
            for t in troops.filter(|t| t.last_paid == 0) {
                t.last_paid = stamp;
            }
        }
        for &k in &ids {
            for &j in &ids {
                let map = &self.world.map;
                let (a, b) = (&self.world.armies[k], &self.world.armies[j]);
                if k == j || stationary(a) || stationary(b) {
                    continue;
                }
                if map.grid.octile(a.tile(map), b.tile(map)) <= target_range(&self.content.options, a.ai.style) {
                    self.rescore_pair(k, Party::Army(j));
                }
            }
        }
    }
}

/// The market goods of building `l` the AI deals with, with their stock index: the random
/// ones only (the original keeps the map's fixed goods as negative ids and skips them).
fn random_goods(l: &Location) -> Vec<(usize, ItemId)> {
    let Some(shop) = &l.shop else { return Vec::new() };
    shop.goods().into_iter().enumerate().filter(|&(k, _)| !shop.is_fixed(k)).collect()
}

impl Game {
    // ------------------------------------------------------------------------------------
    // The driver and the step clock
    // ------------------------------------------------------------------------------------

    /// The armies as the hero stops (0x4ad8a0), once the stop has opened its windows (the
    /// interface calls it after their chords; a battle's start and the next tick do too):
    /// every army the AI steers, in id order, ends a step under way (Razdor has none: its
    /// steps end as they start) and, when it faces a next step (direction below 8), has a
    /// patrol radius above 0 and stands in no building, restarts its idle animation at a
    /// `Random(3000)` ms offset. The offset only times the sprite; the draw is what counts.
    pub fn armies_snap(&mut self) {
        if !std::mem::take(&mut self.snap_due) {
            return;
        }
        for k in 0..self.world.armies.len() {
            let a = &self.world.armies[k];
            if managed(a) && !a.mind.facing_none && a.patrol_radius > 0 && a.mind.standing.is_none() {
                let _idle_offset_ms = self.rng.random(super::rng::ARMY_IDLE_DRAW);
            }
        }
    }

    /// The snap of a stop that comes before the stop's windows (an AI army's attack or
    /// greeting, 0x4ade3c); the stop has no other.
    pub(crate) fn snap_now(&mut self) {
        self.snap_due = true;
        self.armies_snap();
        self.snapped = true;
    }

    /// The AI's part of a tick of `minutes` (0x4ade3c, called every frame): beaten armies
    /// whose time has come return, then every army the AI steers banks the tick (a stationary
    /// guard does nothing at all, a healing one stands still) and walks by the step clock
    /// (0x4a399c, [`Game::ai_start`]): a step starts when the bank covers it and arrives at
    /// the end of its play time, the next step starting only after the arrival. The arrivals
    /// of all the armies come in the order of their times; arrivals at the same moment (the
    /// same frame) in army order. This is the original's order at a steady frame rate as the
    /// frames get short: each frame runs the armies in order, a call makes at most one
    /// arrival, and the next step starts at the next frame. A midnight inside the tick comes
    /// after the arrivals up to its moment (0x4a1998 ends the frame's advance); the clock
    /// reads each arrival's moment. `start` is the tick's first minute, `midnights` the
    /// midnights in it; the ones after the last arrival are returned.
    /// `hero` are the hero's cells while the tick plays, `hero_end` those once his step has
    /// ended: the arrivals at the tick's very end come in the frame where the walk timer has
    /// already ended his step (0x4ae8cc), so they see him on his new cell, facing the step he
    /// took (0x4ae8e0), before the AI's advance of that frame (0x4ade3c).
    pub(crate) fn ai_move(&mut self, minutes: f32, hero: &HeroCells, hero_end: &HeroCells, start: f64, midnights: &[f64]) -> Vec<f64> {
        let end = self.clock.total_minutes();
        self.ai_respawns(end);
        let tick = cmin(minutes);
        // The tick's start: every army banks it, the step clock's window opens.
        let mut queue: BTreeSet<(i64, u32, u8, u32)> = BTreeSet::new();
        let mut totals: BTreeMap<u32, i64> = BTreeMap::new();
        let uids: Vec<u32> = self.world.armies.iter().filter(|a| managed(a)).map(|a| a.uid).collect();
        for uid in uids {
            let Some(i) = self.army_by_uid(uid) else { continue };
            let a = &mut self.world.armies[i];
            if stationary(a) || end <= a.mind.busy_until {
                continue;
            }
            a.budget = (a.budget + minutes).min(super::world::AI_BUDGET_CAP);
            totals.insert(uid, cmin(a.budget));
            let id = a.id;
            if self.is_foe(i) {
                continue;
            }
            queue.insert((0, 0, id, uid));
        }
        let mut pending: BTreeMap<u32, Pending> = BTreeMap::new();
        let mut midnights: Vec<f64> = midnights.to_vec();
        while let Some((t, seq, id, uid)) = queue.pop_first() {
            let at = start + t as f64 / 100.0;
            // A midnight comes after the calls of the moment it falls in (0x4a1998).
            while midnights.first().is_some_and(|&m| m < at) {
                let m = midnights.remove(0);
                self.clock.set_total_minutes(m);
                self.midnight();
            }
            self.clock.set_total_minutes(at);
            let goes_on = |g: &Game| t < tick && g.army_by_uid(uid).is_some_and(|i| !g.is_foe(i));
            if let Some(p) = pending.remove(&uid) {
                // The arrival, as its play time runs out; the next step starts at the next
                // frame, within this tick only.
                self.ai_finish(&p, if t >= tick { hero_end } else { hero });
                if goes_on(self) {
                    queue.insert((t, seq + 1, id, uid));
                }
                continue;
            }
            let total = totals.get(&uid).copied().unwrap_or(0);
            let Some((p, play)) = self.ai_start(uid, hero, tick, total, tick - t) else { continue };
            if play < 1 {
                // A play time under one centi-minute runs out in the call that starts it. A
                // step of no cost that took no time steps no further this tick: the original
                // takes one such step per frame *(Razdor has no frames: it stops there)*.
                self.ai_finish(&p, hero);
                if p.minutes > 0.0 && goes_on(self) {
                    queue.insert((t, seq + 1, id, uid));
                }
                continue;
            }
            pending.insert(uid, p);
            queue.insert((t + play, 0, id, uid));
        }
        self.clock.set_total_minutes(end);
        midnights
    }

    /// Army `uid` takes, one after another, every step its bank covers (tests: the steps of
    /// one army alone, each arriving at once).
    #[cfg(test)]
    pub(crate) fn ai_walk(&mut self, uid: u32, hero: &HeroCells) {
        loop {
            let Some((p, _)) = self.ai_start(uid, hero, 3000, 3000, 3000) else { return };
            self.ai_finish(&p, hero);
            // A cell of no cost would step forever: once.
            if p.minutes <= 0.0 || self.army_by_uid(uid).is_none_or(|i| self.is_foe(i)) {
                return;
            }
        }
    }

    /// The start of army `uid`'s next step by the step clock (0x4a399c), when its bank
    /// covers it: `cost(cell left) × speed` minutes, ×1.5 diagonally; with no path it steps
    /// in place on its own cell's cost (no diagonal factor); after a respawn or an activation
    /// the stored cost is 0. The bank pays it. Whether a step into one of the hero's cells
    /// moves is decided now. Its play time (centi-minutes): the cost scaled by the tick over
    /// the bank as the tick began (`total`) when what is left in the bank also covers the
    /// step after it, else the rest of the tick's `window`; never more than the window. The
    /// step after is charged on the cell this step enters, times the weight its path buffer
    /// holds for that node when the path goes on from it, raw when it does not.
    fn ai_start(&mut self, uid: u32, hero: &HeroCells, tick: i64, total: i64, window: i64) -> Option<(Pending, i64)> {
        let i = self.army_by_uid(uid)?;
        let (need, next, moves, after) = {
            let w = &self.world;
            let a = &w.armies[i];
            let map = &w.map;
            let here = a.tile(map);
            let next = a.path.first().copied();
            let to = next.unwrap_or(here);
            let left = self.ai_cost(a, here);
            let speed = a.speed.max(1);
            let need = match next {
                _ if a.mind.free_step => 0.0,
                Some(t) => step_minutes(map.grid, here, t, left, speed),
                None => left as f32 * speed as f32,
            };
            // With no path the step after is priced on the cell its direction points at
            // (the map load's south, or none).
            let ahead = match (next, a.mind.stand_facing) {
                (None, Some(d)) => (here.0 + d.0, here.1 + d.1),
                _ => to,
            };
            let raw = self.ai_cost(a, ahead) as i64 * speed as i64 * 100;
            let after = match (next, a.path.get(1)) {
                // The path goes on from the cell it enters: that step's weight.
                (Some(t), Some(&u)) => raw * map.grid.weight(t, u) as i64 / 2,
                // It enters the path's last cell: whatever weight the buffer holds there.
                (Some(_), None) => {
                    let weight = a.mind.path_weights.get(a.mind.walked.max(0) as usize + 1).copied().unwrap_or(0);
                    let weight = if weight == 0 { map.grid.direction_weight(0) as i64 } else { weight as i64 };
                    raw * weight / 2
                }
                (None, _) => raw,
            };
            (need, next, !hero.cells.contains(&Some(to)), after)
        };
        let a = &mut self.world.armies[i];
        if a.budget < need {
            return None;
        }
        a.budget -= need;
        a.mind.free_step = false;
        let cost = cmin(need);
        let play = if cmin(a.budget) >= after {
            if total > 0 { cost * tick / total } else { 0 }
        } else {
            window
        };
        Some((Pending { uid, next, moves, minutes: need }, play.min(window)))
    }

    /// The arrival of a step [`Game::ai_start`] began: the step clock's bookkeeping, the
    /// re-plan and the arrival rules ([`Game::ai_stepped`], [`Game::ai_arrival`]).
    fn ai_finish(&mut self, p: &Pending, hero: &HeroCells) {
        let Some(i) = self.army_by_uid(p.uid) else { return };
        self.ai_stepped(i, p.next, p.moves, p.minutes);
        self.ai_arrival(p.uid, hero);
    }

    /// Cost units of cell `t` on army `a`'s map: LAND, or SHIP for a ship army (0 closed).
    fn ai_cost(&self, a: &Army, t: Tile) -> u16 {
        if a.sails() {
            self.world.sea_step(t).unwrap_or(0)
        } else {
            self.world.map.cost(t).unwrap_or(0)
        }
    }

    /// The step-clock bookkeeping of a step that ended (0x4a399c): a real step moves the army
    /// on (unless the hero's cell barred it), counts one step off its re-plan countdown and
    /// ends its idle plans; reaching the end of its path, or a closed next cell, draws new
    /// wander points and forces a re-plan. A step in place (no path) clears "just respawned",
    /// draws new wander points, forces a re-plan and counts one more idle plan.
    fn ai_stepped(&mut self, i: usize, next: Option<Tile>, moves: bool, minutes: f32) {
        let closed = |g: &Game, t: Tile| g.ai_cost(&g.world.armies[i], t) == 0;
        let after = next.filter(|_| moves).and_then(|_| self.world.armies[i].path.get(1).copied()).or_else(|| next.filter(|_| !moves));
        let next_closed = after.is_some_and(|t| closed(self, t));
        let map = &self.world.map;
        let a = &mut self.world.armies[i];
        a.arrived = true;
        // The original's path index moves on with a real step; it is still on its path when
        // that index is below the path's length.
        let on_path = next.is_some() || (!moves && !a.mind.no_path && a.mind.walked == 0);
        if moves {
            a.mind.walked += 1;
        }
        // For drawing, every step takes its time on the figure's walk: one in place, or one
        // the hero's cell barred, stands for its play time (0x4a399c plays each in turn).
        let stays = !(moves && next.is_some() && on_path);
        if stays {
            a.walk.points.push(a.pos);
            a.walk.minutes.push(minutes);
        }
        let renew = match next {
            Some(t) if on_path => {
                if moves {
                    a.pos = map.center(t);
                    a.path.remove(0);
                    a.walk.points.push(a.pos);
                    a.walk.minutes.push(minutes);
                }
                a.mind.countdown -= 1;
                a.mind.idle = 0;
                a.path.is_empty() || next_closed
            }
            None if on_path => {
                // Standing on a one-cell path, its own cell barred by the hero's: the index
                // stays on its only cell, so it counts as a step that reached the end.
                a.mind.countdown -= 1;
                a.mind.idle = 0;
                true
            }
            _ => {
                // Past the end of its path (a step in place, or one the hero's cell barred
                // when the index had already moved on along a path planned away, or with no
                // path at all): an idle plan.
                a.mind.just_respawned = false;
                a.mind.idle += 1;
                true
            }
        };
        if renew {
            a.mind.countdown = 0;
            a.mind.walked = 0;
            a.path.clear();
            self.ai_wander(i);
        }
    }

    /// The four wander points of army `i` (0x4a2550): x then y for each, inside its patrol
    /// box (its centre ± radius, clamped to the map) when it patrols, anywhere on the map when
    /// it does not; a point on its own cell gets x = 0, which no plan seeds.
    pub(crate) fn ai_wander(&mut self, i: usize) {
        let (w, h) = (self.world.map.w, self.world.map.h);
        let (c, r, here, patrols) = {
            let a = &self.world.armies[i];
            (a.patrol_centre(), a.patrol_radius, a.tile(&self.world.map), a.patrols)
        };
        let mut pts = [(0, 0); WANDER_POINTS];
        for p in pts.iter_mut() {
            *p = if patrols {
                let (x0, x1) = ((c.0 - r).max(0), (c.0 + r).min(w - 1));
                let (y0, y1) = ((c.1 - r).max(0), (c.1 + r).min(h - 1));
                let x = x0 + self.rng.random(x1 - x0 + 1);
                (x, y0 + self.rng.random(y1 - y0 + 1))
            } else {
                let x = self.rng.random(w);
                (x, self.rng.random(h))
            };
        }
        for p in pts.iter_mut() {
            if *p == here {
                p.0 = 0;
            }
        }
        self.world.armies[i].mind.wander = pts;
    }

    /// The rest of an arrival (0x4a399c's tail, then 0x4a548c): the octile distance to every
    /// other party on the map (0 on the same cell, which then counts as absent); +1 to the
    /// talk counter towards each at a distance; a re-plan when the countdown ran out or any
    /// is within `AIGetPathDistance`; then the arrival rules. An attack on or a greeting of
    /// the hero is kept for [`Game::ai_contact`].
    fn ai_arrival(&mut self, uid: u32, hero: &HeroCells) {
        let Some(i) = self.army_by_uid(uid) else { return };
        let here = self.world.armies[i].tile(&self.world.map);
        let grid = self.world.map.grid;
        let reach = self.content.options.ai_get_path_distance;
        let mut dist: Vec<(Party, i32)> = Vec::new();
        let mut replan = self.world.armies[i].mind.countdown < 1;
        for p in self.parties() {
            if p == Party::Army(i) {
                continue;
            }
            let d = grid.octile(here, self.cell_of(p, hero));
            if 0 < d && d <= reach {
                replan = true;
            }
            if d > 0 {
                let key = self.key_of(p);
                let a = &mut self.world.armies[i];
                if key == HERO {
                    a.talk = a.talk.saturating_add(1);
                } else {
                    let c = a.mind.talk.entry(key).or_insert(0);
                    *c = c.saturating_add(1);
                }
            }
            dist.push((p, d));
        }
        if replan {
            self.ai_plan(i, &dist, hero);
        }
        // The step clock ends by setting the direction of the next step from the path, 8
        // when it has no next cell (0x4a399c), before the arrival rules run (0x4ade3c).
        let here = self.world.armies[i].tile(&self.world.map);
        let a = &mut self.world.armies[i];
        a.mind.facing_none = a.path.is_empty();
        a.mind.stand_facing = a.path.first().map(|t| (t.0 - here.0, t.1 - here.1));
        if let Some(c) = self.ai_arrive(uid, hero) {
            if let Some(i) = self.army_by_uid(uid) {
                self.world.armies[i].mind.contact = Some(c);
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // The planner
    // ------------------------------------------------------------------------------------

    /// Army `i` plans (0x4a2d88, ai.md §7), with `dist` its distance to every other party:
    /// 1. the dirty pairs within `AIDistance` are rescored (farther ones keep their last
    ///    score, still used below);
    /// 2. with a heal or resurrection bill and spare gold, every friendly service building's
    ///    stored score is lowered to the healing value `h` (3h at a town or church when a
    ///    resurrection is due), for good until it is rescored;
    /// 3. seeds: every building with a positive score (inside its patrol box when it
    ///    patrols), every party by its score or talk value, the wander points; a danger
    ///    (negative value) pushes two repulsion cones instead;
    /// 4. closed: forbidden buildings' footprints, stationary guards, and the armies near an
    ///    ignored one (or near it when it is ignored);
    /// 5. one flood from all seeds, the cells of the parties within `AIGetPathDistance` and
    ///    the cells they step to next erased, the path read by steepest descent. With no seed
    ///    kept it stands (an empty path) and the countdown is left as it is.
    pub(crate) fn ai_plan(&mut self, i: usize, dist: &[(Party, i32)], hero: &HeroCells) {
        #[cfg(test)]
        if self.world.armies[i].mind.scripted {
            return;
        }
        let c = self.content.clone();
        let o = &c.options;
        let (style, model, uid) = {
            let a = &self.world.armies[i];
            (a.ai.style, a.ai.model, a.uid)
        };
        let range = target_range(o, style);
        for &(p, d) in dist {
            let key = self.key_of(p);
            if 0 < d && d <= range && !self.world.armies[i].mind.clean.contains(&key) {
                self.rescore_pair(i, p);
            }
        }
        self.ai_stats.paths += 1;
        let pr = Priorities::of(o, model);
        let (w, h) = (self.world.map.w, self.world.map.h);
        let idx = |t: Tile| -> Option<usize> { (t.0 >= 0 && t.1 >= 0 && t.0 < w && t.1 < h).then(|| (t.1 * w + t.0) as usize) };
        let mut seeds: Vec<(Tile, u32)> = Vec::new();
        let mut mult = vec![1u16; (w.max(0) * h.max(0)) as usize];
        // Healing lowers the stored building scores.
        let t = self.army_totals(i);
        let spare = self.spare_gold(i);
        if (t.heal_bill > 0 || t.res_bill > 0) && spare > 0 {
            let (lo, hi) = pr.heal;
            let mut v = fpu_round((1.0 - t.missing as f64 / t.max_living as f64) * (hi - lo) as f64 + lo as f64);
            if spare < t.heal_bill {
                v = fpu_round((t.heal_bill as i64 * v as i64) as f64 / spare as f64);
            }
            let faction = self.world.armies[i].faction;
            for l in 0..self.world.locations.len() {
                let b = &self.world.locations[l];
                if !b.services || b.attitude_to(faction) < 0 {
                    continue;
                }
                let cap = if t.res_bill >= 1 && matches!(b.kind, LocationKind::Town | LocationKind::Church) { v.wrapping_mul(3) } else { v };
                if cap < self.stored_building(i, l) {
                    self.set_stored_building(i, l, cap);
                }
            }
        }
        let a = &self.world.armies[i];
        let here = a.tile(&self.world.map);
        let box_has = |t: Tile| in_patrol(a, t);
        if !a.ai.no_buildings {
            for (l, b) in self.world.locations.iter().enumerate() {
                let v = self.stored_building(i, l);
                if v > 0 && (!a.patrols || box_has(b.tile)) {
                    seeds.push((b.tile, v as u32));
                }
            }
        }
        for &(p, _) in dist {
            let r = self.relation_to(i, p);
            let key = self.key_of(p);
            let mut v = if r < 0 {
                self.cached(i, key)
            } else if a.ai.no_talk {
                0
            } else {
                let cnt = if key == HERO { a.talk } else { a.mind.talk.get(&key).copied().unwrap_or(0) };
                if cnt < 1 {
                    cnt / 100
                } else {
                    (TALK_COOLED - cnt).max(0) + pr.talk
                }
            };
            let f = if self.guard(p) { GUARD_SLOPE } else { 1.0 };
            if a.ai.player_only {
                if p != Party::Hero && v > 0 {
                    v = 0;
                }
                if p == Party::Hero && v == 0 {
                    v = 1;
                }
            }
            if self.ignored(p) {
                v = 0;
            }
            let cell = self.cell_of(p, hero);
            if v < 1 {
                if v < 0 && a.sails() == self.afloat(p) {
                    repulsion(&mut mult, w, h, cell, f, -v);
                    repulsion(&mut mult, w, h, cell, f * STEEP, v.wrapping_mul(-5));
                }
            } else if (!a.patrols || (box_has(here) && box_has(cell))) && !a.mind.just_respawned {
                seeds.push((cell, v as u32));
            }
        }
        // Razdor keeps AI armies to the hero's roads (its choice since 0.2.0, kept over the
        // original's planner): the buildings an army may not walk through ([`bars_army`])
        // are closed as the forbidden ones, except the one it stands in and the ones it
        // seeks (a positive score: a seed).
        let standing = self.world.location_covering(here);
        for (l, b) in self.world.locations.iter().enumerate() {
            let score = self.stored_building(i, l);
            if score < 0 || (score == 0 && Some(l) != standing && bars_army(a, b)) {
                let (x0, y0) = (b.anchor.0 - b.size.0 + 1, b.anchor.1 - b.size.1 + 1);
                fill_rect(&mut mult, w, (x0, y0), b.size);
            }
        }
        let parties = self.parties();
        for &p in &parties {
            if self.guard(p) {
                if let Some(k) = idx(self.cell_of(p, hero)) {
                    mult[k] = 0;
                }
            }
        }
        let grid = self.world.map.grid;
        for &p in parties.iter().filter(|&&p| p != Party::Hero) {
            if (a.ai.ignored || self.ignored(p)) && grid.octile(here, self.cell_of(p, hero)) < NEAR_IGNORED {
                if let Some(k) = idx(self.cell_of(p, hero)) {
                    mult[k] = 0;
                }
            }
        }
        if !a.ai.no_random || a.mind.idle > IDLE_FOR_RANDOM {
            for &pt in &a.mind.wander {
                if pt.0 > 0 && idx(pt).is_some_and(|k| mult[k] != 0) {
                    seeds.push((pt, pr.random as u32));
                }
            }
        }
        let world = &self.world;
        let costs = if a.sails() { world.map.water_costs() } else { world.map.land_costs() };
        let mut field = world.map.flood_maps(costs, &mult, &seeds, here);
        let reach = o.ai_get_path_distance;
        for &(p, d) in dist {
            if 0 < d && d <= reach {
                field.erase(self.cell_of(p, hero));
                field.erase(self.next_cell(p, hero));
            }
        }
        let path = (field.kept > 0).then(|| world.map.descend(&field, here));

        let a = &mut self.world.armies[i];
        // A path read puts its index back at its start; with no seed the path is one cell
        // long and the index stays where it was (0x4a2d88).
        a.mind.no_path = false;
        match path {
            Some(path) => {
                let grid = self.world.map.grid;
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
                a.mind.countdown = reach;
                a.mind.walked = 0;
            }
            None => a.path.clear(),
        }
        let _ = uid;
    }
}

/// Closes the footprint `size` at top-left `(x0, y0)` on the multiplier map (0x47650c): the
/// bitmap's rows from `y0`, `size.0` cells from `x0` each, at least one row.
fn fill_rect(mult: &mut [u16], w: i32, (x0, y0): Tile, size: (i32, i32)) {
    for r in 0..size.1.max(1) {
        for c in 0..size.0.max(0) {
            let k = (y0 + r) as i64 * w as i64 + (x0 + c) as i64;
            if let Some(m) = usize::try_from(k).ok().and_then(|k| mult.get_mut(k)) {
                *m = 0;
            }
        }
    }
}

impl Game {
    // ------------------------------------------------------------------------------------
    // Arrival
    // ------------------------------------------------------------------------------------

    /// The arrival rules of army `uid` (0x4a548c, ai.md §8–9):
    /// 1. its home is rescored when it does not own it; after 12:00 its noon runs and every
    ///    building is rescored; corpses older than `MaxTimeResurection` are dropped;
    /// 2. contacts, the hero first: a friendly party's talk counter grows by relation + 1; a
    ///    hostile neighbour (|dx| ≤ 1, |dy| ≤ 1) in no building, on a bridge or in its own
    ///    building is attacked when the cached score is positive and it is not ignored (the
    ///    hero: [`Contact::Attack`]); one inside someone else's building triples both scores;
    ///    a friendly neighbour is greeted (the hero only after his step);
    /// 3. in a building: assault and capture, village gold, shopping, healing, resurrection,
    ///    hiring, garrison buying and reshuffle; the building's stored score is zeroed.
    ///
    /// Returns what it did to the hero. An army beaten in a fight of its own arrival is off
    /// the map, but the arrival goes on with its record as the original's does (0x4a548c):
    /// it may fight again with nobody (and lose again, its gold and wage bill to the winner),
    /// collect a village's gold, hire, heal or raise its dead; it leaves the map at the end,
    /// and what it did to the hero then is dropped *(guess: the original would open a
    /// battle or a meeting with the beaten army)*.
    pub(crate) fn ai_arrive(&mut self, uid: u32, hero: &HeroCells) -> Option<Contact> {
        // An attack on the hero counts only in the frame his step ends (his step flag
        // 0x75e0c7, 0x4ade3c): not while he waits or stands, nor mid-step.
        let result = self.ai_arrive_rules(uid, hero).filter(|c| *c != Contact::Attack || hero.boundary);
        let i = self.army_by_uid(uid)?;
        if self.world.armies[i].mind.fallen {
            self.world.armies[i].mind.fallen = false;
            self.army_beaten(i, Beaten::ByAi);
            return None;
        }
        result
    }

    fn ai_arrive_rules(&mut self, uid: u32, hero: &HeroCells) -> Option<Contact> {
        let now = self.clock.total_minutes();
        let i = self.army_by_uid(uid)?;
        let mut result = None;
        if let Some(home) = self.world.armies[i].home {
            if self.world.locations[home].owner != Owner::Army(self.world.armies[i].id) {
                self.rescore_building(i, home);
            }
        }
        if self.world.armies[i].mind.next_noon < now {
            self.ai_noon(i, now);
            self.rescore_buildings(i);
        }
        let window = self.content.options.max_time_resurection.max(0) as f64;
        if self.army_totals(i).res_bill > 0 {
            self.world.armies[i].troops.retain(|t| t.died_at.is_none_or(|d| d as f64 + window >= now));
        }
        // Contacts.
        // The hero's step flag (0x75e0c7) is set only in the frame where his step ends (the
        // walk timer, 0x4ae977) and cleared at the next: an attack or a greeting counts only
        // for an arrival in that frame (0x4ade3c), the tick's end of his step.
        let hero_step = hero.boundary;
        let here = self.world.armies[i].tile(&self.world.map);
        let parties: Vec<(Party, u32)> = self.parties().into_iter().map(|p| (p, self.key_of(p))).collect();
        for (p, key) in parties {
            let Some(i) = self.army_by_uid(uid) else { return result };
            let p = match p {
                Party::Hero => Party::Hero,
                Party::Army(_) => match self.army_by_uid(key) {
                    Some(j) => Party::Army(j),
                    None => continue,
                },
            };
            if p == Party::Army(i) {
                continue;
            }
            let r = self.relation_to(i, p);
            if r >= 0 {
                self.add_talk(i, key, r as i32 + 1);
            }
            let there = self.cell_of(p, hero);
            if (here.0 - there.0).abs() >= 2 || (here.1 - there.1).abs() >= 2 {
                continue;
            }
            if r < 0 {
                let b = self.world.location_covering(there);
                let open = match b {
                    None => true,
                    Some(l) => {
                        let loc = &self.world.locations[l];
                        loc.kind.is_bridge()
                            || match p {
                                Party::Hero => loc.owner == Owner::Player,
                                Party::Army(j) => loc.owner == Owner::Army(self.world.armies[j].id),
                            }
                    }
                };
                if open {
                    if self.cached(i, key) > 0 && !self.ignored(p) {
                        match p {
                            Party::Hero => result = Some(Contact::Attack),
                            Party::Army(j) => {
                                self.ai_battle(i, Defender::Army(j));
                            }
                        }
                    }
                } else {
                    // Sheltered in someone else's building: no fight, both scores tripled.
                    let triple = |v: i32| if v > 0 { (v * 3).min(TRIPLED_CAP) } else { v.min(TRIPLED_CAP) };
                    let v = triple(self.cached(i, key));
                    self.world.armies[i].mind.scores.insert(key, v);
                    if let Party::Army(j) = p {
                        let v = triple(self.cached(j, uid));
                        self.world.armies[j].mind.scores.insert(uid, v);
                    }
                }
            } else if p != Party::Hero || hero_step {
                let mine = self.talk_of(i, key);
                if p == Party::Hero && mine > 0 {
                    result = Some(Contact::Greet);
                }
                if p != Party::Hero || mine > 0 {
                    self.world.armies[i].mind.countdown = 0;
                    self.set_talk(i, key, TALKED);
                    self.ai_wander(i);
                }
                if let Party::Army(j) = p {
                    self.world.armies[j].mind.countdown = 0;
                    self.set_talk(j, uid, TALKED);
                }
            }
        }
        let i = self.army_by_uid(uid)?;
        let here = self.world.armies[i].tile(&self.world.map);
        let Some(l) = self.world.location_covering(here) else {
            let m = &mut self.world.armies[i].mind;
            if m.standing.is_some() {
                m.standing = None;
                m.defence = 0;
                m.strength_bd = 0;
                self.recount_bill(i);
            }
            return result;
        };
        self.ai_in_building(uid, l, now, &mut result);
        result
    }

    fn talk_of(&self, i: usize, key: u32) -> i32 {
        let a = &self.world.armies[i];
        if key == HERO {
            a.talk
        } else {
            a.mind.talk.get(&key).copied().unwrap_or(0)
        }
    }

    fn set_talk(&mut self, i: usize, key: u32, v: i32) {
        let a = &mut self.world.armies[i];
        if key == HERO {
            a.talk = v;
        } else {
            a.mind.talk.insert(key, v);
        }
    }

    fn add_talk(&mut self, i: usize, key: u32, d: i32) {
        let v = self.talk_of(i, key).saturating_add(d);
        self.set_talk(i, key, v);
    }

    /// Army `uid` arrived in building `l` (ai.md §9).
    fn ai_in_building(&mut self, uid: u32, l: usize, now: f64, result: &mut Option<Contact>) {
        let Some(i) = self.army_by_uid(uid) else { return };
        let assault = assaults(&self.world.armies[i], &self.world.locations[l]);
        if !assault {
            self.world.armies[i].mind.standing = Some(l);
        } else {
            let owner = self.world.locations[l].owner;
            // The player's building with him inside: he is attacked.
            if owner == Owner::Player && self.location == Some(l) {
                *result = Some(Contact::Attack);
                return;
            }
            let mut won = self.ai_battle(i, Defender::Garrison(l));
            if let Owner::Army(oid) = owner {
                let defender = self.world.armies.iter().position(|b| b.id == oid && managed(b) && b.mind.standing == Some(l));
                if let (Some(j), Some(i)) = (defender, self.army_by_uid(uid)) {
                    if !self.world.armies[j].ai.ignored && i != j {
                        won = self.ai_battle(i, Defender::Army(j));
                    }
                }
            }
            let Some(i) = self.army_by_uid(uid) else { return };
            if !won {
                self.world.armies[i].mind.standing = None;
            } else {
                let (id, faction, relations) = {
                    let a = &self.world.armies[i];
                    (a.id, a.faction, a.ai.relations)
                };
                let kind = self.world.locations[l].kind;
                let town = kind == LocationKind::Town && attitude_of(&relations, self.world.locations[l].faction) == -3;
                let was_players = self.world.locations[l].owned();
                if town || matches!(kind, LocationKind::Village | LocationKind::Castle | LocationKind::Fort) {
                    let loc = &mut self.world.locations[l];
                    loc.take_sides(faction, relations);
                    loc.owner = Owner::Army(id);
                    loc.cleared = false;
                    if self.world.armies[i].home.is_none() {
                        self.world.armies[i].home = Some(l);
                    }
                    self.ai_stats.captures += 1;
                    let text = crate::trf!("{name} took {place}.", name = army_name(&self.world.armies[i]), place = building_name(&self.world.locations[l]));
                    let tile = self.world.locations[l].tile;
                    self.report(text, tile, was_players);
                }
                if matches!(kind, LocationKind::Altar | LocationKind::Ruins) {
                    let loc = &mut self.world.locations[l];
                    loc.owner = Owner::Neutral;
                    loc.take_sides(3, [0; 4]);
                }
                let d = self.world.locations[l].garrison_defence;
                let m = &mut self.world.armies[i].mind;
                m.standing = Some(l);
                m.defence = d;
            }
        }
        let Some(i) = self.army_by_uid(uid) else { return };
        let (id, style) = (self.world.armies[i].id, self.world.armies[i].ai.style);
        let own = self.world.locations[l].owner == Owner::Army(id);
        if own {
            self.world.armies[i].mind.defence = self.world.locations[l].garrison_defence;
        }
        if self.world.armies[i].mind.standing.is_none() || self.world.locations[l].kind.is_bridge() {
            self.set_stored_building(i, l, 0);
            return;
        }
        // Village gold: feudal armies take any village's whole stock; its mana is lost.
        if style == Style::Feudal && self.world.locations[l].kind == LocationKind::Village && self.world.locations[l].tribute_gold != 0 {
            let loc = &mut self.world.locations[l];
            let gold = std::mem::take(&mut loc.tribute_gold);
            loc.tribute_mana = 0;
            let a = &mut self.world.armies[i];
            a.gold += gold;
            a.mind.village_today += gold;
        }
        let mut changed = false;
        if style.lordly() {
            self.ai_shop(i, l);
            let welcome_here = welcome(&self.world.armies[i], &self.world.locations[l]);
            if welcome_here {
                if self.world.locations[l].services {
                    changed |= self.ai_heal(i, l, now);
                    changed |= self.ai_hire(i, l);
                }
                let level = self.world.armies[i].ai.garrison_level;
                let keeps = |g: &Game| {
                    let loc = &g.world.locations[l];
                    level != 0 && loc.owner == Owner::Army(id) && matches!(loc.kind, LocationKind::Town | LocationKind::Castle | LocationKind::Fort) && g.stored_building(i, l) > 0
                };
                if keeps(self) {
                    self.ai_buy_garrison(i, l);
                    changed = true;
                }
                if keeps(self) {
                    self.ai_reshuffle(i, l);
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_dirty(uid);
        }
        // Its strengths are recounted with the defence it has here now (0x4a79c5).
        self.recount_bill(i);
        let m = &mut self.world.armies[i].mind;
        m.strength_bd = m.defence;
        m.countdown = 0;
        self.set_stored_building(i, l, 0);
    }
}

impl Game {
    // ------------------------------------------------------------------------------------
    // Spending
    // ------------------------------------------------------------------------------------

    /// The price army `i` pays at building `l`: the relation factor of the building's
    /// attitude to its faction (+3 when it owns it).
    fn ai_price(&self, i: usize, l: usize, base: i32) -> i32 {
        let a = &self.world.armies[i];
        let b = &self.world.locations[l];
        relation_price(base, b.attitude_to(a.faction), b.owner == Owner::Army(a.id))
    }

    /// Shopping (ai.md §9.3, 0x4a548c): in a building with goods well disposed to it — a
    /// market or church when its leader is not undead, an altar when it is — it sells its
    /// pack for half the price, then, if its spare gold covers the cheapest good, buys by
    /// tactical gain: every (unit, good) the unit can wear that raises its tactical cost is
    /// valued once at its gain; then the largest value above 5 whose price fits the spare
    /// gold is bought, again and again; after each buy the buyer's values are worked out
    /// again. Razdor fixes the original's bug: it did not recompute them, so a good bought
    /// for a unit that could no longer wear it (it bought one of that type just before) was
    /// paid for and lost.
    fn ai_shop(&mut self, i: usize, l: usize) {
        let c = self.content.clone();
        let (faction, leader_undead, bd) = {
            let a = &self.world.armies[i];
            (a.faction, a.troops.first().is_some_and(|t| c.unit(t.unit).nature == Nature::Undead), a.mind.defence)
        };
        let b = &self.world.locations[l];
        let place = (matches!(b.kind, LocationKind::Market | LocationKind::Church) && !leader_undead) || (b.kind == LocationKind::Altar && leader_undead);
        // A market is there while its timer is set, empty or not (0x4a548c).
        if !place || b.shop.is_none() || b.attitude_to(faction) < 0 {
            return;
        }
        let pack = std::mem::take(&mut self.world.armies[i].items);
        let mut kept = Vec::new();
        for item in pack {
            match c.try_item(item).map(|d| d.cost) {
                Some(p) if p > 0 => {
                    let v = self.ai_price(i, l, p.abs()) / 2;
                    self.world.armies[i].gold += v;
                }
                _ => kept.push(item),
            }
        }
        self.world.armies[i].items = kept;
        let goods = random_goods(&self.world.locations[l]);
        let spare = self.spare_gold(i);
        let cheapest = goods.iter().filter_map(|&(_, g)| c.try_item(g)).map(|d| self.ai_price(i, l, d.cost.abs())).min().unwrap_or(100_000).min(100_000);
        if spare <= 0 || cheapest > spare {
            return;
        }
        // value[k][u] and the price of good k.
        let n = self.world.armies[i].troops.len();
        let mut value = vec![vec![0i32; n]; goods.len()];
        let mut price = vec![0i32; goods.len()];
        // The gain of good `item` on troop `t`, if it can wear it.
        let gain = |t: &Troop, item: ItemId| {
            let before = tactical_now(&c, t, bd);
            let mut tried = *t;
            wear(&c, &mut tried, item)?;
            Some(if tactical_now(&c, &tried, bd) > before { item_gain(&c, &tried, bd) } else { 0 })
        };
        for u in 0..n {
            for (k, &(_, item)) in goods.iter().enumerate() {
                if let Some(v) = gain(&self.world.armies[i].troops[u], item) {
                    value[k][u] = v;
                    // `|RelationPrice(price)|` of the raw price: a good of negative price
                    // costs its absolute value (Round is symmetric).
                    price[k] = self.ai_price(i, l, c.item(item).cost.abs());
                }
            }
        }
        let mut bought = Vec::new();
        loop {
            let spare = self.spare_gold(i);
            let mut best: Option<(usize, usize, i32)> = None;
            for u in 0..n {
                for k in 0..goods.len() {
                    if price[k] != 0 && price[k] <= spare && value[k][u] > WORTH && value[k][u] > best.map_or(0, |b| b.2) {
                        best = Some((u, k, value[k][u]));
                    }
                }
            }
            let Some((u, k, _)) = best else { break };
            let item = goods[k].1;
            wear(&c, &mut self.world.armies[i].troops[u], item);
            self.world.armies[i].gold -= price[k];
            bought.push(goods[k].0);
            value[k].iter_mut().for_each(|v| *v = 0);
            price[k] = 0;
            self.ai_stats.bought += 1;
            let t = self.world.armies[i].troops[u];
            for (k, &(_, item)) in goods.iter().enumerate().filter(|&(k, _)| price[k] != 0) {
                value[k][u] = gain(&t, item).unwrap_or(0);
            }
        }
        bought.sort_unstable();
        if let Some(shop) = self.world.locations[l].shop.as_mut() {
            for k in bought.into_iter().rev() {
                shop.take(k);
            }
        }
    }

    /// Healing and resurrection (ai.md §9.4): with a heal bill, and a barracks unit undead
    /// exactly when its leader is, every wounded unit is healed fully for the relation price
    /// of `Round(Cost × HealingConst/100 × HP/maxHP)` — its current HP, so a badly hurt unit
    /// is cheap — when that is below its gold; in a town or church the dead are raised, the
    /// highest tactical cost first, for `Round(Cost × ResurectConst/100)`, those it cannot
    /// afford skipped. Each one keeps it busy for `HealingTime` from now. Returns whether
    /// anyone was healed or raised.
    fn ai_heal(&mut self, i: usize, l: usize, now: f64) -> bool {
        let c = self.content.clone();
        let o = &c.options;
        let mut done = false;
        let t = self.army_totals(i);
        if t.heal_bill > 0 {
            let leader_undead = self.world.armies[i].troops.first().is_some_and(|t| c.unit(t.unit).nature == Nature::Undead);
            let kind = self.world.locations[l].recruits.iter().any(|r| c.try_unit(r.unit).is_some_and(|d| (d.nature == Nature::Undead) == leader_undead));
            if kind {
                for k in 0..self.world.armies[i].troops.len() {
                    let tr = self.world.armies[i].troops[k];
                    let (hp, max) = troop_hp(&c, &tr);
                    if !tr.alive() || hp >= max {
                        continue;
                    }
                    let base = fpu_round(c.unit(tr.unit).cost as f64 / (100.0 / o.healing_const as f64) * hp as f64 / max as f64);
                    let p = self.ai_price(i, l, base);
                    let a = &mut self.world.armies[i];
                    if p < a.gold {
                        a.gold -= p;
                        a.troops[k].hurt = 0;
                        a.mind.busy_until = now + o.healing_time as f64;
                        done = true;
                    }
                }
            }
        }
        if matches!(self.world.locations[l].kind, LocationKind::Town | LocationKind::Church) && self.army_totals(i).res_bill > 0 {
            let bd = self.world.armies[i].mind.defence;
            let mut skipped = vec![false; self.world.armies[i].troops.len()];
            loop {
                let a = &self.world.armies[i];
                let mut pick: Option<(usize, i32)> = None;
                for (k, tr) in a.troops.iter().enumerate() {
                    let v = tactical_now(&c, tr, bd);
                    if !tr.alive() && !skipped[k] && v > pick.map_or(0, |p| p.1) {
                        pick = Some((k, v));
                    }
                }
                let Some((k, _)) = pick else { break };
                let base = fpu_round(c.unit(a.troops[k].unit).cost as f64 * (o.resurect_const as f64 / 100.0));
                let p = self.ai_price(i, l, base);
                let a = &mut self.world.armies[i];
                if p < a.gold {
                    a.gold -= p;
                    // Raised, it forgets its time of death (the original's bug kept it).
                    (a.troops[k].died_at, a.troops[k].kept_death) = (None, None);
                    a.troops[k].hurt = 0;
                    a.mind.busy_until = now + o.healing_time as f64;
                    done = true;
                } else {
                    skipped[k] = true;
                }
            }
        }
        done
    }

    /// `P` of the hire XP (experience.md §5): with "add experience like the player", the
    /// player's units' `Σ (level value + XP) div (units + 2)`, the dead included; else 0.
    /// The level value is the tactical cost's mode 0 (+0x1aa), which the Community hook
    /// never touches: a value of 0 stays 0 here.
    fn hire_base_xp(&self, i: usize) -> i32 {
        if !self.world.armies[i].ai.exp_like_player {
            return 0;
        }
        let c = &self.content;
        let sum: i64 = self.squad.iter().map(|u| super::experience::level_value(c, u.def, &u.base_stats(c)) + u.xp as i64).sum();
        (sum / (self.squad.len() as i64 + 2)) as i32
    }

    /// The XP a unit of `unit` hired by army `i` gets, given `P` (4a6b40): `X = (P − its
    /// level value (mode 0, no hook) div 2 when P ≥ 1, else 0) + the army's hire bonus`; when
    /// `X` > 0 it is fed `Rand(X) + X div 2`, level by level. Only drawn when `P` or the bonus
    /// is positive.
    fn hire_xp(&mut self, i: usize, unit: UnitId, p: i32) -> i32 {
        let bonus = self.world.armies[i].ai.hire_bonus_exp;
        if p <= 0 && bonus <= 0 {
            return 0;
        }
        let c = &self.content;
        let value = super::experience::level_value(c, unit, &super::units::Stats::of_level(c, unit, 1)) as i32;
        let x = if p < 1 { 0 } else { p - value / 2 } + bonus;
        if x <= 0 {
            return 0;
        }
        self.rng.random(x) + x / 2
    }

    /// The preference order of the roles by their tactical cost sums (warriors ×1, shooters
    /// and mages ×2): the lowest first, ties as the original orders them.
    fn role_order(c: &Content, troops: &[Troop], bd: i32) -> [u8; 3] {
        let (mut w, mut s, mut m) = (0i64, 0i64, 0i64);
        for t in troops {
            let v = tactical_now(c, t, bd) as i64;
            match attack_kind(c, t.unit) {
                4 => w += v,
                7 => s += 2 * v,
                0x11 => m += 2 * v,
                _ => {}
            }
        }
        if w < s && w < m {
            if s < m {
                [4, 7, 0x11]
            } else {
                [4, 0x11, 7]
            }
        } else if s < m {
            if w < m {
                [7, 4, 0x11]
            } else {
                [7, 0x11, 4]
            }
        } else if w < s {
            [0x11, 4, 7]
        } else {
            [0x11, 7, 4]
        }
    }

    /// Hiring (ai.md §9.5): while its spare gold is 1 or more and it has fewer units than the
    /// cap (12), the roles are ordered by their sums and the barracks slots scanned role by
    /// role for a unit in stock of that role, of its leader's Nature (a leader of unit 74 may
    /// hire any non-undead), whose price fits the spare gold; the first is hired. Entering
    /// the third role lowers the cap to 8, but the cap is only tested before each pass, and
    /// passing the last slot of the third role ends the visit's hiring. A unit is a recruit
    /// in its own building, a mercenary elsewhere; its XP comes level by level with a
    /// promotion try each. Then the army's items are handed out again. Returns whether it
    /// hired.
    fn ai_hire(&mut self, i: usize, l: usize) -> bool {
        let c = self.content.clone();
        let p = self.hire_base_xp(i);
        let now = self.clock.total_minutes() as u64;
        let mut cap = MAX_UNITS;
        let mut hired = false;
        while self.spare_gold(i) >= 1 && self.world.armies[i].troops.len() < cap {
            let a = &self.world.armies[i];
            let order = Game::role_order(&c, &a.troops, a.mind.defence);
            let leader = a.troops.first().map(|t| t.unit);
            let nature = leader.map_or(Nature::Normal, |u| c.unit(u).nature);
            let any_living = leader == Some(UnitId(74));
            let own = self.world.locations[l].owner == Owner::Army(a.id);
            let wanted = |u: UnitId, role: u8| c.try_unit(u).is_some_and(|d| attack_kind(&c, u) == role && (d.nature == nature || (any_living && d.nature != Nature::Undead)));
            let mut scan = Scan::default();
            loop {
                if let Some((k, r)) = barracks_slot(&self.world.locations[l], scan.slot) {
                    if r.stock.unwrap_or(0) > 0 && wanted(r.unit, order[scan.pref]) {
                        let price = self.ai_price(i, l, c.unit(r.unit).cost);
                        if price <= self.spare_gold(i) {
                            self.ai_add_hire(i, l, k, price, own, p, now);
                            hired = true;
                            scan.done = true;
                        }
                    }
                }
                scan.next(&mut cap);
                if scan.done || self.spare_gold(i) == 0 {
                    break;
                }
            }
        }
        let (troops, pack, bd) = {
            let a = &mut self.world.armies[i];
            (std::mem::take(&mut a.troops), std::mem::take(&mut a.items), a.mind.defence)
        };
        let (mut troops, mut pack) = (troops, pack);
        redistribute(&c, &mut troops, &mut pack, Vec::new(), bd);
        let a = &mut self.world.armies[i];
        a.troops = troops;
        a.items = pack;
        hired
    }

    /// Army `i` hires the unit of recruit `k` at building `l` for `price`.
    #[allow(clippy::too_many_arguments)]
    fn ai_add_hire(&mut self, i: usize, l: usize, k: usize, price: i32, own: bool, p: i32, now: u64) {
        let c = self.content.clone();
        let unit = take_stock(&mut self.world.locations[l], k);
        let slot = free_slot(&c, &self.world.armies[i].troops);
        let mut t = Troop::new(unit, 1, slot.unwrap_or(super::formation::Slot::new(super::formation::Row::Reserve, 0)));
        t.kind = if own { WageKind::Recruit } else { WageKind::Mercenary };
        t.last_paid = now;
        let xp = self.hire_xp(i, unit, p);
        let mut pool = Vec::new();
        if xp > 0 {
            ai_hire_gain(&c, &mut self.rng, &mut t, xp, &mut pool);
        }
        let a = &mut self.world.armies[i];
        a.gold -= price;
        a.troops.push(t);
        self.ai_stats.hired += 1;
    }

    /// Garrison buying (ai.md §9.6) in its own town, castle or fort with a garrison level and
    /// a positive stored score: while its spare gold is above a third of the gold it had when
    /// it started and the garrison has fewer units than the cap, it buys into the garrison as
    /// it hires (the garrison's role sums, its leader's Nature, no exception), recruits. The
    /// price is not compared with the gold: it is simply deducted. Returns whether it bought.
    fn ai_buy_garrison(&mut self, i: usize, l: usize) -> bool {
        let c = self.content.clone();
        let g0 = self.world.armies[i].gold;
        let p = self.hire_base_xp(i);
        let now = self.clock.total_minutes() as u64;
        let mut cap = MAX_UNITS;
        let mut bought = false;
        while self.spare_gold(i) > g0 / 3 && self.world.locations[l].garrison.len() < cap {
            let loc = &self.world.locations[l];
            let order = Game::role_order(&c, &loc.garrison, loc.garrison_defence);
            let nature = self.world.armies[i].troops.first().map_or(Nature::Normal, |t| c.unit(t.unit).nature);
            let wanted = |u: UnitId, role: u8| c.try_unit(u).is_some_and(|d| attack_kind(&c, u) == role && d.nature == nature);
            let mut scan = Scan::default();
            loop {
                if let Some((k, r)) = barracks_slot(&self.world.locations[l], scan.slot) {
                    if r.stock.unwrap_or(0) > 0 && wanted(r.unit, order[scan.pref]) && self.spare_gold(i) > g0 / 3 {
                        let price = self.ai_price(i, l, c.unit(r.unit).cost);
                        take_stock(&mut self.world.locations[l], k);
                        self.world.armies[i].gold -= price;
                        let slot = free_slot(&c, &self.world.locations[l].garrison);
                        let mut t = Troop::new(r.unit, 1, slot.unwrap_or(super::formation::Slot::new(super::formation::Row::Reserve, 0)));
                        t.last_paid = now;
                        let xp = self.hire_xp(i, r.unit, p);
                        let mut pool = Vec::new();
                        if xp > 0 {
                            ai_hire_gain(&c, &mut self.rng, &mut t, xp, &mut pool);
                        }
                        self.world.locations[l].garrison.push(t);
                        // The garrison is recounted with its new unit (0x4a704e).
                        self.world.locations[l].strengths_bare = false;
                        self.ai_stats.hired += 1;
                        bought = true;
                        scan.done = true;
                    }
                }
                scan.next(&mut cap);
                if scan.done || self.spare_gold(i) < g0 / 3 {
                    break;
                }
            }
        }
        bought
    }

    /// The garrison reshuffle (ai.md §9.6): every living unit but the leader, of the army and
    /// the garrison, is dealt again between them by a quota table on the garrison level `L`
    /// and the building's defence `D` (`q = L / (D/25 + 1)`); units with the Garrison bonus go
    /// to the garrison first; then up to 201 rounds give the cell of the smallest value (army
    /// before garrison, mages before shooters before warriors, the last of equals) the
    /// unassigned unit of that role with the highest `Round(cost + √XP)`. The dead are dropped
    /// and the units left after the last round lost.
    fn ai_reshuffle(&mut self, i: usize, l: usize) {
        let c = self.content.clone();
        let (army_bd, d, level, income, fort) = {
            let a = &self.world.armies[i];
            let b = &self.world.locations[l];
            (a.mind.defence, b.garrison_defence, a.ai.garrison_level, b.gold_income, b.kind == LocationKind::Fort)
        };
        // The pool: the army's units after the leader, then the garrison's, living ones.
        let mut pool: Vec<(Troop, i32)> = Vec::new();
        let leader = {
            let a = &mut self.world.armies[i];
            let mut rest = std::mem::take(&mut a.troops);
            let leader = if rest.is_empty() { None } else { Some(rest.remove(0)) };
            pool.extend(rest.into_iter().filter(|t| t.alive()).map(|t| (t, army_bd)));
            leader
        };
        let garrison = std::mem::take(&mut self.world.locations[l].garrison);
        pool.extend(garrison.into_iter().filter(|t| t.alive()).map(|t| (t, d)));
        let cost: Vec<i32> = pool.iter().map(|(t, bd)| tactical_now(&c, t, *bd)).collect();
        let kind: Vec<u8> = pool.iter().map(|(t, _)| attack_kind(&c, t.unit)).collect();
        let role = |k: u8| match k {
            4 => Some(0),
            7 => Some(1),
            0x11 => Some(2),
            _ => None,
        };
        let mut quota = [[0i64; 3]; 2];
        for (k, &v) in cost.iter().enumerate() {
            if let Some(r) = role(kind[k]) {
                quota[1][r] += v as i64;
            }
        }
        let factor = d as f64 / 25.0 + 1.0;
        let q = level as f64 / factor;
        for r in 0..3 {
            if q >= 50.0 {
                quota[0][r] = delphi_round((q - 50.0) * 2.0 * quota[1][r] as f64 / 100.0);
                if r == 0 {
                    quota[1][0] = 0;
                }
            } else {
                quota[1][r] = delphi_round((100.0 - 2.0 * q) * quota[1][r] as f64 / 100.0);
            }
        }
        for v in quota[1].iter_mut() {
            if fort {
                *v += 150 - 2 * income as i64;
            }
            *v = (*v - 2 * income as i64).max(0);
        }
        let mut assigned = vec![false; pool.len()];
        let mut left = pool.len();
        let mut army: Vec<Troop> = leader.into_iter().collect();
        let mut held: Vec<Troop> = Vec::new();
        for k in (0..pool.len()).rev() {
            if c.unit(pool[k].0.unit).bonus == Some(Bonus::Garrison) {
                held.push(pool[k].0);
                assigned[k] = true;
                left -= 1;
                quota[1][0] += delphi_round(factor * cost[k] as f64);
            }
        }
        let mut rounds = 0;
        while left > 0 && rounds < RESHUFFLE_ROUNDS {
            rounds += 1;
            let (mut br, mut bc) = (0usize, 0usize);
            let mut want = 4u8;
            for r in 0..2 {
                for cc in (0..3).rev() {
                    if quota[r][cc] <= quota[br][bc] {
                        br = r;
                        bc = cc;
                        want = [4, 7, 0x11][cc];
                    }
                }
            }
            let mut pick: Option<(usize, i64)> = None;
            for k in 0..pool.len() {
                if assigned[k] || kind[k] != want {
                    continue;
                }
                let v = cost[k] as f64 + (pool[k].0.xp as f64).sqrt();
                if (pick.map_or(0, |p| p.1) as f64) < v {
                    pick = Some((k, delphi_round(v)));
                }
            }
            let Some((k, _)) = pick else {
                quota[br][bc] += FULL as i64;
                continue;
            };
            if br == 0 {
                if army.len() < MAX_UNITS {
                    army.push(pool[k].0);
                    assigned[k] = true;
                    left -= 1;
                    quota[0][bc] += cost[k] as i64;
                    self.world.armies[i].troops = army.clone();
                    if self.spare_gold(i) == 0 {
                        for v in quota[0].iter_mut() {
                            *v += FULL as i64;
                        }
                    }
                } else {
                    quota[0][bc] += FULL as i64;
                }
            } else if held.len() < MAX_UNITS {
                held.push(pool[k].0);
                assigned[k] = true;
                left -= 1;
                quota[1][bc] += delphi_round(factor * cost[k] as f64);
            } else {
                quota[1][bc] += FULL as i64;
            }
        }
        // Both are re-gridded: the leader keeps his cell, the others take the first free
        // ones from the reserve forward.
        let mut taken: Vec<super::formation::Slot> = army.first().map(|t| t.slot).into_iter().collect();
        for t in army.iter_mut().skip(1) {
            if let Some(s) = c.formation.new_unit_slot(&taken) {
                t.slot = s;
                taken.push(s);
            }
        }
        self.world.armies[i].troops = army;
        let mut slots: Vec<super::formation::Slot> = Vec::new();
        for t in held.iter_mut() {
            if let Some(s) = c.formation.new_unit_slot(&slots) {
                t.slot = s;
                slots.push(s);
            }
        }
        self.world.locations[l].garrison = held;
        // Recounted, then passed through the first side (0x4a7972).
        self.world.locations[l].strengths_bare = false;
        // The army, then the garrison, are passed through the first static side and back
        // (0x4a7923, 0x4a7989: 49855c, 4988c0), each arranged anew in the building.
        let defence = self.world.locations[l].garrison_defence;
        super::game::arrange_troops(&c, &mut self.world.armies[i].troops, defence);
        super::game::arrange_troops(&c, &mut self.world.locations[l].garrison, defence);
        let army_hp: Vec<i32> = self.world.armies[i].troops.iter().map(|t| troop_unit(&c, t).hp).collect();
        let held_hp: Vec<i32> = self.world.locations[l].garrison.iter().map(|t| troop_unit(&c, t).hp).collect();
        let mut sims = self.sims.borrow_mut();
        sims.pass_through(army_hp);
        sims.pass_through(held_hp);
    }
}

/// The scan of the barracks by role (0x4a548c's hire loops): slots 1–6 of the first role,
/// then of the second, then of the third. Entering the third sets the cap to 8; passing its
/// last slot ends the scan and sets the cap to 0, even right after a unit was taken there.
#[derive(Default)]
struct Scan {
    pref: usize,
    slot: usize,
    done: bool,
}

impl Scan {
    fn next(&mut self, cap: &mut usize) {
        self.slot += 1;
        if self.slot == 6 {
            self.pref += 1;
            self.slot = 0;
            if self.pref == 2 {
                *cap = LATE_CAP;
            }
        }
        if self.pref == 3 {
            self.done = true;
            *cap = 0;
        }
    }
}

/// The recruit in barracks slot `slot` (0–5) of `l`, with its index: an empty slot has
/// none, but the scan still passes it (0x4a548c reads the six slots).
fn barracks_slot(l: &Location, slot: usize) -> Option<(usize, super::world::Recruit)> {
    l.recruits.iter().position(|r| r.slot as usize == slot).map(|k| (k, l.recruits[k]))
}

/// The stock of recruit `k` at `l` goes down by one. Returns its unit.
fn take_stock(l: &mut Location, k: usize) -> UnitId {
    let r = &mut l.recruits[k];
    if let Some(n) = r.stock.as_mut() {
        *n = (*n - 1).max(0);
    }
    r.unit
}

/// Free formation cell for a new unit next to `troops`: the first from the reserve forward,
/// whatever the unit (495ce0).
fn free_slot(c: &Content, troops: &[Troop]) -> Option<super::formation::Slot> {
    let taken: Vec<_> = troops.iter().map(|t| t.slot).collect();
    c.formation.new_unit_slot(&taken)
}

impl Game {
    // ------------------------------------------------------------------------------------
    // AI against AI
    // ------------------------------------------------------------------------------------

    /// Army `att` fights `def` off screen (0x4a4c68, ai.md §10), the battle engine playing
    /// both sides (only its paid units fight). HP and deaths are written back; then:
    /// - the attacker wiped out: it is beaten; a feudal or rogue defender takes its wage bill
    ///   (if it is feudal) and its gold, all of it below `MinVictoryGold`, else
    ///   `gold div VictoryGoldDiv`; its worn items go to the pool;
    /// - else its fighters gain XP (with promotion tries), a dead leader gets 1 HP, its dead
    ///   units' items go to the pool;
    /// - the defender wiped out: an army is beaten, a garrison emptied; a feudal or rogue
    ///   attacker takes a town's, castle's or fort's income, garrison gold and stock, or an
    ///   army's wage bill (when it is feudal or rogue) and `gold div VictoryGoldDiv` (no
    ///   `MinVictoryGold` here), or another garrison's `gold div VictoryGoldDiv`;
    /// - else the same XP and items for the defender.
    ///
    /// "Wiped out" is a strength of 0 at the end (side +0x7e8), not an empty side. The pool
    /// goes to the attacker when its end strength is strictly greater, else to the defender.
    /// Returns true when the defender was wiped out (or had nobody).
    pub fn ai_battle(&mut self, att: usize, def: Defender) -> bool {
        let c = self.content.clone();
        let o = c.options.clone();
        let now = self.clock.total_minutes();
        let att_uid = self.world.armies[att].uid;
        let def_uid = match def {
            Defender::Army(j) => Some(self.world.armies[j].uid),
            Defender::Garrison(_) => None,
        };
        // An empty record is beaten at once.
        let empty = match def {
            Defender::Army(j) => self.world.armies[j].troops.is_empty(),
            Defender::Garrison(l) => self.world.locations[l].garrison.is_empty() && self.world.locations[l].stationed.is_empty(),
        };
        if empty {
            return true;
        }
        // The loot reads each record's wage bill (+0x16e0) of its last recount.
        let att_bill = self.world.armies[att].mind.wage_bill;
        let def_wages = match def {
            Defender::Army(j) => self.world.armies[j].mind.wage_bill,
            Defender::Garrison(_) => 0,
        };
        let (side_a, fought_a) = self.army_side(att, true);
        let (side_b, fought_b) = match def {
            Defender::Army(j) => self.army_side(j, false),
            Defender::Garrison(l) => {
                let loc = &self.world.locations[l];
                let fought: Vec<usize> = (0..loc.garrison.len()).filter(|&k| loc.garrison[k].alive()).collect();
                (self.garrison_side(l), fought)
            }
        };
        // The player's units left in a garrison that fight, after its own troops.
        let fought_s: Vec<usize> = match def {
            Defender::Garrison(l) => (0..self.world.locations[l].stationed.len()).filter(|&k| self.world.locations[l].stationed[k].unit.alive()).collect(),
            Defender::Army(_) => Vec::new(),
        };
        // The battle is played from the static sides and leaves them changed (0x4a0710).
        let bt = {
            let mut sims = self.sims.borrow_mut();
            sims.stage(&side_a, &side_b);
            let bt = fight(&c, &side_a, &side_b, true, sims.records);
            sims.records = bt.side_records();
            bt
        };
        let na = side_a.units.len();
        drop((side_a, side_b));
        self.ai_stats.battles += 1;
        self.battles += 1;
        // Each fighter's share of its side's pool (experience.md §3); the gain takes
        // `AIExpiriencePercent` of it.
        let award = |team: Team, k: usize| -> i32 { bt.xp_awards(team).iter().find(|a| a.fighter == k).map_or(0, |a| a.xp) };
        // Each side's strength at the end (48bb10, experience.md §3): 0 beats it, even with a
        // lone shooter or mage still standing whose fifth rounds to 0, and a side that
        // surrendered is 0; the stronger takes the loot pool.
        let a_left = bt.strength_now(Team::Player);
        let b_left = bt.strength_now(Team::Enemy);
        let stamp = now as u64;
        // Write back: HP and deaths.
        {
            let a = &mut self.world.armies[att];
            for (n, &k) in fought_a.iter().enumerate() {
                write_hp(&c, &mut a.troops[k], bt.fighters[n].hp, stamp);
            }
        }
        match def {
            Defender::Army(j) => {
                let b = &mut self.world.armies[j];
                for (n, &k) in fought_b.iter().enumerate() {
                    write_hp(&c, &mut b.troops[k], bt.fighters[na + n].hp, stamp);
                }
            }
            Defender::Garrison(l) => {
                let loc = &mut self.world.locations[l];
                for (n, &k) in fought_b.iter().enumerate() {
                    write_hp(&c, &mut loc.garrison[k], bt.fighters[na + n].hp, stamp);
                }
                let mut f = na + fought_b.len();
                for s in loc.stationed.iter_mut().filter(|s| s.unit.alive()) {
                    if let Some(x) = bt.fighters.get(f) {
                        s.unit.hp = x.hp.max(0);
                        if s.unit.hp == 0 {
                            s.unit.died_at = Some(stamp);
                        }
                    }
                    f += 1;
                }
            }
        }
        let mut pool: Vec<ItemId> = Vec::new();
        self.mark_dirty(att_uid);
        if let Some(u) = def_uid {
            self.mark_dirty(u);
        }
        let def_lordly = match def {
            Defender::Army(j) => self.world.armies[j].ai.style.lordly(),
            // A garrison's record has style 0.
            Defender::Garrison(_) => true,
        };
        let a_beaten = a_left == 0;
        if a_beaten {
            let (style, gold) = (self.world.armies[att].ai.style, self.world.armies[att].gold);
            if def_lordly {
                let mut take = if style == Style::Feudal { att_bill } else { 0 };
                let g = if gold < o.min_victory_gold { gold } else { gold / o.victory_gold_div.max(1) };
                take += g;
                self.world.armies[att].gold -= g;
                match def {
                    Defender::Army(j) => self.world.armies[j].gold += take,
                    Defender::Garrison(l) => self.world.locations[l].treasure_gold += take,
                }
            }
            for t in self.world.armies[att].troops.iter_mut() {
                pool.extend(t.worn.iter_mut().filter_map(Option::take));
            }
        } else {
            let pct = o.ai_experience_percent;
            // Only the survivors: the original walks the side's list, from which the dead
            // were taken during the battle, so a corpse draws no promotion roll.
            for (n, &k) in fought_a.iter().enumerate().filter(|&(n, _)| bt.fighters[n].alive()) {
                let xp = award(Team::Player, n);
                let t = &mut self.world.armies[att].troops[k];
                ai_gain_xp(&c, &mut self.rng, t, xp, pct, &mut pool);
            }
            let a = &mut self.world.armies[att];
            revive_leader(&c, &mut a.troops);
            for t in a.troops.iter_mut().filter(|t| !t.alive()) {
                pool.extend(t.worn.iter_mut().filter_map(Option::take));
            }
        }
        let b_beaten = b_left == 0;
        let mut won = true;
        if b_beaten {
            if self.world.armies[att].ai.style.lordly() {
                match def {
                    Defender::Garrison(l) if matches!(self.world.locations[l].kind, LocationKind::Town | LocationKind::Castle | LocationKind::Fort) => {
                        let loc = &mut self.world.locations[l];
                        let gold = loc.gold_income + loc.treasure_gold + std::mem::take(&mut loc.tribute_gold);
                        self.world.armies[att].gold += gold;
                    }
                    Defender::Garrison(l) => {
                        let loc = &mut self.world.locations[l];
                        let g = loc.treasure_gold / o.victory_gold_div.max(1);
                        loc.treasure_gold -= g;
                        self.world.armies[att].gold += g;
                    }
                    Defender::Army(j) => {
                        let b = &self.world.armies[j];
                        let wages = if b.ai.style.lordly() { def_wages } else { 0 };
                        let g = b.gold / o.victory_gold_div.max(1);
                        self.world.armies[j].gold -= g;
                        self.world.armies[att].gold += wages + g;
                    }
                }
            }
            match def {
                Defender::Army(j) => {
                    for t in self.world.armies[j].troops.iter_mut() {
                        pool.extend(t.worn.iter_mut().filter_map(Option::take));
                    }
                }
                Defender::Garrison(l) => {
                    let loc = &mut self.world.locations[l];
                    for t in loc.garrison.iter_mut() {
                        pool.extend(t.worn.iter_mut().filter_map(Option::take));
                    }
                    for s in loc.stationed.iter_mut() {
                        pool.extend(s.unit.items.iter_mut().filter_map(Option::take));
                    }
                    loc.garrison.clear();
                    loc.stationed.clear();
                }
            }
        } else {
            won = false;
            let pct = o.ai_experience_percent;
            match def {
                Defender::Army(j) => {
                    for (n, &k) in fought_b.iter().enumerate().filter(|&(n, _)| bt.fighters[na + n].alive()) {
                        let xp = award(Team::Enemy, na + n);
                        ai_gain_xp(&c, &mut self.rng, &mut self.world.armies[j].troops[k], xp, pct, &mut pool);
                    }
                    let b = &mut self.world.armies[j];
                    revive_leader(&c, &mut b.troops);
                    for t in b.troops.iter_mut().filter(|t| !t.alive()) {
                        pool.extend(t.worn.iter_mut().filter_map(Option::take));
                    }
                }
                Defender::Garrison(l) => {
                    for (n, &k) in fought_b.iter().enumerate().filter(|&(n, _)| bt.fighters[na + n].alive()) {
                        let xp = award(Team::Enemy, na + n);
                        ai_gain_xp(&c, &mut self.rng, &mut self.world.locations[l].garrison[k], xp, pct, &mut pool);
                    }
                    // The player's units left here are in the same garrison record in the
                    // original: its survivors gain, and roll for promotion, by the AI's rule.
                    let first = na + fought_b.len();
                    let loc = &mut self.world.locations[l];
                    for (m, &k) in fought_s.iter().enumerate().filter(|&(m, _)| bt.fighters.get(first + m).is_some_and(|f| f.alive())) {
                        stationed_gain_xp(&c, &mut self.rng, &mut loc.stationed[k].unit, award(Team::Enemy, first + m), pct, &mut pool);
                    }
                    for t in loc.garrison.iter_mut().filter(|t| !t.alive()) {
                        pool.extend(t.worn.iter_mut().filter_map(Option::take));
                    }
                    for s in loc.stationed.iter_mut().filter(|s| !s.unit.alive()) {
                        pool.extend(s.unit.items.iter_mut().filter_map(Option::take));
                    }
                    loc.stationed.retain(|s| s.unit.alive());
                }
            }
        }
        if !pool.is_empty() {
            if b_left < a_left {
                let a = &mut self.world.armies[att];
                let bd = a.mind.defence;
                redistribute(&c, &mut a.troops, &mut a.items, pool, bd);
            } else {
                match def {
                    Defender::Army(j) => {
                        let b = &mut self.world.armies[j];
                        let bd = b.mind.defence;
                        redistribute(&c, &mut b.troops, &mut b.items, pool, bd);
                    }
                    Defender::Garrison(l) => {
                        let loc = &mut self.world.locations[l];
                        let bd = loc.garrison_defence;
                        redistribute(&c, &mut loc.garrison, &mut loc.treasure, pool, bd);
                    }
                }
            }
        }
        // Both records are recounted (0x4a4c68 → 0x4a16d4), with the defence each has now.
        if let Defender::Garrison(l) = def {
            self.world.locations[l].strengths_bare = false;
        }
        for i in [Some(att), match def {
            Defender::Army(j) => Some(j),
            Defender::Garrison(_) => None,
        }]
        .into_iter()
        .flatten()
        {
            self.recount_bill(i);
            let m = &mut self.world.armies[i].mind;
            m.strength_bd = m.defence;
        }
        // Reports, and the beaten leave the map.
        let a_name = army_name(&self.world.armies[att]);
        let tile = self.world.armies[att].tile(&self.world.map);
        match def {
            Defender::Army(j) => {
                let b_name = army_name(&self.world.armies[j]);
                if b_beaten {
                    self.report(crate::trf!("{winner} defeated {loser}.", winner = a_name, loser = b_name), tile, false);
                } else if a_beaten {
                    self.report(crate::trf!("{winner} defeated {loser}.", winner = b_name, loser = a_name), tile, false);
                }
            }
            Defender::Garrison(l) if a_beaten => {
                let loc = &self.world.locations[l];
                let text = crate::trf!("{name} fell at the walls of {place}.", name = a_name, place = building_name(loc));
                let (t, mine) = (loc.tile, loc.owned());
                self.report(text, t, mine);
            }
            _ => {}
        }
        if a_beaten {
            // Off the map; the rest of its arrival still runs with its record, and it leaves
            // at the end of it ([`Game::ai_arrive`]).
            if let Some(i) = self.army_by_uid(att_uid) {
                let m = &mut self.world.armies[i].mind;
                m.fallen = true;
                m.standing = None;
            }
        }
        if b_beaten {
            if let Some(i) = def_uid.and_then(|u| self.army_by_uid(u)) {
                self.army_beaten(i, Beaten::ByAi);
            }
        }
        if !a_beaten {
            if let Some(i) = self.army_by_uid(att_uid) {
                self.rescore_buildings(i);
            }
        }
        if !b_beaten {
            if let Some(i) = def_uid.and_then(|u| self.army_by_uid(u)) {
                self.rescore_buildings(i);
            }
        }
        if let Some(i) = self.army_by_uid(att_uid) {
            self.world.armies[i].mind.countdown = 0;
        }
        won
    }

    /// Adds a battle report to the log; the player hears of it (an event) when it happened
    /// within his sight, or concerned his own building.
    fn report(&mut self, text: String, tile: Tile, concerns_player: bool) {
        let news = AiNews { text, tile, at: self.clock.total_minutes() as u64 };
        let seen = fog::within(self.tile(), tile, self.sight_radius());
        if seen || concerns_player {
            self.ai_events.push(Event::Battle(news.clone()));
            self.ai_log.push(news);
            if self.ai_log.len() > LOG_KEPT {
                self.ai_log.remove(0);
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // Beaten armies and respawn
    // ------------------------------------------------------------------------------------

    /// Army `i` lost a battle (or a spell destroyed it): it leaves the map and is recorded as
    /// beaten (ai.md §12). Beaten by the player, its record keeps only its leader unless it
    /// respawns whole (byte 83); beaten by the AI it keeps every unit, dead, so the whole army
    /// comes back. With a home and a respawn delay it waits for its respawn; without, it
    /// waits for good ([`NEVER`]), where an event's activation can still bring it back.
    pub(crate) fn army_beaten(&mut self, i: usize, by: Beaten) {
        let now = self.clock.total_minutes();
        let mut a = self.remove_army(i);
        if a.id != 0 {
            // One "beaten by" mark, the last winner's (0x496834 overwrites it).
            match by {
                Beaten::ByPlayer => {
                    self.beaten_armies.insert(a.id);
                    self.ai_beaten.remove(&a.id);
                }
                Beaten::ByAi => {
                    self.ai_beaten.insert(a.id);
                    self.beaten_armies.remove(&a.id);
                }
            }
        }
        if !a.ai.enabled {
            return;
        }
        a.path.clear();
        a.chasing = false;
        for t in &mut a.troops {
            t.spells = Default::default();
        }
        a.mind.standing = None;
        a.mind.contact = None;
        if by == Beaten::ByPlayer && !a.ai.respawn_all {
            a.troops.truncate(1);
        }
        if !a.troops.is_empty() {
            let due = if a.home.is_some() && a.ai.respawn_days > 0 { now + (a.ai.respawn_days as u64 * MINUTES_PER_DAY) as f64 } else { NEVER };
            self.world.respawns.push(Respawn { due, army: a });
        }
    }

    /// Beaten armies whose delay has passed (strictly) come back (0x4a28d0, ai.md §12):
    /// a feudal army at its home if it still owns it, else the first town, castle or fort it
    /// owns (owning none, never); a rogue or peasant at its home, taking it over when it is a
    /// village, shipyard, altar or ruins (from anyone, the player included). It stands at the
    /// building's centre with every unit of its record alive, unhurt and paid, its first
    /// wander point at its post, "just respawned", and the delay's days of income in gold.
    fn ai_respawns(&mut self, now: f64) {
        let mut k = 0;
        while k < self.world.respawns.len() {
            if self.world.respawns[k].due >= now {
                k += 1;
                continue;
            }
            let Respawn { mut army, .. } = self.world.respawns.remove(k);
            let Some(home) = army.home else { continue };
            let own = |w: &World, l: usize| w.locations[l].owner == Owner::Army(army.id);
            let at = if army.ai.style == Style::Feudal && !own(&self.world, home) {
                let pick = [LocationKind::Town, LocationKind::Castle, LocationKind::Fort].iter().find_map(|&kind| (0..self.world.locations.len()).find(|&l| own(&self.world, l) && self.world.locations[l].kind == kind));
                match pick {
                    Some(l) => l,
                    None => {
                        // Its delay set to 0: it never respawns by itself again.
                        army.ai.respawn_days = 0;
                        self.world.respawns.insert(k, Respawn { due: NEVER, army });
                        k += 1;
                        continue;
                    }
                }
            } else {
                if army.ai.style != Style::Feudal && matches!(self.world.locations[home].kind, LocationKind::Village | LocationKind::Shipyard | LocationKind::Altar | LocationKind::Ruins) {
                    let loc = &mut self.world.locations[home];
                    loc.take_sides(army.faction, army.ai.relations);
                    loc.owner = Owner::Army(army.id);
                }
                home
            };
            let stamp = now as u64;
            for t in army.troops.iter_mut() {
                t.hurt = 0;
                t.died_at = None;
                t.kept_death = None;
                t.unpaid = false;
                t.last_paid = stamp;
            }
            let m = &mut army.mind;
            m.wander = [(0, 0); WANDER_POINTS];
            m.wander[0] = army.post;
            m.just_respawned = true;
            // Recounted with the defence its record kept (0x4a28d0).
            m.strength_bd = m.defence;
            m.wage_bill = army_wages(&self.content, &army.troops);
            m.walked = 0;
            m.no_path = true;
            m.free_step = true;
            army.path.clear();
            army.pos = self.world.map.center(self.world.locations[at].tile);
            army.gold += army.ai.respawn_days as i32 * army.ai.extra_income;
            // Back on the map: no longer destroyed, its "beaten by" mark cleared (0x4a28d0),
            // so the events' "beaten" conditions no longer hold for it.
            self.beaten_armies.remove(&army.id);
            self.ai_beaten.remove(&army.id);
            // Rearranged through the first static side and back (0x4a28d0: 49855c, 4988c0).
            super::game::arrange_troops(&self.content, &mut army.troops, m.defence);
            let hps: Vec<i32> = army.troops.iter().map(|t| troop_unit(&self.content, t).hp).collect();
            self.sims.borrow_mut().pass_through(hps);
            let uid = army.uid;
            self.insert_army(army);
            self.mark_dirty(uid);
            self.ai_stats.respawns += 1;
        }
    }

    // ------------------------------------------------------------------------------------
    // Noon and midnight
    // ------------------------------------------------------------------------------------

    /// Army `i`'s noon (0x4a41d8, ai.md §14), run at its first arrival after 12:00: the next
    /// is tomorrow's; it gains its base income, the gold stock of its castles and forts and
    /// of the villages linked to its buildings (all emptied); today's income keeps the
    /// castles' and forts' income instead of their stock. A feudal army then pays its wages
    /// (economy.md §1); the others mark every unit paid.
    fn ai_noon(&mut self, i: usize, now: f64) {
        let day = MINUTES_PER_DAY as f64;
        let id = self.world.armies[i].id;
        let mut income = self.world.armies[i].ai.extra_income;
        let mut delta = 0;
        for l in 0..self.world.locations.len() {
            let owner = self.world.locations[l].owner;
            let loc = &self.world.locations[l];
            if owner == Owner::Army(id) && loc.kind.capturable() {
                let x = loc.tribute_gold;
                income += x;
                delta += x - loc.gold_income;
                self.world.locations[l].tribute_gold = 0;
            }
            let loc = &self.world.locations[l];
            if loc.kind == LocationKind::Village && loc.linked.is_some_and(|k| self.world.locations[k].owner == Owner::Army(id)) {
                income += loc.tribute_gold;
                self.world.locations[l].tribute_gold = 0;
            }
        }
        let stamp = now as u64;
        let a = &mut self.world.armies[i];
        a.mind.next_noon = ((now / day).floor() + 1.0) * day + day / 2.0;
        a.gold += income;
        a.mind.income = income - delta;
        if a.ai.style == Style::Feudal {
            self.ai_pay_wages(i, stamp);
            // The wage payment ends by marking its pairs to be rescored (0x4a26e8 at the end
            // of 0x4a41d8's wage paths); the others' noon does not.
            let uid = self.world.armies[i].uid;
            self.mark_dirty(uid);
        } else {
            for t in self.world.armies[i].troops.iter_mut() {
                t.unpaid = false;
                t.last_paid = stamp;
            }
        }
    }

    /// A feudal army pays its gold wage bill (economy.md §1), cut by its own Rear Service,
    /// and its elementals' mana bill out of the *player's* mana (raising the mana-short flag
    /// when he has none). With the gold not below 0 everyone is paid (with the flag up the
    /// elementals go unpaid, and the flag is cleared); else the paid units of kind 1 or 2,
    /// not Elementals, with the lowest full wage (the earliest of equals, corpses included)
    /// are refunded and go unpaid until it is not, the gold is set to 0, and every unit last
    /// paid more than `MaxTimeNotUpkeep` ago leaves. Razdor fixes two of the original's bugs
    /// here: its Rear Service read the *player's* flag and stored income for an AI army, and
    /// with the flag up an enough-gold noon left the others' old marks and the flag itself.
    fn ai_pay_wages(&mut self, i: usize, now: u64) {
        let c = self.content.clone();
        let mut bill = self.army_totals(i).wages;
        if super::economy::army_has(&c, &self.world.armies[i], &Bonus::AddPayment) {
            bill = rear_service(bill, self.world.armies[i].mind.income);
        }
        // The mana bill is one global in the original, rebuilt by every army's totals; this
        // army's own (dormant: no shipped unit is an Elemental).
        let mana_bill: i32 = self.world.armies[i]
            .troops
            .iter()
            .filter(|t| t.alive() && t.kind == WageKind::Recruit && c.paid_in_mana(t.unit))
            .map(|t| c.wage_for(t.unit, t.kind))
            .sum();
        self.pay_mana_bill(mana_bill);
        let mut flag = self.mana_short;
        let a = &mut self.world.armies[i];
        a.gold -= bill;
        if a.gold >= 0 {
            for t in a.troops.iter_mut() {
                t.last_paid = now;
                t.unpaid = flag && c.paid_in_mana(t.unit);
            }
            self.mana_short = false;
            return;
        }
        for t in a.troops.iter_mut() {
            t.unpaid = false;
        }
        while a.gold < 0 {
            let pick = (0..a.troops.len())
                .filter(|&k| {
                    let t = &a.troops[k];
                    !t.unpaid && t.kind.is_paid() && !c.paid_in_mana(t.unit)
                })
                .min_by_key(|&k| (c.wage_for(a.troops[k].unit, a.troops[k].kind), k));
            if flag {
                for t in a.troops.iter_mut().take(11).filter(|t| c.paid_in_mana(t.unit)) {
                    t.unpaid = true;
                }
                flag = false;
            }
            let Some(k) = pick else {
                a.gold += FULL;
                break;
            };
            a.troops[k].unpaid = true;
            a.gold += c.wage_for(a.troops[k].unit, a.troops[k].kind);
        }
        for t in a.troops.iter_mut().filter(|t| !t.unpaid) {
            t.last_paid = now;
        }
        a.gold = 0;
        let limit = c.options.max_time_not_upkeep.max(0) as u64;
        a.troops.retain(|t| t.last_paid + limit >= now);
        self.mana_short = flag;
    }

    /// The AI's midnight (0x4a1998, ai.md §14): every army's village average becomes
    /// `(average + today's) div 2` and every AI army rescores every building. (The Medic's
    /// 10% runs with the economy's midnight.)
    pub(crate) fn ai_midnight(&mut self) {
        for i in 0..self.world.armies.len() {
            if !managed(&self.world.armies[i]) {
                continue;
            }
            let m = &mut self.world.armies[i].mind;
            m.village_avg = (m.village_avg + m.village_today) / 2;
            m.village_today = 0;
            self.rescore_buildings(i);
        }
    }
}

/// Writes a fighter's end HP `hp` into troop `t`: dead (the time of death now, unless it was
/// already dead), or the HP it lacks against its maximum. Razdor fixes the original's bug
/// (0x4a4c68 sets the time only when it is 0, and a raised unit kept its first one): a unit
/// raised again counts from its latest death.
pub(crate) fn write_hp(c: &Content, t: &mut Troop, hp: i32, now: u64) {
    if hp <= 0 {
        if t.died_at.is_none() {
            (t.died_at, t.kept_death) = (Some(now), None);
        }
        return;
    }
    let max = troop_max_hp(c, t);
    t.hurt = (max - hp).max(0);
}

/// A side that survived keeps its leader (unit 1) with 1 HP; its time of death is forgotten.
fn revive_leader(c: &Content, troops: &mut [Troop]) {
    if let Some(t) = troops.first_mut().filter(|t| !t.alive()) {
        (t.died_at, t.kept_death) = (None, None);
        t.hurt = troop_max_hp(c, t) - 1;
    }
}

/// A troop banks `xp` by the original's gain rule ([`super::experience::add_xp`]) and rises
/// the levels it pays for, keeping the rest; a wounded troop's HP then follows its new
/// maximum, as the stat rebuild after a gain does ([`follow_rebuild`]). Returns the levels
/// gained.
pub fn troop_gain_xp(c: &Content, t: &mut Troop, xp: i32) -> i32 {
    let before = troop_hp(c, t);
    let (level, left, gained) = super::experience::add_xp(t.level, t.xp, xp, |l| c.xp_to_next(t.unit, l));
    t.level = level;
    t.xp = left;
    if gained > 0 {
        follow_rebuild(c, t, before);
    }
    gained
}

/// The stat rebuild after a troop's level or class changed: from its HP and maximum
/// `before`, a wounded living troop keeps its HP in proportion to the new maximum with the
/// fractional carry ([`crate::rules::units::follow_max`]).
fn follow_rebuild(c: &Content, t: &mut Troop, (hp, old_max): (i32, i32)) {
    if !t.alive() {
        return;
    }
    let new_max = troop_max_hp(c, t);
    let hp = crate::rules::units::follow_max(hp, &mut t.carry, old_max, new_max);
    t.hurt = (new_max - hp).max(0);
}

/// How many levels one gain may add (a bound for absurd amounts).
const MAX_LEVELS_AT_ONCE: i32 = 200;

/// An AI unit's gain (0x4a4a7c): `award × pct div 100` XP, then one try at the upgrade tree
/// ([`ai_promote`]), the roll made even when nothing was gained.
pub fn ai_gain_xp(c: &Content, rng: &mut Rng, t: &mut Troop, award: i32, pct: i32, pool: &mut Vec<ItemId>) {
    troop_gain_xp(c, t, (pct as i64 * award as i64 / 100) as i32);
    ai_promote(c, rng, t, pool);
}

/// XP a newly hired AI unit starts with (0x4a4c04): fed level by level, each level a gain
/// with a promotion try; what does not reach a level stays as its XP.
fn ai_hire_gain(c: &Content, rng: &mut Rng, t: &mut Troop, xp: i32, pool: &mut Vec<ItemId>) {
    let mut left = xp.max(0).saturating_add(t.xp);
    for _ in 0..MAX_LEVELS_AT_ONCE {
        let need = c.xp_to_next(t.unit, t.level);
        if left < need {
            t.xp = left;
            return;
        }
        ai_gain_xp(c, rng, t, need, 100, pool);
        left -= need;
        if left == 0 {
            return;
        }
    }
}

/// The AI's pick in the upgrade tree (0x4a4a7c, ai.md §11): Militia (unit 4) tries slot 1
/// one time in three and slot 3 otherwise, Infantry (unit 8) slot 3 one time in three and
/// slot 1 otherwise, any other class `Rand(3) + 1` until it hits a filled slot. The pick is
/// taken when its `NextUnitNLevel` is at most the unit's 0-based level: the unit becomes that
/// class at level 1 (the original's 0) with no XP, its worn items to the pool.
fn ai_promote(c: &Content, rng: &mut Rng, t: &mut Troop, pool: &mut Vec<ItemId>) -> bool {
    let Some(target) = ai_pick(c, rng, t.unit, t.level) else { return false };
    let before = troop_hp(c, t);
    t.unit = target;
    t.level = 1;
    t.xp = 0;
    pool.extend(t.worn.iter_mut().filter_map(Option::take));
    follow_rebuild(c, t, before);
    true
}

/// The class a unit of type `unit` at `level` (1 = as hired) takes by the AI's roll in the
/// upgrade tree ([`ai_promote`]), if the pick is open to it. The roll is made whenever the
/// type has an option, so it always advances the seed then.
fn ai_pick(c: &Content, rng: &mut Rng, unit: UnitId, level: i32) -> Option<UnitId> {
    let def = c.unit(unit);
    let slots: [Option<&super::content::Upgrade>; 3] = [1u8, 2, 3].map(|n| def.upgrades.iter().find(|u| u.slot == n && u.target.is_some()));
    if slots.iter().all(Option::is_none) {
        return None;
    }
    let pick = match unit.0 {
        4 => {
            if rng.random(3) == 0 {
                1
            } else {
                3
            }
        }
        8 => {
            if rng.random(3) == 0 {
                3
            } else {
                1
            }
        }
        _ => loop {
            let n = rng.random(3) as usize + 1;
            if slots[n - 1].is_some() {
                break n;
            }
        },
    };
    // Militia and Infantry always hold two options, in slots 1 and 3 (the loader's moves);
    // were one empty, the original would turn the unit into an invalid class.
    let up = slots[pick - 1]?;
    let target = up.target.map(UnitId).filter(|&id| c.try_unit(id).is_some())?;
    // `NextUnitNLevel ≤ L` with the 0-based L = level − 1.
    (up.level < level).then_some(target)
}

/// One of the player's units left in a building gains as the building's garrison does when
/// it holds out against an AI army (0x4a4c68 → 0x4a4a7c): the share × `AIExpiriencePercent`
/// div 100, then the AI's roll in the upgrade tree, which may promote it, its worn items to
/// the battle's pool. The original keeps the player's units in the garrison record, so the
/// AI's rule reaches them too.
fn stationed_gain_xp(c: &Content, rng: &mut Rng, u: &mut Unit, award: i32, pct: i32, pool: &mut Vec<ItemId>) {
    u.gain_xp(c, (pct as i64 * award as i64 / 100) as i32);
    if let Some(target) = ai_pick(c, rng, u.def, u.level) {
        let before = u.max_hp(c);
        u.def = target;
        u.level = 1;
        u.xp = 0;
        pool.extend(u.items.iter_mut().filter_map(Option::take));
        u.follow_max(c, before);
    }
}

fn army_name(a: &Army) -> String {
    if a.name.trim().is_empty() {
        crate::i18n::tr("An army").to_string()
    } else {
        a.name.trim().to_string()
    }
}

fn building_name(l: &Location) -> String {
    if l.name.trim().is_empty() {
        match crate::i18n::lang() {
            crate::i18n::Lang::En => format!("a {}", l.kind.label().to_lowercase()),
            crate::i18n::Lang::Ru => l.kind.label().to_lowercase(),
        }
    } else {
        l.name.trim().to_string()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod real_maps;

