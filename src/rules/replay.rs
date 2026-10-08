//! Headless replays of the player's scenarios (tests only; skipped without `RAZDOR_DT_DIR`).
//! Numbers only: texts are read at runtime and never asserted.

use crate::dt::dtm::Scenario;
use crate::dt::install::DtInstall;

fn install() -> Option<DtInstall> {
    let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
    Some(DtInstall::load(std::path::Path::new(&dir)).expect("install loads"))
}

/// A `Debug` dump with the zero fields left out.
fn nonzero_debug(s: &str) -> String {
    let body = s.trim_start_matches(|c: char| c != '{').trim_matches(|c| c == '{' || c == '}' || c == ' ');
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for ch in body.chars() {
        match ch {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth -= 1,
            _ => {}
        }
        if ch == ',' && depth == 0 {
            out.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    out.push(cur.trim().to_string());
    out.retain(|f| {
        let v = f.split_once(": ").map_or("", |x| x.1);
        !(v == "0" || v.chars().all(|c| "[]0, ".contains(c)) || v == "\"\"" || v == "None")
    });
    out.join(" ")
}

fn short(s: &str, n: usize) -> String {
    s.replace(['\r', '\n'], " ").chars().take(n).collect()
}

/// Prints a map's buildings, armies, points and events (`RAZDOR_DUMP=<map prefix>`).
#[test]
fn dump_map() {
    let Some(prefix) = std::env::var_os("RAZDOR_DUMP") else { return };
    let Some(dt) = install() else { return };
    let prefix = prefix.to_string_lossy().to_string();
    let m = dt.maps.iter().find(|m| m.name.starts_with(&prefix)).expect("map");
    let s: Scenario = m.load().unwrap();
    println!("MAP {} {}x{} next={:?} victory={} defeat={} carry={:?} kind={}", m.name, s.header.width, s.header.height, s.next_map, s.header.victory_event, s.header.defeat_event, s.header.carry_over, s.header.scenario_kind);
    for (k, h) in s.header.heroes.iter().enumerate() {
        println!("HERO{k} at ({},{}) b{}", h.x, h.y, h.start_building);
    }
    for (i, b) in s.buildings.iter().enumerate() {
        println!(
            "B{} kind={} ({},{}) owner={:?} fac={} ev={:?} name={}",
            i + 1,
            b.kind,
            b.x,
            b.y,
            b.owner(),
            b.faction,
            b.events().collect::<Vec<_>>(),
            short(&b.name, 30)
        );
    }
    for a in &s.armies {
        println!(
            "A{} ({},{}) model={} inactive={} active={} home={} fac={} resp={} beh={} named={} name={}",
            a.id,
            a.x,
            a.y,
            a.model,
            a.inactive,
            a.is_active(),
            a.home_building,
            a.faction,
            a.respawn_days,
            a.behaviour,
            a.named_character,
            short(&a.name, 30)
        );
    }
    for p in &s.points {
        println!("P{} ({},{}) model={} active={} ev={:?}", p.id, p.x, p.y, p.model, p.active, p.events().collect::<Vec<_>>());
    }
    for (i, e) in s.events.iter().enumerate() {
        println!(
            "E{} kind={} t={} rep={} dur={} arch={} once={} sub={} flags={:?} | C: {} | R: {} | u={:?}{:?}{:?}{:?} 151={:?} | T={} | Q={} | M={}",
            i + 1,
            e.kind,
            e.start_time,
            e.repeat,
            e.duration,
            e.archetype,
            e.once,
            e.subordinate,
            e.flags.as_ref().map(|f| f.raw.clone()),
            nonzero_debug(&format!("{:?}", e.conditions)),
            nonzero_debug(&format!("{:?}", e.results)),
            e.unknown_23,
            e.unknown_27,
            e.unknown_87,
            e.unknown_91,
            e.unknown_151,
            short(e.title_text(), 40),
            short(&e.question, 60),
            short(&e.message, 160)
        );
    }
}

// -------------------------------------------------------------------------------------------
// The driver: a Game played without a window.
// -------------------------------------------------------------------------------------------

use crate::rules::battle::Team;
use crate::rules::content::{Content, HeroClass};
use crate::rules::events::{EventId, EventOutcome};
use crate::rules::game::{Event, Foe, Game};
use std::collections::BTreeSet;
use std::sync::Arc;

/// A game driven by a script: walks, battles won by wiping the enemy, questions answered by
/// a policy. Every scenario outcome is logged.
pub(crate) struct Driver {
    pub g: Game,
    pub s: Scenario,
    pub fired: Vec<EventId>,
    pub asked: Vec<EventId>,
    pub ended: Option<EventOutcome>,
    pub battles: usize,
    /// Answer for a question (default Yes).
    pub no_to: BTreeSet<EventId>,
    pub verbose: bool,
    /// Armies met so far, one entry per meeting (their scenario ids).
    pub met_log: Vec<u8>,
}

impl Driver {
    pub fn new(dt: &DtInstall, content: &Arc<Content>, prefix: &str, class: HeroClass) -> Driver {
        let m = dt.maps.iter().find(|m| m.name.starts_with(prefix)).expect("map");
        let s = m.load().unwrap();
        let mut g = Game::from_scenario(content.clone(), &s, class);
        g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
        let mut d = Driver { g, s, fired: Vec::new(), asked: Vec::new(), ended: None, battles: 0, no_to: BTreeSet::new(), verbose: std::env::var_os("RAZDOR_REPLAY_LOG").is_some(), met_log: Vec::new() };
        let opening = d.g.drain_events();
        d.settle(opening);
        d
    }

    fn log(&self, s: String) {
        if self.verbose {
            println!("  [{}] {s}", self.g.clock.total_minutes() as u64);
        }
    }

    /// Handles what happened: logs outcomes, answers questions, fights battles.
    pub fn settle(&mut self, events: Vec<Event>) {
        let mut queue = events;
        for _ in 0..500 {
            let mut next = Vec::new();
            for e in queue {
                match e {
                    Event::Script(o) => match o {
                        EventOutcome::Fired { event, .. } => {
                            self.log(format!("fired E{event}"));
                            self.fired.push(event);
                        }
                        EventOutcome::Question(id) => {
                            self.asked.push(id);
                            let yes = !self.no_to.contains(&id);
                            self.log(format!("question E{id}: {}", if yes { "yes" } else { "no" }));
                            next.extend(self.g.answer_question(yes));
                        }
                        EventOutcome::Declined(id) => self.log(format!("declined E{id}")),
                        o @ (EventOutcome::Victory(_) | EventOutcome::Defeat(_)) => {
                            self.log(format!("END {o:?}"));
                            self.ended = Some(o);
                        }
                        o => self.log(format!("{o:?}")),
                    },
                    Event::Encounter(_) => {}
                    Event::Met(i) => {
                        if let Some(a) = self.g.world.armies.get(i) {
                            let id = a.id;
                            self.log(format!("met A{id}"));
                            self.met_log.push(id);
                        }
                    }
                    Event::Captured(l) => self.log(format!("captured B{}", self.g.world.locations[l].id)),
                    _ => {}
                }
            }
            // The player reads the window shown: the events after it run (0x4ab1ec).
            if next.is_empty() && self.g.script().is_some_and(|s| s.holds_window()) {
                next.extend(self.g.event_window_closed());
                if !next.is_empty() {
                    queue = next;
                    continue;
                }
            }
            if self.g.foe.is_some() && self.g.pending_question().is_none() {
                self.fight();
            }
            next.extend(self.g.drain_events());
            if next.is_empty() && self.g.foe.is_none() {
                break;
            }
            queue = next;
        }
    }

    /// Wins the pending battle by wiping the enemy.
    fn fight(&mut self) {
        let what = match self.g.foe {
            Some(Foe::Army(i)) => format!("army A{}", self.g.world.armies[i].id),
            Some(Foe::Garrison(l)) => format!("garrison of B{}", self.g.world.locations[l].id),
            None => return,
        };
        let mut b = self.g.start_battle();
        b.begin();
        for f in b.fighters.iter_mut().filter(|f| f.team == Team::Enemy) {
            f.hp = 0;
        }
        let r = self.g.resolve_battle(&b);
        self.battles += 1;
        self.log(format!("beat {what}: {}", matches!(r, crate::rules::game::BattleResult::Victory { .. })));
    }

    /// Walks towards `to` (re-planned as needed) until standing there or `stop` holds.
    pub fn walk(&mut self, to: Tile, stop: &dyn Fn(&Game) -> bool) -> bool {
        for _ in 0..400 {
            if self.ended.is_some() || stop(&self.g) {
                return true;
            }
            if !self.g.moving() && !self.g.set_destination(to) {
                return false;
            }
            for _ in 0..2000 {
                let ev = self.g.tick(1.0);
                self.settle(ev);
                if !self.g.moving() || self.ended.is_some() || stop(&self.g) {
                    break;
                }
            }
            if self.g.tile() == to {
                return true;
            }
        }
        false
    }

    fn loc_index(&self, id: u16) -> usize {
        self.g.world.locations.iter().position(|l| l.id == id).expect("building")
    }

    /// Enters building `id` (steps out first if standing in it).
    pub fn enter(&mut self, id: u16) -> bool {
        let l = self.loc_index(id);
        if self.g.location == Some(l) {
            self.step_out();
        }
        let tile = self.g.world.locations[l].tile;
        let ok = self.walk(tile, &|g| g.location == Some(l));
        self.log(format!("entered B{id}: {ok}"));

        if ok {
            self.hear_rumours();
        }
        ok
    }

    /// Steps onto a neighbouring free cell.
    pub fn step_out(&mut self) {
        let here = self.g.tile();
        let map = &self.g.world.map;
        let n = map.grid.neighbours(here).find(|&n| map.passable(n) && self.g.world.location_at(n).is_none());
        if let Some(n) = n {
            self.walk(n, &|_| false);
        }
        self.g.location = None;
    }

    /// Walks onto event point `id`.
    pub fn point(&mut self, id: u8) -> bool {
        let p = self.s.points.iter().find(|p| p.id == id).expect("point");
        let t = (p.x as i32, p.y as i32);
        if self.g.tile() == t {
            self.step_out();
        }
        let ok = self.walk(t, &|_| false);
        self.log(format!("on point P{id}: {ok}"));
        ok
    }

    /// Chases army `id` until it is met or beaten (a friendly one is met).
    pub fn reach_army(&mut self, id: u8) -> bool {
        // Success is a new meeting with it, or it beaten, since this call began: the player
        // clicks the army ("click it to talk or fight"), so an army met before talks again.
        let meetings = self.met_log.iter().filter(|&&m| m == id).count();
        let beaten = self.g.beaten_armies.contains(&id);
        // Stepped onto, a friend is engaged as a foe is (world.md §4.2): no `Met` comes, and
        // an event fired while he stands by it, stopped, stands for the meeting.
        let fired = self.fired.len();
        let engaged = |d: &Driver| {
            let by = d.g.world.armies.iter().find(|a| a.id == id).is_some_and(|a| d.g.world.map.distance(d.g.tile(), a.tile(&d.g.world.map)) <= 1);
            d.fired.len() > fired && by && !d.g.moving()
        };
        for _ in 0..200 {
            let Some(a) = self.g.world.armies.iter().find(|a| a.id == id) else { return false };
            let (t, hostile, uid) = (a.tile(&self.g.world.map), a.hostile(), a.uid);
            let near = self.g.world.map.distance(self.g.tile(), t);
            // Close to an army on the move, a player stands and lets it come (a patrolling
            // friend that seeks him out keeps its distance while chased).
            if (2..=3).contains(&near) && !hostile {
                let ev = self.g.wait(1);
                self.settle(ev);
                if self.met_log.iter().filter(|&&m| m == id).count() > meetings {
                    return true;
                }
                // Still apart: click it again.
                self.g.talk_to = Some(uid);
            }
            self.g.stop();
            if !self.g.set_destination(t) {
                // Stand next to it instead; with no way there now (it stands where no path
                // leads, e.g. behind a guard), wait an hour for it to move on, as a player would.
                let map = &self.g.world.map;
                let next = map.grid.neighbours(t).find(|&n| map.passable(n));
                if !next.is_some_and(|n| self.g.set_destination(n)) {
                    let ev = self.g.wait(1);
                    self.settle(ev);
                    if self.ended.is_some() {
                        return false;
                    }
                    continue;
                }
            }
            for _ in 0..5 {
                let ev = self.g.tick(STEP);
                self.settle(ev);
                if !self.g.moving() {
                    break;
                }
            }
            if self.g.beaten_armies.contains(&id) && !beaten || self.met_log.iter().filter(|&&m| m == id).count() > meetings || engaged(self) {
                return true;
            }
        }
        false
    }

    /// Hears every rumour on offer here, as a player reading the main hall would.
    pub fn hear_rumours(&mut self) {
        for _ in 0..20 {
            let Some(&r) = self.g.hall_here().first() else { break };
            self.log(format!("hear rumour E{r}"));
            let Ok(ev) = self.g.take_hall_entry(r) else { break };
            self.settle(ev);
        }
    }

    pub fn wait(&mut self, hours: u32) {
        let ev = self.g.wait(hours);
        self.settle(ev);
    }

    /// Why each not-yet-fired event does not fire here and now.
    pub fn report(&self, ids: impl Iterator<Item = EventId>) {
        let e = self.g.script().unwrap();
        for id in ids {
            println!("  E{id}: fired {}x, {}", e.times_fired(id), e.why_not(id, &self.g).unwrap_or_else(|| "eligible".into()));
        }
    }
}

type Tile = (i32, i32);
const STEP: f32 = crate::rules::game::STEP_SECONDS * 1.01;

fn content(dt: &DtInstall) -> Arc<Content> {
    Arc::new(Content::from_dt(dt))
}

impl Driver {
    /// The next campaign map after a victory, with what carries over.
    pub fn next_map(&self, dt: &DtInstall, content: &Arc<Content>) -> Driver {
        let next = self.g.next_map().expect("a victory with a next map");
        let stem = next.name.trim().trim_end_matches(".DTm").to_string();
        let m = dt.maps.iter().find(|m| m.name == stem).expect("the next map is installed");
        let s = m.load().unwrap();
        let mut g = Game::from_campaign(content.clone(), &s, &next);
        g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
        let mut d = Driver { g, s, fired: Vec::new(), asked: Vec::new(), ended: None, battles: 0, no_to: self.no_to.clone(), verbose: self.verbose, met_log: Vec::new() };
        let opening = d.g.drain_events();
        d.settle(opening);
        d
    }

    fn step(&self, what: &str, ids: &[EventId]) {
        if self.verbose {
            println!("{what}:");
            self.report(ids.iter().copied());
        }
    }

    fn has_fired(&self, id: EventId) -> bool {
        self.g.script().unwrap().times_fired(id) > 0
    }
}

/// РК1 along its quest line: the castle, the priest, the undead and the ruins, the suzerain,
/// the herald (Yes, or No: then the herald waits at the inn up north), the inn.
fn play_rk1(d: &mut Driver) {
    d.enter(1);
    d.step("castle", &[4, 6, 8]);
    d.enter(4);
    d.enter(5);
    d.step("church", &[8, 9, 10, 12]);
    for p in 6..=10 {
        d.point(p);
    }
    for a in [3, 5] {
        d.reach_army(a);
    }
    d.enter(5);
    // The suzerain gives his errand (he no longer happens to cross the hero's way here).
    d.reach_army(1);
    d.enter(11);
    d.step("ruins B11", &[54]);
    d.enter(8);
    d.step("ruins B8", &[18, 19]);
    d.reach_army(2);
    d.reach_army(1);
    d.step("suzerain", &[20, 21, 22, 23]);
    d.enter(5);
    d.step("church", &[15, 16, 28, 29]);
    d.enter(1);
    d.step("castle", &[32, 51]);
    d.enter(7);
    d.step("inn", &[37, 38]);
}

/// РК2 along its quest line: the baron's letter, peasants for the two mines, the Long
/// Knives and the Swamp King, the inn up north.
fn play_rk2(d: &mut Driver) {
    d.enter(7);
    d.step("town", &[3, 4, 5]);
    d.enter(4);
    d.step("village", &[8, 9, 10]);
    d.enter(22);
    d.step("north fort", &[17, 18, 19, 20, 21]);
    d.enter(4);
    d.step("village", &[8, 9, 10]);
    d.enter(23);
    d.step("south fort", &[22, 23, 24, 25, 26, 27]);
    d.reach_army(9);
    d.reach_army(11);
    d.enter(2);
    d.step("the swamp king", &[36, 37]);
    d.enter(27);
    d.step("inn", &[32, 33, 34]);
}

/// РК3 along its quest line: the king, the baron's three errands, the king again.
fn play_rk3(d: &mut Driver) {
    d.enter(60);
    d.step("capital", &[2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 26]);
    d.enter(9);
    d.reach_army(6);
    d.step("baron", &[17, 18]);
    // Army 19 takes the fort B140 on its own first (its AI assaults it): beat it, then take
    // the fort back.
    d.reach_army(19);
    d.enter(140);
    d.reach_army(6);
    d.step("baron", &[19, 20]);
    d.reach_army(21);
    d.reach_army(6);
    d.step("baron", &[21, 22]);
    d.reach_army(20);
    d.reach_army(18);
    d.reach_army(6);
    d.step("baron", &[23]);
    d.enter(60);
    d.step("capital", &[24, 25, 27, 28, 29, 30, 31, 32]);
    for _ in 0..4 {
        d.wait(1);
    }
    d.step("an hour on", &[32]);
}

/// РК1 both ways: the Yes to the herald and the No (the herald waits at the inn; the inn's
/// event chains the victory, whose own condition is the Yes).
#[test]
fn rk1_reaches_its_victory_either_way() {
    let Some(dt) = install() else { return };
    let c = content(&dt);
    for no in [false, true] {
        let mut d = Driver::new(&dt, &c, "РК1", HeroClass::Knight);
        if no {
            d.no_to.insert(28);
        }
        play_rk1(&mut d);
        assert_eq!(d.ended, Some(EventOutcome::Victory(38)), "no: {no}; fired {:?}", d.fired);
        assert!(d.has_fired(if no { 37 } else { 36 }));
        // The suzerain's monologue is a meeting event: once per meeting, not every step.
        assert!(d.g.script().unwrap().times_fired(24) <= 3, "{}", d.g.script().unwrap().times_fired(24));
    }
}

/// The campaign РК1 → РК2 → РК3: each victory hands over to the next map with the army
/// (the herald) and the flags; РК3's king rewards the band and the swamp king beaten in РК2.
#[test]
fn campaign_rk1_to_rk3() {
    let Some(dt) = install() else { return };
    let c = content(&dt);
    let mut d = Driver::new(&dt, &c, "РК1", HeroClass::Knight);
    play_rk1(&mut d);
    assert_eq!(d.ended, Some(EventOutcome::Victory(38)));
    let mut d = d.next_map(&dt, &c);
    assert_eq!(d.g.script().unwrap().times_fired(29), 0, "the herald came along");
    assert_eq!(d.ended, None);
    play_rk2(&mut d);
    println!("РК2 fired {:?}, flags {:?}", d.fired, d.g.script().unwrap().flag_string());
    assert_eq!(d.ended, Some(EventOutcome::Victory(34)), "fired {:?}", d.fired);
    assert!(d.g.script().unwrap().flag("Band") && d.g.script().unwrap().flag("King"));
    let mut d = d.next_map(&dt, &c);
    play_rk3(&mut d);
    println!("РК3 fired {:?}", d.fired);
    for id in [5, 7, 8, 9, 13] {
        assert!(d.has_fired(id), "РК3 E{id}; fired {:?}", d.fired);
    }
    assert_eq!(d.ended, Some(EventOutcome::Victory(32)), "fired {:?}", d.fired);
}

/// Counts data patterns whose reading is in question (`RAZDOR_SCAN=1`).
#[test]
fn scan_patterns() {
    if std::env::var_os("RAZDOR_SCAN").is_none() {
        return;
    }
    let Some(dt) = install() else { return };
    for m in &dt.maps {
        let s = m.load().unwrap();
        let mut notes: Vec<String> = Vec::new();
        for (i, e) in s.events.iter().enumerate() {
            let id = i + 1;
            let c = &e.conditions;
            for k in 0..3 {
                if c.buildings_check != 0 && c.buildings[k] != 0 && c.buildings_owner[k] != 1 {
                    notes.push(format!("E{id} building B{} owner code {}", c.buildings[k], c.buildings_owner[k]));
                }
                if c.units_check != 0 && c.units[k] != 0 && !matches!(c.units_owner[k], 1 | 6) {
                    notes.push(format!("E{id} unit {} owner code {}", c.units[k], c.units_owner[k]));
                }
                if c.artifacts_check != 0 && c.artifacts[k] != 0 && !matches!(c.artifacts_owner[k], 1 | 6) {
                    notes.push(format!("E{id} artifact {} owner code {}", c.artifacts[k], c.artifacts_owner[k]));
                }
            }
            if c.units_check != 0 {
                let u: Vec<_> = (0..3).filter(|&k| c.units[k] != 0).map(|k| (c.units[k], c.units_named[k], c.units_owner[k])).collect();
                if u.len() > 1 {
                    notes.push(format!("E{id} units {u:?}"));
                }
            }
            if e.results.chained_event != 0 {
                let t = &s.events[e.results.chained_event as usize - 1];
                let tc = &t.conditions;
                let has = tc.buildings_check + tc.units_check + tc.artifacts_check + tc.defeated_check + tc.happened_yes_check + tc.not_happened_check + tc.beaten_check + tc.happened_no_check + tc.stats_check + tc.meet_army + tc.army_active + tc.army_inactive + tc.army_at_home;
                if has != 0 || t.archetype != 0 || t.flags.as_ref().is_some_and(|f| f.require_set.is_some() || f.require_unset.is_some()) {
                    notes.push(format!("E{id} chains E{} which has conditions (sub={} once={})", e.results.chained_event, t.subordinate, t.once));
                }
            }
        }
        for (i, b) in s.buildings.iter().enumerate() {
            for id in b.events() {
                let k = s.events[id as usize - 1].kind;
                let village = matches!(b.kind, 2 | 9);
                if village && k != 2 {
                    notes.push(format!("B{} (kind {}) lists E{id} of kind {k}", i + 1, b.kind));
                }
                if !village && k == 3 {
                    notes.push(format!("B{} (kind {}) lists quest E{id}", i + 1, b.kind));
                }
                if k == 1 {
                    notes.push(format!("B{} lists global E{id}", i + 1));
                }
            }
        }
        for p in &s.points {
            for id in p.events() {
                let k = s.events[id as usize - 1].kind;
                if k != 2 {
                    notes.push(format!("P{} lists E{id} of kind {k}", p.id));
                }
            }
        }
        println!("{}: {}", m.name, notes.len());
        for n in notes {
            println!("    {n}");
        }
    }
}

/// Every shipped map, played by a script that walks everywhere and beats everything
/// (`RAZDOR_SMOKE=1`, best with `--release`): buildings nearest first, event points, every
/// army on the map, three rounds, questions answered Yes. Prints how far each map gets and
/// why each event that never fired did not.
#[test]
fn smoke_every_map() {
    if std::env::var_os("RAZDOR_SMOKE").is_none() {
        return;
    }
    let Some(dt) = install() else { return };
    let c = content(&dt);
    let only = std::env::var("RAZDOR_SMOKE").unwrap_or_default();
    let mut prev: Option<Driver> = None;
    for m in &dt.maps {
        if only.len() > 1 && !m.name.starts_with(&only) {
            continue;
        }
        // A campaign map goes on from the last one's victory, as a player gets there.
        let follows = prev.as_ref().and_then(|p| p.g.next_map()).is_some_and(|n| n.name.trim().trim_end_matches(".DTm") == m.name);
        let mut d = match prev.take() {
            Some(p) if follows => p.next_map(&dt, &c),
            _ => Driver::new(&dt, &c, &m.name, HeroClass::Knight),
        };
        let from = if follows { " (from the last map)" } else { "" };
        let started = std::time::Instant::now();
        'rounds: for _round in 0..3 {
            let mut todo: Vec<u16> = d.g.world.locations.iter().filter(|l| !l.kind.is_bridge() && l.id != 0).map(|l| l.id).collect();
            while !todo.is_empty() {
                let here = d.g.tile();
                let map = &d.g.world.map;
                // Buildings with events first, the nearest of them.
                let k = (0..todo.len())
                    .min_by_key(|&k| {
                        let l = d.g.world.locations.iter().find(|l| l.id == todo[k]).unwrap();
                        (l.events.iter().all(|&e| e == 0), map.distance(here, l.tile))
                    })
                    .unwrap();
                let id = todo.remove(k);
                d.enter(id);
                if d.ended.is_some() || started.elapsed().as_secs() > 240 {
                    break 'rounds;
                }
            }
            let points: Vec<u8> = d.s.points.iter().filter(|p| p.events().next().is_some()).map(|p| p.id).collect();
            for p in points {
                d.point(p);
                if d.ended.is_some() {
                    break 'rounds;
                }
            }
            let armies: Vec<u8> = d.g.world.armies.iter().map(|a| a.id).filter(|&a| a != 0).collect();
            for a in armies {
                d.reach_army(a);
                if d.ended.is_some() || started.elapsed().as_secs() > 240 {
                    break 'rounds;
                }
            }
            d.wait(24);
        }
        let e = d.g.script().unwrap();
        let n = d.s.events.len() as u16;
        let fired = (1..=n).filter(|&id| e.times_fired(id) > 0).count();
        println!(
            "{}{from}: {fired}/{n} events fired, {} battles, day {}, ended {:?}, {:.0}s",
            m.name,
            d.battles,
            d.g.clock.day_index() - d.g.world.start.day_index(),
            d.ended,
            started.elapsed().as_secs_f32()
        );
        if std::env::var_os("RAZDOR_SMOKE_WHY").is_some() {
            d.g.location = None;
            let e = d.g.script().unwrap();
            for id in (1..=n).filter(|&id| e.times_fired(id) == 0) {
                println!("    E{id}: {}", e.why_not(id, &d.g).unwrap_or_else(|| "eligible".into()));
            }
        }
        prev = Some(d);
    }
}
