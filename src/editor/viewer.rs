//! The original editor's AI viewer (TViewAI, docs/reference/editor/testers.md §3): the world
//! AI run on the edited map with no game around it, in 6-minute steps, with a hero the AI
//! steers too or none, a cut-down event engine, two subjects to inspect, the AI's overlays
//! and predicted battles. It runs Razdor's world AI as the editor's copy
//! ([`crate::rules::ai::EditorAi`]) on a [`Game`] of the map; the window is
//! `ui::editor::viewer`.

use std::sync::Arc;

use crate::dt::dtm::{Archetype, Army as DtArmy, Event, Scenario};
use crate::rules::ai::{self, EditorAi, SimResult, EDITOR_STEP, EDITOR_TICK};
use crate::rules::battle::Switches;
use crate::rules::content::{Content, HeroClass, ItemId, UnitId, WageKind};
use crate::rules::events::EventWorld;
use crate::rules::game::Game;
use crate::rules::rng::Rng;
use crate::rules::world::{Army, LocationKind, Owner, Troop, Walk};

use super::tester::{self, Placed};

/// The hero's army record: uid out of the scenario's range (its id is 0, the hero's).
pub const HERO_UID: u32 = u32::MAX - 1;
/// An event fired less than this long ago waits (unless it has a meet army).
const REFIRE_MINUTES: u64 = 60;
/// The panels refresh this often while the clock runs (game minutes).
pub const RUN_REFRESH: u64 = 180;
/// The viewer hero's speed, whatever the class (0x592cc7; the game's 5, 5, 4).
const HERO_SPEED: u32 = 4;
/// The viewer hero's aggression (0x592aac).
const HERO_AGGRESSION: i8 = -20;
/// The viewer hero's garrison level byte (0x592aac sets 50).
const HERO_GARRISON_LEVEL: u8 = 50;

/// Why the clock stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerStop {
    /// The scenario's victory or defeat event fired.
    Victory,
    Defeat,
    /// A repeating event with a period under a day: the original divides by zero there.
    RepeatUnderADay { event: u16 },
    /// An event kept firing within one scan (a meet event bypasses the 60-minute rule): the
    /// original's scan calls itself without end.
    EventLoop { event: u16 },
}

/// What the viewer keeps of an event (the copy 0xf7ec98).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct EvState {
    fired: u32,
    /// Minute of the last firing (0 never).
    last: u64,
    /// The relative start a "relative event" result gave it.
    start: Option<u64>,
    answer: u8,
}

/// A subject to inspect: the hero entry, an army or a building.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subject {
    Hero,
    Army(usize),
    Building(usize),
}

pub struct Viewer {
    pub game: Game,
    scenario: Scenario,
    /// The hero's class, if one was picked.
    pub hero: Option<HeroClass>,
    /// Time has run: the hero can no longer be picked (0x58f220).
    pub ran: bool,
    /// The events box: the event scan runs on the 24-minute ticks.
    pub events_on: bool,
    states: Vec<EvState>,
    /// The viewer's flag string (entries end in a non-breaking space).
    flags: Vec<u8>,
    /// The army (id) that met the hero last (0x104f924, never cleared).
    met: Option<u8>,
    /// Messages of the events fired, oldest first.
    pub messages: Vec<String>,
    pub stop: Option<ViewerStop>,
    /// The map's start time.
    pub start: u64,
}

const NBSP: u8 = 0xA0;

impl Viewer {
    /// Opens the viewer on `scenario` (0x58cc84): the random generator set to 1, so a run can
    /// be repeated, or from the clock when `rnd` brings a seed; the armies and buildings from
    /// the editor's records, the events copied, no hero.
    pub fn open(content: Arc<Content>, scenario: &Scenario, switches: Switches, rnd: Option<u32>) -> Viewer {
        let mut game = Game::for_editor(tester::on_editor_grid(content), scenario, EditorAi { switches, hero: None });
        game.rng = Rng::new(rnd.unwrap_or(1));
        let start = game.clock.total_minutes() as u64;
        Viewer {
            game,
            scenario: scenario.clone(),
            hero: None,
            ran: false,
            events_on: true,
            states: vec![EvState::default(); scenario.events.len()],
            flags: Vec::new(),
            met: None,
            messages: Vec::new(),
            stop: None,
            start,
        }
    }

    pub fn now(&self) -> u64 {
        self.game.clock.total_minutes() as u64
    }

    /// Minutes since the map's start.
    pub fn elapsed(&self) -> u64 {
        self.now().saturating_sub(self.start)
    }

    /// The hero classes offered: those whose start preset has a position (x and y not 0).
    pub fn offered(&self, class: HeroClass) -> bool {
        let p = self.scenario.header.hero(archetype(class));
        p.x != 0 && p.y != 0
    }

    /// The hero choice (0x592aac), only before time has run: none, or class `c`, whose army
    /// is built from the preset — the class leader and the preset's troops (types above 3)
    /// at their levels, its position and gold, faction 1 with the header's attitudes,
    /// aggression −20, speed 4, the preset's three items worn by the hero, no patrol — and
    /// whose start building and every building flagged for the class become the hero's.
    pub fn set_hero(&mut self, class: Option<HeroClass>) -> bool {
        if self.ran || class.is_some_and(|c| !self.offered(c)) {
            return false;
        }
        let g = &mut self.game;
        if let Some(k) = g.world.armies.iter().position(|a| a.uid == HERO_UID) {
            g.world.armies.remove(k);
        }
        self.hero = class;
        let mut editor = g.editor_ai.expect("the viewer's game");
        editor.hero = class.map(|_| HERO_UID);
        g.set_editor_ai(Some(editor));
        if let Some(c) = class {
            let army = hero_army(g, &self.scenario, c);
            for l in g.world.start_buildings(&self.scenario, c) {
                g.world.give_to_player(l);
            }
            g.world.armies.insert(0, army);
            g.ai_init(false);
        }
        true
    }

    /// The hero's army, if any.
    pub fn hero_index(&self) -> Option<usize> {
        self.game.world.armies.iter().position(|a| a.uid == HERO_UID)
    }

    /// One 6-minute step (0x58eaf0): on a 24-minute tick the event scan first (events box
    /// on), then the AI's step. Nothing once the clock has stopped.
    pub fn step(&mut self) {
        if self.stop.is_some() {
            return;
        }
        self.ran = true;
        if self.events_on && self.now().is_multiple_of(EDITOR_TICK) {
            self.scan_events();
            if self.stop.is_some() {
                return;
            }
        }
        // The original's global is only ever written by a meeting: the last army to have met
        // the hero stays "met" for every later scan until another one meets him.
        let met = self.game.editor_step();
        if let Some(id) = met.filter(|&u| u != HERO_UID).and_then(|u| self.game.world.armies.iter().find(|a| a.uid == u)).map(|a| a.id) {
            self.met = Some(id);
        }
    }

    /// `n` steps (the buttons: 1 = 6 minutes, 10 = 1 hour, 60 = 6 hours), stopping with the
    /// clock.
    pub fn steps(&mut self, n: u32) {
        for _ in 0..n {
            if self.stop.is_some() {
                break;
            }
            self.step();
        }
    }

    // --------------------------------------------------------------------------------------
    // The cut-down event engine (0x58d70c–0x58e988)
    // --------------------------------------------------------------------------------------

    /// The scan (0x58e988): the first global event that passes fires; the original then scans
    /// again from its end (unless it chained or ended the game).
    fn scan_events(&mut self) {
        let mut fired_now: Vec<usize> = Vec::new();
        loop {
            let mut pick = None;
            for k in 0..self.scenario.events.len() {
                if self.scenario.events[k].kind != 1 {
                    continue;
                }
                match self.passes(k) {
                    Err(stop) => {
                        self.stop = Some(stop);
                        return;
                    }
                    Ok(true) => {
                        pick = Some(k);
                        break;
                    }
                    Ok(false) => {}
                }
            }
            let Some(k) = pick else { return };
            if fired_now.contains(&k) {
                self.stop = Some(ViewerStop::EventLoop { event: k as u16 + 1 });
                return;
            }
            fired_now.push(k);
            if !self.execute(k, &mut fired_now) {
                return;
            }
        }
    }

    /// The conditions (0x58deac), in the original's order. `Err` where the original divides
    /// by zero.
    fn passes(&mut self, k: usize) -> Result<bool, ViewerStop> {
        let e = &self.scenario.events[k];
        let st = self.states[k];
        let now = self.now() as i64;
        let c = &e.conditions;
        // A subordinate event (its "done" byte) fires only through a chain.
        if e.subordinate != 0 {
            return Ok(false);
        }
        if c.meet_army == 0 && st.last as i64 + REFIRE_MINUTES as i64 >= now {
            return Ok(false);
        }
        // A start before 0 counts from 0.
        let start = match st.start {
            Some(s) => s as i64,
            None => e.start_time as i32 as i64,
        }
        .max(0);
        if start > now {
            return Ok(false);
        }
        let duration = if e.duration == 0 { 1 } else { e.duration as i64 };
        // The window opens on the start's day, or every `repeat div 1440` days from it, at the
        // start's minute of the day, for `duration` × 60 minutes.
        let days = (now - start) / 1440;
        let opened = start + days * 1440;
        if e.repeat != 0 {
            // The period in days: under a day it is 0 and the original divides by it.
            let period = e.repeat as i64 / 1440;
            if period == 0 {
                return Err(ViewerStop::RepeatUnderADay { event: k as u16 + 1 });
            }
            if days % period != 0 || now > opened + duration * 60 {
                return Ok(false);
            }
        } else if now > start + duration * 60 {
            return Ok(false);
        }
        if e.once != 0 && st.fired != 0 {
            return Ok(false);
        }
        let class = self.hero.map_or(0, |h| HeroClass::ALL.iter().position(|&x| x == h).map_or(0, |i| i as u8 + 1));
        if e.archetype != 0 && e.archetype != class {
            return Ok(false);
        }
        let hero = self.hero_index().map(|i| &self.game.world.armies[i]);
        // The hero's figures: with no hero the check fails.
        if c.stats_check != 0 {
            let Some(h) = hero else { return Ok(false) };
            let level = h.troops.first().map_or(0, |t| t.level as i64 - 1);
            let strength: i64 = h.troops.iter().map(|t| ai::tactical_modes(&self.game.content, t, 0).1 as i64).sum();
            let ok = threshold(c.level, level) && threshold(c.gold, h.gold as i64) && threshold(c.squad_count, h.troops.len() as i64) && threshold(c.army_strength, strength);
            if !ok {
                return Ok(false);
            }
        }
        let g = &self.game;
        if c.buildings_check != 0 {
            for (&b, &code) in c.buildings.iter().zip(&c.buildings_owner) {
                if b == 0 || code == 0 {
                    continue;
                }
                let Some(l) = g.world.locations.iter().find(|l| l.id == b as u16) else { return Ok(false) };
                let mine = matches!(l.owner, Owner::Player | Owner::Army(0));
                // Codes 1 and 6 also pass on a building of faction 0 or 5.
                let ok = (code == 1 && mine) || (code == 6 && !mine) || l.faction == code.wrapping_sub(1);
                if !ok {
                    return Ok(false);
                }
            }
        }
        if c.units_check != 0 && !self.units_hold(c) {
            return Ok(false);
        }
        if c.artifacts_check != 0 {
            for (&item, &code) in c.artifacts.iter().zip(&c.artifacts_owner) {
                let worn = |a: &Army| a.faction == code.wrapping_sub(1) && a.troops.iter().any(|t| t.worn.contains(&Some(ItemId(item as u32))));
                if item != 0 && !self.records().any(worn) {
                    return Ok(false);
                }
            }
        }
        let fired = |id: u16| self.states.get((id as usize).wrapping_sub(1)).is_some_and(|s| s.fired > 0);
        let answer = |id: u16| self.states.get((id as usize).wrapping_sub(1)).map_or(0, |s| s.answer);
        if c.happened_yes_check != 0 && !c.happened_yes.iter().all(|&id| id == 0 || (fired(id) && answer(id) != 1)) {
            return Ok(false);
        }
        if c.not_happened_check != 0 && !c.not_happened.iter().all(|&id| id == 0 || !fired(id)) {
            return Ok(false);
        }
        if c.defeated_check != 0 && !c.defeated_armies.iter().all(|&a| a == 0 || g.player_defeated(a)) {
            return Ok(false);
        }
        if c.army_active != 0 && !g.army_active(c.army_active) {
            return Ok(false);
        }
        if c.army_inactive != 0 && !g.army_inactive(c.army_inactive) {
            return Ok(false);
        }
        if c.beaten_check != 0 && !c.beaten_armies.iter().all(|&a| a == 0 || g.army_beaten(a)) {
            return Ok(false);
        }
        if c.meet_army != 0 {
            // The army's met flag is set as soon as everything before passed, meeting or not;
            // the event passes only while that army is the last to have met the hero
            // (0x58e83a).
            self.game.met_armies.insert(c.meet_army);
            if self.met != Some(c.meet_army) {
                return Ok(false);
            }
        }
        // "Happened, answer no" reads only the answer, which the viewer never sets.
        if c.happened_no_check != 0 && !c.happened_no.iter().all(|&id| id == 0 || answer(id) != 0) {
            return Ok(false);
        }
        if let Some(test) = flag_parts(&self.scenario.events[k]).1 {
            if !self.flag_test(test) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Every army record but the hero's (the original's armies 1 on): on the map, waiting
    /// off it, or waiting to come back.
    fn records(&self) -> impl Iterator<Item = &Army> {
        let w = &self.game.world;
        w.armies.iter().chain(&w.inactive).chain(w.respawns.iter().map(|r| &r.army)).filter(|a| a.uid != HERO_UID)
    }

    /// The named-squad condition (0x58deac): each slot looks for its own unit, the units
    /// taken noted in a list of three places (a full list overwrites the third). Codes 1 and
    /// 6 search the hero's army for a living unit of the slot's type when the slot has no
    /// name, or carrying the slot's name, or any unnamed unit an event brought (whatever the
    /// slot's type); code 1 needs one, 6 none. Codes 2–5 search the armies of faction
    /// code − 1 for a unit of the slot's type carrying exactly the slot's name, dead or alive.
    /// Code 0 fails.
    fn units_hold(&self, c: &crate::dt::dtm::EventConditions) -> bool {
        let named = |a: &Army, j: usize| if j == 0 { a.named } else { 0 };
        let mut used: [Option<(usize, usize)>; 3] = [None; 3];
        let note = |used: &mut [Option<(usize, usize)>; 3], at: (usize, usize)| {
            let k = used.iter().position(Option::is_none).unwrap_or(2);
            used[k] = Some(at);
        };
        let hero = self.hero_index().map(|i| &self.game.world.armies[i]);
        for k in 0..3 {
            let (t, name, code) = (c.units[k], c.units_named[k], c.units_owner[k]);
            if t == 0 {
                continue;
            }
            let ok = match code {
                1 | 6 => {
                    let fits = |h: &Army, j: usize| {
                        let (u, n) = (&h.troops[j], named(h, j));
                        u.alive() && ((u.unit == UnitId(t as u32) && name == 0) || (n > 0 && n == name) || (u.kind == WageKind::Event && n == 0))
                    };
                    let found = hero.and_then(|h| (0..h.troops.len()).find(|&j| !used.contains(&Some((0, j))) && fits(h, j)));
                    if let Some(j) = found {
                        note(&mut used, (0, j));
                    }
                    (code == 1) == found.is_some()
                }
                2..=5 => {
                    let mut any = false;
                    for (a, army) in self.records().enumerate().filter(|(_, a)| a.faction == code - 1) {
                        let fits = |j: usize| army.troops[j].unit == UnitId(t as u32) && named(army, j) == name;
                        if let Some(j) = (0..army.troops.len()).find(|&j| !used.contains(&Some((a + 1, j))) && fits(j)) {
                            note(&mut used, (a + 1, j));
                            any = true;
                        }
                    }
                    any
                }
                _ => false,
            };
            if !ok {
                return false;
            }
        }
        true
    }

    /// The results (0x58d9d8): the message, the flag script, armies activated and
    /// deactivated, artefacts lost, a relative event's start, the chain. Nothing else (no
    /// XP, gold, mana, units, spells, battles, lanterns, moves …). False when the scan ends
    /// here (a victory or defeat stops the clock; a chain fires its event instead).
    fn execute(&mut self, k: usize, fired_now: &mut Vec<usize>) -> bool {
        let now = self.now();
        let e = self.scenario.events[k].clone();
        self.states[k].last = now;
        self.states[k].fired += 1;
        if !e.message.trim().is_empty() {
            self.messages.push(e.message.clone());
        }
        if let Some(action) = flag_parts(&e).0 {
            self.flag_action(action);
        }
        let r = &e.results;
        for &a in r.activate_armies.iter().filter(|&&a| a != 0) {
            self.game.activate_army(a);
        }
        if r.deactivate_army != 0 {
            self.game.deactivate_army(r.deactivate_army);
        }
        for &item in r.artifacts_remove.iter().filter(|&&i| i != 0) {
            self.hero_loses(ItemId(item as u32));
        }
        if let Some(st) = (r.relative_event as usize).checked_sub(1).and_then(|j| self.states.get_mut(j)) {
            st.start = Some(now + 60 * r.relative_delay_hours as u64);
        }
        self.states[k].answer = 0;
        let h = &self.scenario.header;
        if k + 1 == h.victory_event as usize {
            self.stop = Some(ViewerStop::Victory);
            return false;
        }
        if k + 1 == h.defeat_event as usize {
            self.stop = Some(ViewerStop::Defeat);
            return false;
        }
        if let Some(j) = (r.chained_event as usize).checked_sub(1).filter(|&j| j < self.states.len()) {
            if fired_now.contains(&j) {
                self.stop = Some(ViewerStop::EventLoop { event: j as u16 + 1 });
                return false;
            }
            fired_now.push(j);
            self.states[j].answer = 0;
            return self.execute(j, fired_now);
        }
        true
    }

    /// An artefact lost (0x58d91c): out of the hero's pack, else off the first unit of his
    /// army wearing it.
    fn hero_loses(&mut self, item: ItemId) {
        let Some(i) = self.hero_index() else { return };
        let a = &mut self.game.world.armies[i];
        if let Some(k) = a.items.iter().position(|&x| x == item) {
            a.items.remove(k);
            return;
        }
        if let Some(slot) = a.troops.iter_mut().flat_map(|t| t.worn.iter_mut()).find(|s| **s == Some(item)) {
            *slot = None;
        }
    }

    /// The flag test: with a `/` anywhere in it, that `/` taken out, it holds when the rest
    /// does not occur in the flag string; without one, when the test does. No `^` stripping
    /// and no tutorial keyword, unlike the game.
    fn flag_test(&self, test: &str) -> bool {
        let mut s = crate::dt::text::encode(test);
        if s.is_empty() {
            return true;
        }
        match s.iter().position(|&b| b == b'/') {
            Some(k) => {
                s.remove(k);
                find(&self.flags, &s).is_none()
            }
            None => find(&self.flags, &s).is_some(),
        }
    }

    /// The flag script (as the game's 0x4ab2a3): `+X` adds X, `-X` removes it, `+X^` adds X1
    /// or raises its counter, `-X^` lowers it and removes the entry at 0.
    fn flag_action(&mut self, action: &str) {
        let a = crate::dt::text::encode(action);
        if a.len() <= 2 {
            return;
        }
        let counter = a.contains(&b'^');
        let mut s = a[1..].to_vec();
        if counter {
            s.pop();
        }
        let p = find(&self.flags, &s);
        match (a[0], counter, p) {
            (b'+', true, None) => {
                self.flags.extend_from_slice(&s);
                self.flags.extend([b'1', NBSP]);
            }
            (b'+', true, Some(p)) => {
                if let Some(c) = self.flags.get_mut(p + s.len()) {
                    *c = c.wrapping_add(1);
                }
            }
            (b'+', false, None) => {
                self.flags.extend_from_slice(&s);
                self.flags.push(NBSP);
            }
            // Not there: the original decrements the byte at the name's length less 1 from
            // the string's start, and its removal at position 0 deletes nothing (its bug, kept;
            // past the string's end it writes outside it, which Razdor skips).
            (b'-', true, None) => {
                if let Some(c) = self.flags.get_mut(s.len() - 1) {
                    *c = c.wrapping_sub(1);
                }
            }
            (b'-', true, Some(p)) => {
                if let Some(c) = self.flags.get_mut(p + s.len()) {
                    *c = c.wrapping_sub(1);
                    if *c == b'0' {
                        let end = (p + s.len() + 2).min(self.flags.len());
                        self.flags.drain(p..end);
                    }
                }
            }
            (b'-', false, Some(p)) => {
                let end = (p + s.len() + 1).min(self.flags.len());
                self.flags.drain(p..end);
            }
            _ => {}
        }
    }

    /// The flag string's names.
    pub fn flags(&self) -> Vec<String> {
        self.flags.split(|&b| b == NBSP).filter(|n| !n.is_empty()).map(crate::dt::text::decode).collect()
    }

    // --------------------------------------------------------------------------------------
    // Subjects, overlays, battles
    // --------------------------------------------------------------------------------------

    /// The first list: the hero entry, then the armies on the map.
    pub fn first_list(&self) -> Vec<Subject> {
        std::iter::once(Subject::Hero).chain(self.armies().map(Subject::Army)).collect()
    }

    /// The second list: the armies, then the towns, castles, forts and ruins (types 1, 3, 4
    /// and 12, in that order).
    pub fn second_list(&self) -> Vec<Subject> {
        let w = &self.game.world;
        let kinds = [LocationKind::Town, LocationKind::Castle, LocationKind::Fort, LocationKind::Ruins];
        let buildings = kinds.into_iter().flat_map(|k| (0..w.locations.len()).filter(move |&l| w.locations[l].kind == k));
        self.armies().map(Subject::Army).chain(buildings.map(Subject::Building)).collect()
    }

    fn armies(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.game.world.armies.len()).filter(|&i| self.game.world.armies[i].uid != HERO_UID && ai::managed(&self.game.world.armies[i]))
    }

    /// The army of a subject (the hero entry is the hero's army).
    pub fn army_of(&self, s: Subject) -> Option<usize> {
        match s {
            Subject::Hero => self.hero_index(),
            Subject::Army(i) => (i < self.game.world.armies.len()).then_some(i),
            Subject::Building(_) => None,
        }
    }

    /// The daily income totals of the towns, of the castles and forts, and of the villages.
    pub fn incomes(&self) -> [i32; 3] {
        let sum = |f: &dyn Fn(LocationKind) -> bool| self.game.world.locations.iter().filter(|l| f(l.kind)).map(|l| l.gold_income).sum();
        [sum(&|k| k == LocationKind::Town), sum(&|k| matches!(k, LocationKind::Castle | LocationKind::Fort)), sum(&|k| k == LocationKind::Village)]
    }

    /// Army `i`'s plan maps, planning it now (0x583c6c): its density map and its flood
    /// distances.
    pub fn plan(&mut self, i: usize) -> Option<(Vec<u16>, Vec<u16>)> {
        self.game.editor_replan(i);
        let uid = self.game.world.armies.get(i)?.uid;
        self.game.editor_plan.as_ref().filter(|p| p.0 == uid).map(|p| (p.1.clone(), p.2.clone()))
    }

    /// The two predicted battles of a pair of armies (0x58f930): `a` attacking `b`, and `b`
    /// attacking `a`.
    pub fn predictions(&self, a: usize, b: usize) -> [SimResult; 2] {
        [self.game.editor_predict(a, b), self.game.editor_predict(b, a)]
    }

    /// The cached scores between army `a` and subject `b`: army–army and the talk counter,
    /// or army–building.
    pub fn scores(&self, a: usize, b: Subject) -> Option<(i32, i32)> {
        match b {
            Subject::Building(l) => Some((self.game.editor_building_score(a, l), 0)),
            _ => {
                let j = self.army_of(b)?;
                Some(self.game.editor_scores(a, self.game.world.armies[j].uid))
            }
        }
    }

    /// The debug button (0x592f00): an army–army score computed and thrown away.
    pub fn test_score(&self, a: usize, b: usize) -> i32 {
        self.game.editor_test_score(a, b)
    }

    /// The tester's armies of the battle buttons (0x591278): `side1` as built for battle at
    /// less than full strength (its unpaid units weakened), `side2` at full; each with its
    /// units' tactical costs, its building defence, auto-arranged.
    pub fn battle_armies(&self, side1: usize, side2: usize) -> (tester::Army, tester::Army) {
        (self.tester_army(side1, true), self.tester_army(side2, false))
    }

    fn tester_army(&self, i: usize, weak: bool) -> tester::Army {
        let g = &self.game;
        let c = &g.content;
        let a = &g.world.armies[i];
        let defence = a.mind.defence;
        let units: Vec<Placed> = a
            .troops
            .iter()
            .filter(|t| t.alive())
            .map(|t| {
                let unit = crate::rules::game::troop_unit(c, t);
                let value = ai::tactical_modes(c, t, defence).1;
                Placed { weak: weak && t.unpaid, unit, value }
            })
            .collect();
        tester::arrange(c, units, defence)
    }
}

/// The class's preset.
fn archetype(c: HeroClass) -> Archetype {
    match c {
        HeroClass::Knight => Archetype::Knight,
        HeroClass::Archmage => Archetype::Archmage,
        HeroClass::Ranger => Archetype::Ranger,
    }
}

/// The viewer hero's army record (0x592aac).
fn hero_army(g: &Game, s: &Scenario, c: HeroClass) -> Army {
    let content = &g.content;
    let start = g.world.hero_start(s, content, c);
    let mut leader = Troop::new(c.unit(), 1, start.hero_slot);
    leader.kind = WageKind::Leader;
    // The preset's three items are worn by the hero, not packed as in the game.
    for (slot, &item) in leader.worn.iter_mut().zip(start.items.iter().take(3)) {
        *slot = Some(item);
    }
    let mut troops = vec![leader];
    troops.extend(start.troops.into_iter().filter(|t| t.unit.0 > 3));
    let now = g.clock.total_minutes() as u64;
    troops.iter_mut().for_each(|t| t.last_paid = now);
    let p = s.header.hero(archetype(c));
    let dt = DtArmy { faction: 1, relations: s.header.relations[0], aggression: HERO_AGGRESSION, garrison_strength: HERO_GARRISON_LEVEL, ..DtArmy::default() };
    let tile = start.tile;
    Army {
        id: 0,
        uid: HERO_UID,
        name: content.unit(c.unit()).name.clone(),
        leader_name: String::new(),
        description: String::new(),
        model: 0,
        pos: g.world.map.center(tile),
        home: (p.start_building as usize).checked_sub(1).filter(|&l| l < g.world.locations.len()),
        post: tile,
        box_centre: None,
        patrols: false,
        patrol_radius: 0,
        troops,
        faction: 1,
        attitude: 0,
        gold: start.gold,
        items: Vec::new(),
        speed: HERO_SPEED,
        budget: 0.0,
        walk: Walk::default(),
        path: Vec::new(),
        chasing: false,
        ignore_until: 0.0,
        talk: 0,
        arrived: false,
        rest_until: 0.0,
        named: 0,
        old_effects: Vec::new(),
        ship: if g.world.is_sea(tile) { crate::rules::ships::kind::HERO } else { 0 },
        ai: ai::AiProfile::from_dt(&dt),
        mind: Default::default(),
    }
}

/// The original's signed threshold (0x58de6c): 0 passes; a positive `v` wants at least `v`,
/// a negative one at most `−v`.
pub fn threshold(v: i16, x: i64) -> bool {
    match v {
        0 => true,
        1.. => x >= v as i64,
        _ => x <= -(v as i64),
    }
}

/// The title script's action (before `=`) and test (after it), in the text between the
/// first `%` and a second one (0x4dc938).
fn flag_parts(e: &Event) -> (Option<&str>, Option<&str>) {
    let Some(f) = &e.flags else { return (None, None) };
    let script = f.raw.split('%').next().unwrap_or_default();
    match script.split_once('=') {
        Some((a, t)) => ((!a.is_empty()).then_some(a), Some(t)),
        None => ((!script.is_empty()).then_some(script), None),
    }
}

/// The first position of `needle` in `hay` (a substring search, as the original's `Pos`).
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The 9 × 9 values around cell (x, y) of a `w` × `h` map (0x592144), row by row. The
/// window x−4..x+4, y−4..y+4 is cut at the map's right and bottom edges (not moved inside)
/// and its start at the left edge, but when it starts above the top edge the original zeroes
/// its column start instead of its row start (its bug, kept): the rows above the map show 0,
/// as every cell off the map reads, and the columns from 0 on. A cell the window does not
/// reach is `None` (the original's grid keeps the last click's text there).
pub fn grid9(values: &[u16], w: i32, h: i32, x: i32, y: i32) -> [[Option<u16>; 9]; 9] {
    let (mut x0, y0) = (x - 4, y - 4);
    if x0 < 0 {
        x0 = 0;
    }
    if y0 < 0 {
        x0 = 0;
    }
    let (x1, y1) = ((x + 4).min(w - 1), (y + 4).min(h - 1));
    let mut out = [[None; 9]; 9];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            let (cx, cy) = (x0 + c as i32, y0 + r as i32);
            if cx <= x1 && cy <= y1 {
                let on_map = (0..w).contains(&cx) && (0..h).contains(&cy);
                *v = Some(if on_map { values.get((cy * w + cx) as usize).copied().unwrap_or(0) } else { 0 });
            }
        }
    }
    out
}

/// "HHh DD.MM.YYYY" of game minute `t` (0x593fb0).
pub fn format_time(t: u64) -> String {
    format!("{:02}h {:02}.{:02}.{:04}", (t / 60) % 24, (t / 1440) % 30 + 1, (t / 43_200) % 12 + 1, t / 518_400)
}

/// The step buttons' step counts: 6 minutes, 1 hour, 6 hours.
pub const STEPS: [u32; 3] = [1, 10, 60];
const _: () = assert!(EDITOR_STEP * 10 == 60);

#[cfg(test)]
mod tests;
