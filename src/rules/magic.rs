//! World-map spells (original-mechanics/magic-items.md §2–§4): the hero casts a spell of his
//! book on his own army (`Target=Hero`) or on any army he can point at (`Target=Enemy`,
//! `OneEnemy`). Casting costs mana **and game time**: the world moves on meanwhile (armies
//! walk, the scenario's events run; the noon waits), so an event may cut the wait short.
//!
//! - Cost: a function of `CostMana` for the mana and of `2 × TimeCast` for the half-hour
//!   steps: `x div 2` for the Archmage, else `floor(4x/5)` with a `Caster` in the army
//!   ([`cast_cost`]). The mana is taken when the spell lands, with no check.
//! - Each unit holds up to [`SPELL_SLOTS`] spells, each for exactly `TimeWork` hours; casting
//!   a spell a unit holds adds another `TimeWork`; a unit with no free slot is left alone.
//!   The modifiers count in the unit's stats ([`crate::rules::items::rebuild_stats`]).
//! - `DeltaFixedHits` / `DeltaPercentHits` heal or wound at once and can kill; `p-LifeLose`
//!   changes the unit's lasting drain. `OneEnemy` spells, and any spell with `p-LifeLose`,
//!   touch only the first unit.
//! - The scenario's events and the villages cast spells on the player's army through the same
//!   path ([`Game::apply_spell_to_army_ext`]), for free and at once; an own-army spell then
//!   lasts 10 times as long (5 times when `TimeWork` ≥ 8).

use serde::{Deserialize, Serialize};

use crate::dt::data::SpellTarget;

use super::clock::MINUTES_PER_HOUR;
use super::content::{Bonus, Content, HeroClass, SpellDef};
use super::game::{troop_unit, unit_into_troop, Event, Game};
use super::units::{SpellSlot, Unit};
pub use super::units::SPELL_SLOTS;

/// The Community bonus token of units that cast world spells 20% faster and cheaper.
pub const CASTER_BONUS: &str = "Caster";
/// Game time passes in half-hour steps while casting.
pub const CAST_STEP_MINUTES: u64 = 30;
/// The magic window shows (and casts) only the first 15 spells of the book (3 × 5 cells).
pub const CASTABLE: usize = 15;
/// The end Community opcode 11 gives its "permanent" spells: 15,658,734 hundredths of a minute
/// after the map start (0xEEEEEE), about 108.7 game days, in whole minutes rounded up.
pub const OPCODE_SPELL_END: u64 = 156_588;
/// Slice of game time simulated at once while casting, as for waits.
const STEP_MINUTES: f32 = super::game::WAIT_TICK_MINUTES;

/// A lasting spell of a save before format 7, when spells were kept per army: which
/// spell, the game minute it ends (`None`: never), and whether it held only the leader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveSpell {
    pub spell: u32,
    pub until: Option<u64>,
    #[serde(default)]
    pub leader: bool,
}

/// Moves the army-wide spells of an old save into the units' slots (`leader`: into the first
/// unit only), each into the slot of the same spell or the first free one.
pub fn migrate_old_spells(old: &[ActiveSpell], units: &mut [Unit], permanent: u64) {
    for e in old {
        let until = e.until.unwrap_or(permanent);
        for u in units.iter_mut().take(if e.leader { 1 } else { usize::MAX }) {
            let k = u.spells.iter().position(|s| s.is_some_and(|s| s.spell == e.spell)).or_else(|| u.spells.iter().position(Option::is_none));
            if let Some(k) = k {
                u.spells[k] = Some(SpellSlot { spell: e.spell, until });
            }
        }
    }
}

/// How long a spell's modifiers last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duration {
    Instant,
    Minutes(u64),
}

impl Duration {
    /// Exactly `TimeWork` hours (no scaling by the caster's level; 9999 is simply 9999 h).
    pub fn of(spell: &SpellDef) -> Duration {
        match spell.time_work {
            None | Some(..=0) => Duration::Instant,
            Some(h) => Duration::Minutes(h as u64 * MINUTES_PER_HOUR),
        }
    }

    /// Cast by an event or a village on the player's army: `TimeWork × 10` hours, or × 5
    /// when `TimeWork` ≥ 8.
    pub fn extended(spell: &SpellDef) -> Duration {
        match spell.time_work {
            None | Some(..=0) => Duration::Instant,
            Some(h) => Duration::Minutes(h as u64 * if h >= 8 { 5 } else { 10 } * MINUTES_PER_HOUR),
        }
    }
}

/// Mana and game minutes a cast takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastCost {
    pub mana: i32,
    pub minutes: u64,
}

/// The cost function of the Community build (0xc27448): `x div 2` for the Archmage (who
/// ignores Caster), else with the Caster flag `floor(4x/5)` as a multiply by 0xcccccccd,
/// else `x`. Razdor fixes the original's bug: the multiply was unsigned, so a negative `x`
/// came out huge; here it keeps its sign.
pub fn discounted(x: i32, archmage: bool, caster: bool) -> i32 {
    if archmage {
        x / 2
    } else if caster {
        let scaled = ((x.unsigned_abs() as u64 * 0xcccc_cccd) >> 32) as i32;
        if x < 0 { -scaled } else { scaled }
    } else {
        x
    }
}

/// The casting time the spell card shows, in hours: [`discounted`] `TimeCast` (0x49bbe0, not
/// doubled), which is half an hour short of the real wait with Caster, and for the Archmage
/// whenever `TimeCast` is odd (the original's: the card shows whole hours).
pub fn card_cast_hours(spell: &SpellDef, archmage: bool, caster: bool) -> i32 {
    discounted(spell.time_cast.unwrap_or(0), archmage, caster)
}

/// The mana ([`discounted`] `CostMana`) and the half-hour steps ([`discounted`]
/// `2 × TimeCast`) of a cast; no steps or fewer end the wait at once.
pub fn cast_cost(spell: &SpellDef, archmage: bool, caster: bool) -> CastCost {
    let steps = discounted(spell.time_cast.unwrap_or(0).wrapping_mul(2), archmage, caster);
    CastCost { mana: discounted(spell.cost_mana, archmage, caster), minutes: steps.max(0) as u64 * CAST_STEP_MINUTES }
}


/// Casts on an enemy army (`Enemy` and `OneEnemy`); everything else on the hero's own army.
pub fn targets_enemy(spell: &SpellDef) -> bool {
    matches!(spell.target, Some(SpellTarget::Enemy | SpellTarget::OneEnemy))
}

/// The spell changes stats for a while (it has modifiers and a duration).
pub fn is_lasting(spell: &SpellDef) -> bool {
    let mods = !spell.add.is_empty() || !spell.percent.is_empty();
    mods && Duration::of(spell) != Duration::Instant
}

/// What a spell does to one unit (0x4900fc, magic-items.md §4.1), `max` being its maximum
/// HP before the cast. Expired slots are emptied; the slot holding the spell gets another
/// `TimeWork` hours, else the first free slot gets it for `TimeWork` hours; with neither,
/// nothing happens to the unit at all (false). The instant part follows only for the living.
/// Razdor fixes the original's bug: its recast branch had no HP test, so a dead unit still
/// holding the spell was raised by a heal. `event_reset`: an event's or a village's own-army
/// spell, whose end is then set to now + 10 × `TimeWork` hours (× 5 from 8 h), replacing
/// what a recast added.
fn spell_on_unit(spell: &SpellDef, u: &mut Unit, max: i32, now: u64, event_reset: bool) -> bool {
    u.expire_spells(now);
    let work = spell.time_work.unwrap_or(0) as i64;
    let end = |hours: i64| (now as i64 + hours * MINUTES_PER_HOUR as i64).max(0) as u64;
    let k = u.spells.iter().position(|s| s.is_some_and(|s| s.spell == spell.id)).or_else(|| u.spells.iter().position(Option::is_none));
    let Some(k) = k else { return false };
    match &mut u.spells[k] {
        Some(slot) => slot.until = (slot.until as i64 + work * MINUTES_PER_HOUR as i64).max(0) as u64,
        empty => *empty = Some(SpellSlot { spell: spell.id, until: end(work) }),
    }
    if u.hp != 0 {
        instant_part(spell, u, max, now);
    }
    if event_reset {
        let hours = work * if work < 8 { 10 } else { 5 };
        if let Some(slot) = u.spells[k].as_mut() {
            slot.until = end(hours);
        }
    }
    true
}

/// Someone of the army a spell hit lives: its first unit (the hero, a leader) at 0 HP gets 1.
/// Razdor fixes the original's bug: 0x4900fc tested the *player's* hero whatever army was
/// hit, so a curse on an enemy that left a survivor raised his fallen hero, and not the
/// enemy's leader.
fn raise_leader(units: &mut [Unit]) {
    if let Some(h) = units.first_mut().filter(|h| h.hp == 0) {
        h.hp = 1;
        h.died_at = None;
    }
}

/// The instant part of a spell on a unit of maximum `max` (magic-items.md §4.1 step 4): a
/// negative `p-LifeLose` L compounds the drain D into `100 − (100 − D)·(100 + L)/100` and
/// takes `HP·|L|/100` now, a positive one lowers D (not below 0); then `DeltaFixedHits`,
/// then `DeltaPercentHits` of the maximum (a gain) or of the HP (a loss). Below 0 is dead,
/// at the maximum unhurt.
fn instant_part(spell: &SpellDef, u: &mut Unit, max: i32, now: u64) {
    let was_alive = u.alive();
    let mut hp = u.hp.min(max);
    match spell.life_lose_percent.unwrap_or(0) {
        l if l < 0 => {
            u.drain = 100 - (100 - u.drain).wrapping_mul(100 + l) / 100;
            hp += l.wrapping_mul(hp) / 100;
        }
        l if l > 0 => u.drain = (u.drain - l).max(0),
        _ => {}
    }
    hp = hp.wrapping_add(spell.delta_fixed_hits.unwrap_or(0));
    match spell.delta_percent_hits.unwrap_or(0) {
        p if p > 0 => hp = hp.wrapping_add(p.wrapping_mul(max) / 100),
        p if p < 0 => hp = hp.wrapping_add(p.wrapping_mul(hp) / 100),
        _ => {}
    }
    u.hp = if hp < 0 { 0 } else { hp.min(max) };
    if was_alive && u.hp == 0 {
        u.died_at = Some(now);
    } else if !was_alive && u.hp > 0 {
        u.died_at = None;
    }
}

/// A spell on a whole army's units (0x4900fc): the first unit only for `OneEnemy` and for a
/// `p-LifeLose` spell, else every unit, the dead included in the loop. Then every dead
/// unit loses its 4 slots. Returns whether someone lives, and each unit's maximum HP before
/// the cast for [`rebuild_after_spell`].
fn spell_on_units(content: &Content, spell: &SpellDef, units: &mut [Unit], now: u64, event_reset: bool) -> (bool, Vec<i32>) {
    let before: Vec<i32> = units.iter().map(|u| u.max_hp(content)).collect();
    let only_first = spell.target == Some(SpellTarget::OneEnemy);
    let n = if only_first { units.len().min(1) } else { units.len() };
    let drains = spell.life_lose_percent.unwrap_or(0) != 0;
    for (i, u) in units.iter_mut().enumerate().take(n) {
        if i == 0 || !drains {
            spell_on_unit(spell, u, before[i], now, event_reset);
        }
    }
    for u in units.iter_mut().filter(|u| !u.alive()) {
        u.spells = [None; SPELL_SLOTS];
    }
    (units.iter().any(Unit::alive), before)
}

/// The army's rebuild after a spell when someone lives (0x497240): the new modifiers and
/// drain count, each unit's HP follows its maximum.
fn rebuild_after_spell(content: &Content, units: &mut [Unit], before: &[i32], now: u64) {
    for (u, &max) in units.iter_mut().zip(before) {
        u.expire_spells(now);
        u.follow_max(content, max);
    }
}

/// Whom to cast on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CastTarget {
    /// The hero's own army.
    Own,
    /// An army on the map, by its [`crate::rules::world::Army::uid`].
    Army(u32),
}

/// A spell being read on the map ([`Game::begin_cast`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub spell: u32,
    pub target: CastTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastError {
    /// Not among the first 15 spells of the book (the only ones the window shows).
    NotInBook,
    NoSuchSpell,
    NotEnoughMana,
    /// An own-army spell cast on an enemy, or the other way round.
    WrongTarget,
    /// The army cannot be pointed at: on unexplored or impassable ground, in a building, or
    /// a friend with a meeting waiting.
    NotATarget,
    /// A battle is pending.
    Busy,
}

/// How a cast ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CastOutcome {
    /// The spell took effect. `hits`: HP healed (+) or dealt (−) in all; `killed`: units
    /// that fell; `destroyed`: the whole army fell (an enemy is gone; the player's army
    /// means the game is lost).
    Done { hits: i32, killed: usize, destroyed: bool },
    /// An enemy caught the hero while he was casting: the spell is lost, no mana is taken.
    Interrupted,
    /// The target left the map or was destroyed before the spell was ready; no mana is taken.
    TargetLost,
}

/// A finished cast: how it ended, and what happened while time passed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cast {
    pub outcome: CastOutcome,
    pub events: Vec<Event>,
}

impl Game {
    /// Spell definition by its 1-based id.
    pub fn spell(&self, id: u32) -> Option<&SpellDef> {
        self.content.spell(id)
    }

    /// The spells of the hero's book the magic window shows (its first 15 entries), in book
    /// order.
    pub fn book(&self) -> Vec<&SpellDef> {
        self.spells.iter().take(CASTABLE).filter_map(|&id| self.spell(id as u32)).collect()
    }

    /// The `Caster` discount: a unit of the hero's army, dead or alive, whose type's bonus or
    /// current bonus (an item's) is Caster. The original reads a global flag instead, left by
    /// whichever army was recomputed last (often an AI army); Razdor keeps the hero's army.
    pub fn caster_in_army(&self) -> bool {
        let caster = Bonus::parse(CASTER_BONUS);
        let c = &self.content;
        self.squad.iter().any(|u| c.unit(u.def).bonus.as_ref() == Some(&caster) || u.stats(c).has(&caster))
    }

    /// What casting `spell` costs this hero: see [`cast_cost`].
    pub fn cast_cost(&self, spell: &SpellDef) -> CastCost {
        cast_cost(spell, self.start_class() == HeroClass::Archmage, self.caster_in_army())
    }

    /// The hours the spell card shows as its casting time ([`card_cast_hours`]).
    pub fn card_cast_hours(&self, spell: &SpellDef) -> i32 {
        card_cast_hours(spell, self.start_class() == HeroClass::Archmage, self.caster_in_army())
    }

    /// Army `i` can be pointed at with an enemy spell (0x4cc148): it stands on a cell a
    /// click can target ([`Game::can_target`], 0x4cbf20: explored, open on the hero's planner
    /// map), outside any building, and it is not a friend (attitude 1 or more) with a meeting
    /// event waiting. No distance, no sight: an army under the fog of explored ground
    /// counts, and other friends may be cursed.
    fn can_curse(&self, i: usize) -> bool {
        let a = &self.world.armies[i];
        let t = a.tile(&self.world.map);
        let talks = a.attitude >= 1 && self.script.as_deref().is_some_and(|e| e.meeting_waiting(self, a.id));
        self.can_target(t) && self.world.location_at(t).is_none() && !talks
    }

    /// The armies the hero can cast an enemy spell on now ([`Game::can_curse`]), nearest
    /// first: indices into `world.armies`.
    pub fn spell_targets(&self) -> Vec<usize> {
        let map = &self.world.map;
        let here = self.tile();
        let mut v: Vec<(i32, usize)> = (0..self.world.armies.len()).filter(|&i| self.can_curse(i)).map(|i| (map.distance(self.world.armies[i].tile(map), here), i)).collect();
        v.sort();
        v.into_iter().map(|(_, i)| i).collect()
    }

    /// The spells on the hero's army now, each with the latest end among its units.
    pub fn active_spells(&self) -> Vec<SpellSlot> {
        let mut v: Vec<SpellSlot> = Vec::new();
        for s in self.squad.iter().flat_map(|u| u.spells.iter().flatten()) {
            match v.iter_mut().find(|x| x.spell == s.spell) {
                Some(x) => x.until = x.until.max(s.until),
                None => v.push(*s),
            }
        }
        v
    }

    /// The spells on the units of army `i` of the map.
    pub fn spells_on_army(&self, i: usize) -> Vec<&SpellDef> {
        let mut ids: Vec<u32> = Vec::new();
        for s in self.world.armies[i].troops.iter().flat_map(|t| t.spells.iter().flatten()) {
            if !ids.contains(&s.spell) {
                ids.push(s.spell);
            }
        }
        ids.into_iter().filter_map(|id| self.spell(id)).collect()
    }

    /// The minute Community opcode 11's spells end: [`OPCODE_SPELL_END`] after the map start.
    pub(crate) fn opcode_spell_end(&self) -> u64 {
        self.map_start() + OPCODE_SPELL_END
    }

    /// Empties the spell slots whose time is up, on every unit of the hero's army, of the
    /// armies and of the garrisons, and rebuilds the units that lost one (their HP follows
    /// their maximum).
    pub(crate) fn expire_spells(&mut self) {
        let now = self.clock.total_minutes() as u64;
        let c = self.content.clone();
        let stationed = self.world.locations.iter_mut().flat_map(|l| l.stationed.iter_mut().map(|s| &mut s.unit));
        for u in self.squad.iter_mut().chain(stationed) {
            let before = u.max_hp(&c);
            if u.expire_spells(now) {
                u.follow_max(&c, before);
            }
        }
        let w = &mut self.world;
        let armies = w.armies.iter_mut().chain(w.inactive.iter_mut()).flat_map(|a| a.troops.iter_mut());
        for t in armies.chain(w.locations.iter_mut().flat_map(|l| l.garrison.iter_mut())) {
            if t.spells.iter().flatten().any(|s| s.until <= now) {
                let mut u = troop_unit(&c, t);
                let before = u.max_hp(&c);
                u.expire_spells(now);
                u.follow_max(&c, before);
                unit_into_troop(&c, t, &u, now);
            }
        }
    }

    /// Casts `spell` of the book on `target`: lets [`CastCost::minutes`] of game time pass
    /// (armies move, events run, the noon waits; an event that fires ends the wait at once;
    /// an enemy reaching the hero loses the spell), then the spell lands
    /// ([`Game::end_reading`]'s rules).
    pub fn cast(&mut self, spell: u32, target: CastTarget) -> Result<Cast, CastError> {
        self.check_cast(spell, target)?;
        let cost = self.cast_cost(self.spell(spell).ok_or(CastError::NoSuchSpell)?);
        self.stop();
        self.reading = Some(Reading { spell, target });
        let mut events = Vec::new();
        let mut left = cost.minutes as f32;
        while left > 0.0 {
            let slice = left.min(STEP_MINUTES);
            left -= slice;
            let from = events.len();
            self.pass_time(slice, &mut events);
            if self.foe.is_some() || events[from..].iter().any(super::game::fires) {
                break;
            }
        }
        let mut done = Vec::new();
        self.end_reading(&mut done);
        let outcome = match done.pop() {
            Some(Event::SpellCast { outcome, .. }) => outcome,
            _ => CastOutcome::TargetLost,
        };
        Ok(Cast { outcome, events })
    }

    /// Starts reading `spell` on the map, as the UI casts: the reading plays in real time,
    /// half an hour of game time per wait tick like a rest ([`Game::tick`]), and the spell
    /// lands at its end, or as soon as an event fires, as an [`Event::SpellCast`]; an enemy
    /// catching the hero meanwhile loses it, and anything that stops the hero (a walk, a
    /// rest, Space) drops it with no mana spent. A spell with no reading time lands at once
    /// (`Some`).
    pub fn begin_cast(&mut self, spell: u32, target: CastTarget) -> Result<Option<CastOutcome>, CastError> {
        self.check_cast(spell, target)?;
        let cost = self.cast_cost(self.spell(spell).ok_or(CastError::NoSuchSpell)?);
        self.stop();
        let ticks = (cost.minutes as f32 / STEP_MINUTES).ceil() as u32;
        self.reading = Some(Reading { spell, target });
        if ticks == 0 {
            let mut done = Vec::new();
            self.end_reading(&mut done);
            return Ok(match done.pop() {
                Some(Event::SpellCast { outcome, .. }) => Some(outcome),
                _ => None,
            });
        }
        self.wait_ticks = ticks;
        Ok(None)
    }

    /// The spell being read, if any ([`Game::begin_cast`]), and the game minutes left.
    pub fn reading(&self) -> Option<(u32, f32)> {
        self.reading.map(|r| (r.spell, self.wait_ticks as f32 * STEP_MINUTES))
    }

    /// The reading ended (its ticks all played, an event fired, or an enemy fell on the
    /// hero): the spell lands on its target, or is lost; then the spells events cast
    /// meanwhile land.
    ///
    /// Razdor fixes the original's bug: an event that cast a spell left the target code at 1,
    /// "an event's cast" (0x68eca4, 0x4ab1ec tail), and the reading's spell read that code
    /// when it landed (0x4af2f8), so it hit the player's own army whatever its target, as an
    /// event's cast (for free, an own-army spell with the event's 10× or 5× time).
    pub(crate) fn end_reading(&mut self, events: &mut Vec<Event>) {
        let queued = std::mem::take(&mut self.queued_casts);
        if let Some(r) = self.reading.take() {
            self.wait_ticks = 0;
            let outcome = match self.spell(r.spell).cloned() {
                _ if self.foe.is_some() => CastOutcome::Interrupted,
                Some(def) => self.land_spell(&def, r.target),
                None => CastOutcome::TargetLost,
            };
            events.push(Event::SpellCast { spell: r.spell, target: r.target, outcome });
        }
        for id in queued {
            if let Some(def) = self.spell(id).cloned() {
                self.apply_spell_to_army_ext(&def, true);
                events.push(Event::EventSpell { spell: id });
            }
        }
    }

    /// A cast may start (0x4c2e34): the spell is among the first 15 of the book, the target
    /// suits it and can be pointed at, the mana covers its cost, no battle is pending.
    fn check_cast(&self, spell: u32, target: CastTarget) -> Result<(), CastError> {
        if self.foe.is_some() {
            return Err(CastError::Busy);
        }
        if !self.spells.iter().take(CASTABLE).any(|&s| s as u32 == spell) {
            return Err(CastError::NotInBook);
        }
        let def = self.spell(spell).ok_or(CastError::NoSuchSpell)?;
        match target {
            CastTarget::Own if targets_enemy(def) => return Err(CastError::WrongTarget),
            CastTarget::Army(_) if !targets_enemy(def) => return Err(CastError::WrongTarget),
            CastTarget::Army(uid) if !self.spell_targets().iter().any(|&i| self.world.armies[i].uid == uid) => return Err(CastError::NotATarget),
            _ => {}
        }
        if self.mana < self.cast_cost(def).mana {
            return Err(CastError::NotEnoughMana);
        }
        Ok(())
    }

    /// The reading done, the spell takes effect (0x4af2f8): on the target if it is still on
    /// the map (it may have walked anywhere). The mana is paid now, its cost worked out
    /// again, with no check: if the mana ran low meanwhile it goes below zero.
    fn land_spell(&mut self, def: &SpellDef, target: CastTarget) -> CastOutcome {
        let aim = match target {
            CastTarget::Own => None,
            CastTarget::Army(uid) => match self.world.armies.iter().position(|a| a.uid == uid) {
                Some(i) => Some(i),
                None => return CastOutcome::TargetLost,
            },
        };
        let outcome = match aim {
            None => self.spell_on_player(def, false),
            Some(i) => self.apply_spell_to_enemy(def, i),
        };
        self.mana = self.mana.wrapping_sub(self.cast_cost(def).mana);
        outcome
    }

    /// A spell the hero casts takes effect on his army ([`Game::apply_spell_to_army_ext`]
    /// with the spell's own duration).
    pub fn apply_spell_to_army(&mut self, spell: &SpellDef) -> i32 {
        self.apply_spell_to_army_ext(spell, false)
    }

    /// The spell takes effect on the hero's army (magic-items.md §4). `by_event`: cast by an
    /// event or a village, whatever its target: an own-army spell then lasts
    /// [`Duration::extended`]. Returns the HP change in all.
    pub fn apply_spell_to_army_ext(&mut self, spell: &SpellDef, by_event: bool) -> i32 {
        match self.spell_on_player(spell, by_event) {
            CastOutcome::Done { hits, .. } => hits,
            _ => 0,
        }
    }

    /// [`spell_on_units`] on the hero's army. If someone lives and the hero has 0 HP he is
    /// set to 1, before the rebuild (so his 1 HP follows his maximum too); if nobody lives
    /// the game is lost ([`Game::army_fallen`]).
    fn spell_on_player(&mut self, spell: &SpellDef, by_event: bool) -> CastOutcome {
        let c = self.content.clone();
        let now = self.clock.total_minutes() as u64;
        let before: Vec<(i32, bool)> = self.squad.iter().map(|u| (u.hp, u.alive())).collect();
        let reset = by_event && !targets_enemy(spell);
        let (alive, max) = spell_on_units(&c, spell, &mut self.squad, now, reset);
        if alive {
            raise_leader(&mut self.squad);
            rebuild_after_spell(&c, &mut self.squad, &max, now);
            // 0x4900fc → 0x497240(0): the rebuild recounts his army.
            self.recount_hero();
        }
        let hits = self.squad.iter().zip(&before).map(|(u, b)| u.hp - b.0).sum();
        let killed = self.squad.iter().zip(&before).filter(|(u, b)| b.1 && !u.alive()).count();
        CastOutcome::Done { hits, killed, destroyed: !alive }
    }


    /// The spell takes effect on army `i` of the map ([`spell_on_units`]); an army with
    /// nobody left alive is destroyed as beaten by the player, with no loot and no XP.
    fn apply_spell_to_enemy(&mut self, spell: &SpellDef, i: usize) -> CastOutcome {
        let c = self.content.clone();
        let now = self.clock.total_minutes() as u64;
        let mut units: Vec<Unit> = self.world.armies[i].troops.iter().map(|t| troop_unit(&c, t)).collect();
        let before: Vec<(i32, bool)> = units.iter().map(|u| (u.hp, u.alive())).collect();
        let (alive, max) = spell_on_units(&c, spell, &mut units, now, false);
        if alive {
            raise_leader(&mut units);
            rebuild_after_spell(&c, &mut units, &max, now);
        }
        for (t, u) in self.world.armies[i].troops.iter_mut().zip(&units) {
            unit_into_troop(&c, t, u, now);
        }
        if alive {
            // 0x4900fc → 0x497240(i): the rebuild recounts the army with the defence it has.
            self.recount_bill(i);
            let m = &mut self.world.armies[i].mind;
            m.strength_bd = m.defence;
        }
        let hits = units.iter().zip(&before).map(|(u, b)| u.hp - b.0).sum();
        let killed = units.iter().zip(&before).filter(|(u, b)| b.1 && !u.alive()).count();
        if !alive {
            self.army_beaten(i, crate::rules::ai::Beaten::ByPlayer);
            let events = self.run_script();
            self.pending.extend(events);
        }
        CastOutcome::Done { hits, killed, destroyed: !alive }
    }

    /// Nobody of the hero's army lives: the game is lost (a world spell can do that; the
    /// original then plays its defeat).
    pub fn army_fallen(&self) -> bool {
        !self.squad.iter().any(Unit::alive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::battle::{Outcome, Team};
    use crate::rules::content::testkit as ck;
    use crate::rules::content::{Content, Stat, StatMods, UnitDef, UnitId};
    use crate::rules::formation::Formation;
    use crate::rules::game::Foe;
    use crate::rules::world::testkit::{self as tk, army, hero, scenario, troop};
    use std::sync::Arc;

    fn spell(id: u32, mana: i32, cast: i32, work: Option<i32>) -> SpellDef {
        SpellDef { cost_mana: mana, time_cast: Some(cast), time_work: work, delta_fixed_hits: None, ..ck::spell(id, 100) }
    }

    /// 1 heal +30 (instant, 4 h), 2 armour d-DefenceBlow +5 and p-Hits +20 for 10 h,
    /// 3 lightning −15 on an enemy (6 h), 4 weakness p-AttackBlow −50 on an enemy for 8 h,
    /// 5 a curse p-LifeLose −20 for 9999 h, 6 its lifting p-LifeLose +20.
    fn spells() -> Vec<SpellDef> {
        let heal = SpellDef { delta_fixed_hits: Some(30), ..spell(1, 200, 4, None) };
        let armour = SpellDef {
            add: StatMods::from([(Stat::DefenceBlow, 5)]),
            percent: StatMods::from([(Stat::Hits, 20)]),
            ..spell(2, 300, 6, Some(10))
        };
        let lightning = SpellDef { delta_fixed_hits: Some(-15), target: Some(SpellTarget::Enemy), ..spell(3, 500, 6, None) };
        let weakness =
            SpellDef { percent: StatMods::from([(Stat::AttackBlow, -50)]), target: Some(SpellTarget::Enemy), ..spell(4, 160, 2, Some(8)) };
        let curse = SpellDef { life_lose_percent: Some(-20), ..spell(5, 1, 1, Some(9999)) };
        let lift = SpellDef { life_lose_percent: Some(20), time_cast: None, ..spell(6, 1, 1, None) };
        vec![heal, armour, lightning, weakness, curse, lift]
    }

    fn content() -> Arc<Content> {
        let c = tk::content();
        let mut units = c.units.clone();
        // Unit 12: a caster (bonus Caster), unit 13: a frail peasant (10 HP), unit 14: 100 HP.
        units.push(UnitDef { bonus: Some(Bonus::parse(CASTER_BONUS)), ..ck::warrior(12, 5, 1) });
        units.push(UnitDef { hits: 10, ..ck::warrior(13, 3, 0) });
        units.push(UnitDef { hits: 100, ..ck::warrior(14, 3, 0) });
        Arc::new(Content::new(units, c.items.clone(), spells(), c.options.clone(), Formation::WIDE))
    }

    /// A 24×6 strip; the hero starts at (2, 2) with two warriors, 1000 mana and all spells.
    fn game(class: HeroClass) -> Game {
        let mut s = scenario(24, 6);
        s.header.heroes[0] = hero(2, 2, 200, &[troop(4, 0, 2)]);
        s.header.heroes[1] = hero(2, 2, 200, &[troop(4, 0, 2)]);
        let mut g = Game::from_scenario(content(), &s, class);
        g.mana = 1000;
        g.spells = (1..=6).collect();
        g
    }

    fn with_enemy(g: &mut Game, at: (u16, u16), troops: &[crate::dt::dtm::Troop]) -> u32 {
        let mut s = scenario(24, 6);
        s.armies = vec![army(9, at.0, at.1, -2, troops)];
        let w = crate::rules::world::World::from_scenario(&s, &g.content);
        let mut a = w.armies[0].clone();
        // A stationary guard: it waits where it stands (the AI's armies wander).
        a.patrols = true;
        a.patrol_radius = 0;
        let uid = a.uid;
        g.world.armies.push(a);
        uid
    }

    fn slot(spell: u32, until: u64) -> SpellSlot {
        SpellSlot { spell, until }
    }

    fn now(g: &Game) -> u64 {
        g.clock.total_minutes() as u64
    }

    #[test]
    fn cost_and_time_with_archmage_and_caster() {
        let s = spell(1, 200, 4, None);
        assert_eq!(cast_cost(&s, false, false), CastCost { mana: 200, minutes: 240 });
        assert_eq!(cast_cost(&s, true, false), CastCost { mana: 100, minutes: 120 }, "twice as fast for half the mana");
        // Caster: floor(4x/5) of the mana and of the 8 half-hour steps.
        assert_eq!(cast_cost(&s, false, true), CastCost { mana: 160, minutes: 180 }, "Caster: -20%");
        assert_eq!(cast_cost(&s, true, true), CastCost { mana: 100, minutes: 120 }, "they do not stack");
        // A negative cost keeps its sign (the original's unsigned multiply made it huge).
        assert_eq!(discounted(-10, false, true), -8);

        let mut g = game(HeroClass::Archmage);
        assert_eq!(g.hero_class(), Some(HeroClass::Archmage));
        assert_eq!(g.cast_cost(g.spell(1).unwrap()), CastCost { mana: 100, minutes: 120 });
        g.squad[1].def = UnitId(12);
        assert_eq!(g.cast_cost(g.spell(1).unwrap()), CastCost { mana: 100, minutes: 120 });
        let mut k = game(HeroClass::Knight);
        k.squad[1].def = UnitId(12);
        assert_eq!(k.cast_cost(k.spell(1).unwrap()), CastCost { mana: 160, minutes: 180 });
        k.squad[1].hp = 0;
        assert_eq!(k.cast_cost(k.spell(1).unwrap()).mana, 160, "a dead Caster counts (the recompute walks every unit)");
    }

    #[test]
    fn the_card_shows_the_function_of_time_cast_not_the_real_wait() {
        // 0x49bbe0: TimeCast 2 with Caster shows floor(8/5) = 1 h; the wait is floor(16/5)
        // = 3 half-hours. The Archmage with TimeCast 3 sees 1 h and waits 3 half-hours.
        let two = spell(1, 10, 2, None);
        assert_eq!((card_cast_hours(&two, false, true), cast_cost(&two, false, true).minutes), (1, 90));
        let three = spell(1, 10, 3, None);
        assert_eq!((card_cast_hours(&three, true, false), cast_cost(&three, true, false).minutes), (1, 90));
        assert_eq!((card_cast_hours(&three, false, false), cast_cost(&three, false, false).minutes), (3, 180));
    }

    #[test]
    fn casting_spends_mana_and_game_time() {
        let mut g = game(HeroClass::Knight);
        g.squad[1].hp = 5;
        let start = g.clock.total_minutes();
        let cast = g.cast(1, CastTarget::Own).unwrap();
        assert_eq!(cast.outcome, CastOutcome::Done { hits: 30, killed: 0, destroyed: false });
        assert_eq!((g.mana, g.clock.total_minutes() - start), (800, 240.0));
        assert_eq!(g.squad[1].hp, 35);
        assert!(g.active_spells().is_empty(), "instant");
        g.mana = 10;
        assert_eq!(g.cast(1, CastTarget::Own), Err(CastError::NotEnoughMana));
        assert_eq!(g.cast(3, CastTarget::Own), Err(CastError::WrongTarget));
        g.spells.clear();
        assert_eq!(g.cast(1, CastTarget::Own), Err(CastError::NotInBook));
    }

    #[test]
    fn only_the_first_15_spells_of_the_book_can_be_cast() {
        let mut g = game(HeroClass::Knight);
        g.spells = (20..35).chain([1]).collect();
        assert_eq!(g.spells.len(), 16);
        assert_eq!(g.cast(1, CastTarget::Own), Err(CastError::NotInBook), "the 16th is known but never shown");
        assert!(g.book().iter().all(|s| s.id != 1));
    }

    #[test]
    fn the_mana_is_paid_when_the_spell_lands_and_may_go_below_zero() {
        // 0x4af549: no check, no floor.
        let mut g = game(HeroClass::Knight);
        assert_eq!(g.begin_cast(2, CastTarget::Own), Ok(None));
        g.mana = 100;
        read_out(&mut g);
        assert_eq!(g.mana, -200);
        assert_eq!(g.squad[1].spells[0].map(|s| s.spell), Some(2), "the spell landed");
    }

    #[test]
    fn instant_heal_is_capped_and_wounds_can_kill() {
        let mut g = game(HeroClass::Knight);
        let max = g.squad[1].max_hp(&g.content);
        g.squad[1].hp = max - 10;
        g.squad[2].hp = 0; // a corpse is not raised
        let heal = g.spell(1).unwrap().clone();
        assert_eq!(g.apply_spell_to_army(&heal), 10 + (g.hero().max_hp(&g.content) - g.hero().hp));
        assert_eq!((g.squad[1].hp, g.squad[2].hp), (max, 0));
        // A wound the hero alone does not survive: someone lives, so he is set to 1 HP.
        g.squad[0].hp = 5;
        let nick = SpellDef { delta_fixed_hits: Some(-10), ..heal.clone() };
        g.apply_spell_to_army(&nick);
        assert_eq!((g.squad[0].hp, g.squad[1].hp), (1, max - 10));
        // A percent loss takes a share of the current HP, a gain a share of the maximum.
        let hmax = g.squad[0].max_hp(&g.content);
        g.squad[0].hp = 40;
        let half = SpellDef { delta_fixed_hits: None, delta_percent_hits: Some(-50), ..heal.clone() };
        g.apply_spell_to_army(&half);
        assert_eq!(g.squad[0].hp, 20);
        let tenth = SpellDef { delta_percent_hits: Some(10), ..half };
        g.apply_spell_to_army(&tenth);
        assert_eq!(g.squad[0].hp, 20 + hmax / 10);
        // A wound that kills everyone: no 1 HP for the hero, the game is lost (0x4af658).
        let wound = SpellDef { delta_fixed_hits: Some(-1000), ..heal };
        assert!(matches!(g.spell_on_player(&wound, false), CastOutcome::Done { destroyed: true, .. }));
        assert!(!g.squad[1].alive() && g.squad[1].died_at.is_some(), "a world spell can kill");
        assert_eq!(g.squad[0].hp, 0);
        assert!(g.army_fallen());
    }

    #[test]
    fn the_fallen_leader_of_the_army_hit_gets_1_hp_before_the_rebuild() {
        // 0x4900fc: someone of the army hit lives and its first unit is at 0 HP: it gets 1,
        // then the army is rebuilt and its 1 HP follows its new maximum. A dead hero still
        // holding the lifting gets no instant part from its recast (drain 90, maximum 10).
        let mut g = game(HeroClass::Knight);
        g.squad[0].def = UnitId(14);
        (g.squad[0].drain, g.squad[0].hp) = (90, 0);
        g.squad[0].spells[0] = Some(slot(6, now(&g) + 600));
        let lift = g.spell(6).unwrap().clone();
        g.apply_spell_to_army(&lift);
        assert_eq!((g.squad[0].drain, g.squad[0].max_hp(&g.content), g.squad[0].hp), (90, 10, 1));
        assert!(g.squad[0].spells.iter().all(Option::is_none), "dead when the slots were stripped");
        // A curse on an enemy that leaves a survivor raises the enemy's fallen leader, not the
        // player's fallen hero (the original's bug tested the player's hero whatever army was
        // hit).
        g.squad[0].hp = 0;
        let uid = with_enemy(&mut g, (5, 2), &[troop(4, 0, 2)]);
        g.world.armies[0].ignore_until = f64::MAX;
        let max = crate::rules::ai::troop_max_hp(&g.content, &g.world.armies[0].troops[0]);
        g.world.armies[0].troops[0].hurt = max;
        g.world.armies[0].troops[0].died_at = Some(1);
        let cast = g.cast(3, CastTarget::Army(uid)).unwrap();
        assert!(matches!(cast.outcome, CastOutcome::Done { destroyed: false, .. }), "{cast:?}");
        assert_eq!(g.squad[0].hp, 0);
        assert!(g.world.armies[0].troops[0].alive(), "the enemy's leader at 1 HP");
    }

    #[test]
    fn an_event_casting_while_the_hero_reads_leaves_his_spell_on_its_target() {
        // The original's bug: an event's cast set the target code to 1 (0x4ab1ec), and the
        // hero's spell landed on whatever that code was (0x4af2f8), so his lightning struck
        // his own army for free. Razdor fixes it: it strikes the enemy, paid, and then the
        // event's heal lands.
        use crate::rules::events::EventWorld;
        let mut g = game(HeroClass::Knight);
        let uid = with_enemy(&mut g, (5, 2), &[troop(4, 0, 1)]);
        g.world.armies[0].ignore_until = f64::MAX;
        let max = g.squad[1].max_hp(&g.content);
        g.squad[1].hp = max - 10;
        assert_eq!(g.begin_cast(3, CastTarget::Army(uid)), Ok(None));
        g.apply_spell(1);
        assert_eq!(g.squad[1].hp, max - 10, "the event's heal waits behind the reading");
        let mut events = Vec::new();
        g.end_reading(&mut events);
        assert!(matches!(events[..], [Event::SpellCast { spell: 3, target: CastTarget::Army(_), outcome: CastOutcome::Done { .. } }, Event::EventSpell { spell: 1 }]), "{events:?}");
        assert_eq!(g.squad[1].hp, max, "healed by the event, whose effect is shown");
        assert!(g.mana < 1000 && g.world.armies[0].troops[0].hurt > 0, "paid; the enemy is struck");
        // An own-army spell read meanwhile lands with its own time, paid: 10 h.
        let (t, mana) = (now(&g), g.mana);
        g.begin_cast(2, CastTarget::Own).unwrap();
        g.apply_spell(1);
        g.end_reading(&mut events);
        assert_eq!(g.active_spells(), vec![slot(2, t + 10 * 60)]);
        assert!(g.mana < mana);
    }

    #[test]
    fn a_spell_keeps_the_items_of_the_units_it_kills() {
        let mut g = game(HeroClass::Knight);
        g.squad[1].items[0] = Some(crate::rules::content::ItemId(7));
        g.squad[1].hp = 5;
        let bolt = SpellDef { delta_fixed_hits: Some(-10), ..g.spell(1).unwrap().clone() };
        let pack = g.pack.len();
        g.apply_spell_to_army(&bolt);
        assert!(!g.squad[1].alive());
        assert_eq!((g.squad[1].items[0], g.pack.len()), (Some(crate::rules::content::ItemId(7)), pack), "0x4900fc moves no items");
    }

    #[test]
    fn lasting_spells_expire_after_their_duration() {
        let mut g = game(HeroClass::Knight);
        g.cast(2, CastTarget::Own).unwrap();
        let t = now(&g);
        assert_eq!(g.active_spells(), vec![slot(2, t + 600)]);
        assert!(g.squad.iter().all(|u| u.spells[0] == Some(slot(2, t + 600))), "every unit holds it");
        g.wait(9);
        assert_eq!(g.active_spells().len(), 1);
        g.wait(1);
        assert!(g.active_spells().is_empty(), "10 h later");
        assert!(g.squad.iter().all(|u| u.hp == u.max_hp(&g.content)), "unhurt units stay unhurt as the maximum drops back");
    }

    #[test]
    fn a_life_draining_curse_is_a_lasting_drain_cut_twice() {
        // 0x4900fc: an unhurt 100 HP unit hit by L = −20 drops to 80 HP at once, D = 20, and
        // the rebuild cuts the maximum to 80 and scales the HP: 80 × 80 / 100 = 64.
        let mut g = game(HeroClass::Knight);
        g.squad[0].def = UnitId(14);
        g.squad[0].hp = 100;
        let curse = g.spell(5).unwrap().clone();
        g.apply_spell_to_army(&curse);
        assert_eq!((g.squad[0].drain, g.squad[0].max_hp(&g.content), g.squad[0].hp), (20, 80, 64));
        assert_eq!(g.squad[1].drain, 0, "only the first unit");
        assert!(g.squad[1].spells.iter().all(Option::is_none), "the others get no slot either");
        // Again: D = 100 − 80 × 80 / 100 = 36; HP 64 − 12 = 52, then 64 × 52 / 80 = 41.6.
        g.apply_spell_to_army(&curse);
        assert_eq!((g.squad[0].drain, g.squad[0].max_hp(&g.content), g.squad[0].hp), (36, 64, 41));
        // The lifting lowers D by 20 and nothing else: the curse's slot stays.
        let lift = g.spell(6).unwrap().clone();
        g.apply_spell_to_army(&lift);
        assert_eq!((g.squad[0].drain, g.squad[0].max_hp(&g.content)), (16, 84));
        assert!(g.squad[0].spells.iter().flatten().any(|s| s.spell == 5));
        // D is the unit's, not the slot's: it outlives the curse.
        g.squad[0].spells = [None; SPELL_SLOTS];
        assert_eq!(g.squad[0].max_hp(&g.content), 84);
    }

    #[test]
    fn lasting_effects_apply_to_stats_in_battle() {
        let mut g = game(HeroClass::Knight);
        let uid = with_enemy(&mut g, (12, 2), &[troop(4, 0, 2)]);
        let before = g.squad[1].stats(&g.content);
        g.cast(2, CastTarget::Own).unwrap();
        let s = g.squad[1].stats(&g.content);
        assert_eq!(s[Stat::DefenceBlow], before[Stat::DefenceBlow] + 5);
        assert_eq!(s.max_hp(), before.max_hp() + before.max_hp() * 20 / 100);
        assert_eq!(g.squad[1].hp, s.max_hp(), "unhurt stays unhurt");
        // Weakness on the enemy: walk up to it, cast.
        g.world.armies[0].pos = g.world.map.center((4, 2));
        g.world.armies[0].post = (4, 2); // its post now: it does not walk back while he casts
        g.world.armies[0].ignore_until = f64::MAX; // leaves the hero alone while he casts
        let cast = g.cast(4, CastTarget::Army(uid)).unwrap();
        assert!(matches!(cast.outcome, CastOutcome::Done { .. }), "{cast:?}");
        let enemy_atk = crate::rules::units::Stats::of_level(&g.content, UnitId(4), 1)[Stat::AttackBlow];
        g.foe = Some(Foe::Army(0));
        let b = g.start_battle();
        let mine = b.fighters.iter().find(|f| f.squad_index == Some(1)).unwrap();
        assert_eq!(mine.base[Stat::DefenceBlow], before[Stat::DefenceBlow] + 5);
        assert_eq!((mine.max_hp(), mine.hp), (s.max_hp(), s.max_hp()));
        let theirs = b.fighters.iter().find(|f| f.team == Team::Enemy).unwrap();
        assert_eq!(theirs.stats[Stat::AttackBlow], enemy_atk - enemy_atk * 50 / 100);
        assert_eq!(b.outcome(), Outcome::Ongoing);
    }

    #[test]
    fn any_army_on_explored_ground_is_a_target_at_any_distance() {
        // 0x4cc148: no range and no sight test; unexplored ground, buildings and friends with
        // a meeting waiting are refused.
        let mut g = game(HeroClass::Knight);
        let far = with_enemy(&mut g, (20, 2), &[troop(13, 0, 1)]);
        g.world.armies[0].ignore_until = f64::MAX;
        assert_eq!(g.cast(1, CastTarget::Army(far)), Err(CastError::WrongTarget));
        if !g.fog.explored((20, 2)) {
            assert_eq!(g.cast(3, CastTarget::Army(far)), Err(CastError::NotATarget), "unexplored");
            g.fog.mark((20, 2));
        }
        let cast = g.cast(3, CastTarget::Army(far)).unwrap();
        assert!(matches!(cast.outcome, CastOutcome::Done { destroyed: true, .. }), "18 cells away: {cast:?}");
        // A friend is a target too (no meeting event waits for it).
        let friend = with_enemy(&mut g, (5, 2), &[troop(4, 0, 1)]);
        g.world.armies[0].attitude = 2;
        g.world.armies[0].ignore_until = f64::MAX;
        assert_eq!(g.spell_targets(), vec![0]);
        g.mana = 1000;
        assert!(g.cast(3, CastTarget::Army(friend)).is_ok());
    }

    #[test]
    fn the_target_is_lost_only_when_it_leaves_the_map() {
        let mut g = game(HeroClass::Knight);
        let uid = with_enemy(&mut g, (5, 2), &[troop(4, 0, 1)]);
        g.world.armies[0].ignore_until = f64::MAX;
        assert_eq!(g.begin_cast(3, CastTarget::Army(uid)), Ok(None));
        // It walks far off meanwhile: the spell still lands on it.
        g.world.armies[0].pos = g.world.map.center((22, 5));
        g.world.armies[0].post = (22, 5);
        let events = read_out(&mut g);
        assert!(events.iter().any(|e| matches!(e, Event::SpellCast { outcome: CastOutcome::Done { hits: -15, .. }, .. })), "{events:?}");
        assert_eq!(g.mana, 500);
        // Gone from the map: lost, no mana taken.
        g.fog.mark((22, 5));
        assert_eq!(g.begin_cast(3, CastTarget::Army(uid)), Ok(None));
        let a = g.world.armies.remove(0);
        g.world.inactive.push(a);
        let events = read_out(&mut g);
        assert!(events.iter().any(|e| matches!(e, Event::SpellCast { outcome: CastOutcome::TargetLost, .. })), "{events:?}");
        assert_eq!(g.mana, 500);
    }

    #[test]
    fn enemy_spells_can_kill_and_leave_corpses() {
        let mut g = game(HeroClass::Knight);
        // Two peasants (10 HP) and a warrior: lightning kills the peasants, wounds the other.
        let uid = with_enemy(&mut g, (5, 2), &[troop(13, 0, 2), troop(4, 0, 1)]);
        g.world.armies[0].ignore_until = f64::MAX;
        let cast = g.cast(3, CastTarget::Army(uid)).unwrap();
        // The fallen leader is left at 1 HP, as someone survives.
        assert_eq!(cast.outcome, CastOutcome::Done { hits: -34, killed: 1, destroyed: false });
        let a = &g.world.armies[0];
        let dead: Vec<bool> = a.troops.iter().map(|t| !t.alive()).collect();
        assert_eq!((dead, a.troops[2].hurt), (vec![false, true, false], 15), "the dead stay in the record");
        g.foe = Some(Foe::Army(0));
        let b = g.start_battle();
        let e = b.fighters.iter().find(|f| f.team == Team::Enemy && f.hp > 1).unwrap();
        assert_eq!(e.hp, e.max_hp() - 15, "fights wounded");
        g.foe = None;
        // The warrior has 50 HP: three more bolts end the army, beaten by the player.
        for _ in 0..3 {
            g.mana = 1000;
            g.cast(3, CastTarget::Army(uid)).unwrap();
        }
        assert!(g.world.armies.is_empty());
        assert!(g.beaten_armies.contains(&9));
    }

    #[test]
    fn events_cast_through_the_same_path() {
        use crate::rules::events::EventWorld;
        let mut g = game(HeroClass::Knight);
        g.squad[1].hp = 1;
        let mana = g.mana;
        let t = g.clock;
        g.apply_spell(1);
        assert_eq!((g.squad[1].hp, g.mana, g.clock), (31, mana, t), "free and at once");
        g.apply_spell(2);
        let at = now(&g);
        assert_eq!(g.active_spells(), vec![slot(2, at + 50 * 60)], "5 × TimeWork, TimeWork being 8 or more");
        g.apply_spell(2);
        assert_eq!(g.active_spells(), vec![slot(2, at + 50 * 60)], "a recast by an event resets the end, it does not add");
        // A number past the last spell is held to it: spell 6, the lifting.
        g.squad[0].drain = 30;
        g.apply_spell(200);
        assert_eq!(g.squad[0].drain, 10);
        // An enemy spell cast by an event hits the player's own army, for its normal time.
        g.apply_spell(4);
        assert!(g.squad[1].spells.iter().flatten().any(|s| s.spell == 4 && s.until == at + 8 * 60));
    }

    /// Plays the game in real time until the reading ends; returns every event.
    fn read_out(g: &mut Game) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..1000 {
            events.extend(g.tick(0.05));
            if g.reading().is_none() {
                break;
            }
        }
        events
    }

    #[test]
    fn a_spell_read_on_the_map_lets_the_time_pass_in_real_time() {
        let mut g = game(HeroClass::Knight);
        g.squad[1].hp = 5;
        let start = g.clock.total_minutes();
        assert_eq!(g.begin_cast(1, CastTarget::Own), Ok(None));
        assert_eq!(g.reading(), Some((1, 240.0)));
        assert_eq!((g.mana, g.clock.total_minutes()), (1000, start), "nothing yet");
        // A few frames in: the clock runs, the spell is not there yet.
        g.tick(super::super::game::STEP_SECONDS * 2.5);
        assert!(g.clock.total_minutes() > start && g.reading().is_some() && g.squad[1].hp == 5);
        let events = read_out(&mut g);
        let done = CastOutcome::Done { hits: 30, killed: 0, destroyed: false };
        assert!(events.contains(&Event::SpellCast { spell: 1, target: CastTarget::Own, outcome: done }), "{events:?}");
        assert_eq!((g.mana, g.clock.total_minutes() - start, g.squad[1].hp), (800, 240.0, 35));
        assert!(!g.waiting() && g.reading().is_none());
        // Stopping (a walk, a rest, Space) drops it with no mana spent.
        g.begin_cast(1, CastTarget::Own).unwrap();
        g.stop();
        assert!(read_out(&mut g).is_empty());
        assert_eq!(g.mana, 800);
    }

    #[test]
    fn an_enemy_reaching_the_hero_over_his_book_does_not_attack() {
        // AI armies attack only right after a step of his (world.md §4.3): over his book he
        // is left alone, and the spell lands.
        let mut g = game(HeroClass::Knight);
        with_enemy(&mut g, (6, 2), &[troop(4, 0, 1)]);
        g.world.armies.last_mut().unwrap().ai.aggression = 100;
        assert_eq!(g.begin_cast(2, CastTarget::Own), Ok(None));
        let events = read_out(&mut g);
        assert!(!events.iter().any(|e| matches!(e, Event::Encounter(_))), "{events:?}");
        assert!(events.iter().any(|e| matches!(e, Event::SpellCast { outcome: CastOutcome::Done { .. }, .. })), "{events:?}");
        assert!(g.foe.is_none() && g.reading().is_none());
    }

    #[test]
    fn an_enemy_reaching_the_hero_does_not_interrupt_the_cast() {
        let mut g = game(HeroClass::Knight);
        with_enemy(&mut g, (6, 2), &[troop(4, 0, 1)]);
        // Bold enough to attack a stronger hero (the AI attacks only battles it wins), and
        // free to go for him.
        let a = g.world.armies.last_mut().unwrap();
        a.ai.aggression = 100;
        a.ai.no_random = true;
        a.patrols = false;
        // Gold for its wages: its noon marks its pairs dirty, and an unpaid crew scores no
        // battle (ai.md §4).
        a.gold = 500;
        let mana = g.mana;
        let cast = g.cast(2, CastTarget::Own).unwrap();
        assert!(matches!(cast.outcome, CastOutcome::Done { .. }), "{:?}", cast.outcome);
        assert!(!cast.events.iter().any(|e| matches!(e, Event::Encounter(_))));
        assert!(g.mana < mana, "the spell completed and was paid");
        let a = g.world.armies.last().unwrap().tile(&g.world.map);
        assert!((a.0 - g.tile().0).abs() <= 1 && (a.1 - g.tile().1).abs() <= 1, "it came next to him: {a:?}");
    }

    #[test]
    fn recasting_adds_time_and_a_unit_holds_four_spells() {
        let mut g = game(HeroClass::Knight);
        let armour = g.spell(2).unwrap().clone();
        let t = now(&g);
        g.apply_spell_to_army(&armour);
        g.apply_spell_to_army(&armour);
        assert_eq!(g.active_spells(), vec![slot(2, t + 2 * 600)], "another 10 h on top");
        for id in 20..25 {
            let s = SpellDef { id, ..armour.clone() };
            g.apply_spell_to_army(&s);
        }
        assert_eq!(g.active_spells().iter().map(|e| e.spell).collect::<Vec<_>>(), [2, 20, 21, 22], "no free slot: nothing happens");
        // Not even the instant part: a heal finds no slot on these units.
        g.squad[1].hp = 5;
        let heal = SpellDef { id: 30, delta_fixed_hits: Some(30), ..armour.clone() };
        g.apply_spell_to_army(&heal);
        assert_eq!(g.squad[1].hp, 5);
        // A unit with a free slot is healed: each unit has its own four.
        g.squad[2].spells[3] = None;
        g.squad[2].hp = 5;
        g.apply_spell_to_army(&heal);
        assert_eq!((g.squad[1].hp, g.squad[2].hp), (5, 35));
    }

    #[test]
    fn a_recast_leaves_a_dead_unit_that_still_holds_the_spell_dead() {
        // The original's bug: 0x4900fc's recast branch had no HP test, so this raised it.
        let mut g = game(HeroClass::Knight);
        let mend = SpellDef { id: 2, delta_fixed_hits: Some(20), ..g.spell(2).unwrap().clone() };
        g.apply_spell_to_army(&mend);
        g.squad[1].hp = 0; // died some other way, its slot kept
        g.squad[1].died_at = Some(1);
        g.apply_spell_to_army(&mend);
        assert_eq!((g.squad[1].hp, g.squad[1].died_at), (0, Some(1)));
        // A dead unit with a new slot is not raised, and the dead lose their slots.
        g.squad[2].hp = 0;
        g.squad[2].spells = [None; SPELL_SLOTS];
        g.apply_spell_to_army(&mend);
        assert_eq!((g.squad[2].hp, g.squad[2].spells), (0, [None; SPELL_SLOTS]));
    }

    #[test]
    fn one_enemy_spells_hit_only_the_leader() {
        let mut g = game(HeroClass::Knight);
        let bolt = SpellDef { delta_fixed_hits: Some(-15), target: Some(SpellTarget::OneEnemy), ..spell(7, 100, 1, None) };
        let mut spells = g.content.spells.clone();
        spells.push(bolt);
        let c = &g.content;
        g.content = Arc::new(Content::new(c.units.clone(), c.items.clone(), spells, c.options.clone(), Formation::WIDE));
        g.spells.push(7);
        let uid = with_enemy(&mut g, (5, 2), &[troop(4, 0, 3)]);
        g.world.armies[0].ignore_until = f64::MAX;
        let cast = g.cast(7, CastTarget::Army(uid)).unwrap();
        assert_eq!(cast.outcome, CastOutcome::Done { hits: -15, killed: 0, destroyed: false });
        let hurt: Vec<i32> = g.world.armies[0].troops.iter().map(|t| t.hurt).collect();
        assert_eq!(hurt, [15, 0, 0]);
    }

    #[test]
    fn old_army_wide_spells_go_into_the_units_slots() {
        let c = content();
        let at = crate::rules::formation::Slot::new(crate::rules::formation::Row::Front, 0);
        let mut units = vec![Unit::new(&c, UnitId(4), at), Unit::new(&c, UnitId(4), at)];
        let old = [ActiveSpell { spell: 2, until: Some(500), leader: false }, ActiveSpell { spell: 5, until: None, leader: true }];
        migrate_old_spells(&old, &mut units, 9000);
        assert_eq!(units[0].spells[..2], [Some(slot(2, 500)), Some(slot(5, 9000))]);
        assert_eq!(units[1].spells[..2], [Some(slot(2, 500)), None]);
    }
}
