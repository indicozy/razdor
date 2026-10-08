//! The scenario's event engine in the running game: [`Game`] as the engine's [`EventWorld`],
//! and the moments the engine runs (mechanics.md §8.1).
//!
//! The engine runs at the start of a scenario, whenever game time passes (every slice of a
//! walk, a wait, a heal, a delay), when the hero steps into a building or onto an event
//! point, after a battle, when the player answers a question and when he pays for a rumour.
//! What it did comes back as [`Event::Script`] outcomes; the UI shows them (texts are read
//! from the scenario at runtime, never stored).
//!
//! Choices where the sources are silent are marked *(guess)* and listed in mechanics.md §8.1.

use super::content::{HeroClass, ItemId, UnitId, WageKind};
use super::events::{find_unit, ArmyId, EventEngine, EventId, EventOutcome, EventWorld, Holder, Place, UnitRecord};
use super::units::{SpellSlot, SPELL_SLOTS};
use super::game::{troop_unit, unit_into_troop, Event, Foe, Game, PACK_SIZE};
use super::town::ServiceError;
use super::units::Unit;
use super::world::{Army, EventInfo, Troop};
use crate::dt::dtm::EventKind;

/// Radius revealed around an army an event shows: 6 half-cells (world.md §3, 0x4ab6c3).
const SHOW_ARMY_RADIUS: i32 = 3;

/// How the scenario ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptEnd {
    Victory(EventId),
    Defeat(EventId),
}

/// A line of a main hall's list (0x4beaac): the building's quests and rumours that pass
/// their check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HallEntry {
    /// A rumour on offer. Hearing it is free; any price is the rumour event's own (its gold
    /// condition and result).
    Rumour(EventId),
    /// A quest on offer: taking it opens its dialog.
    Quest(EventId),
}

impl Event {
    /// Something the player should read before time goes on: a message, a question, a quest
    /// notice or the end of the scenario.
    pub fn needs_reading(&self) -> bool {
        match self {
            Event::Script(EventOutcome::Fired { message, .. }) => *message,
            Event::Script(EventOutcome::Declined(_) | EventOutcome::LoopGuard) => false,
            Event::Script(_) => true,
            _ => false,
        }
    }
}

impl Game {
    /// The scenario's event engine, if the game has one (not in the demo).
    pub fn script(&self) -> Option<&EventEngine> {
        self.script.as_deref()
    }

    /// Victory or defeat by a scenario event.
    pub fn script_end(&self) -> Option<ScriptEnd> {
        match self.script()?.ended()? {
            EventOutcome::Victory(id) => Some(ScriptEnd::Victory(*id)),
            EventOutcome::Defeat(id) => Some(ScriptEnd::Defeat(*id)),
            _ => None,
        }
    }

    /// Events that happened outside a tick or a wait (see [`Game::pending`]).
    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    /// Runs the event engine once. Returns its outcomes, then what its effects caused (noon
    /// reports during a delay, a battle).
    pub(crate) fn run_script(&mut self) -> Vec<Event> {
        let Some(mut engine) = self.script.take() else { return Vec::new() };
        let out = engine.tick(self);
        self.script = Some(engine);
        // A check that fires nothing goes idle, which drops the wish for a check at the
        // building window's close (0x4ac3a6).
        if !out.iter().any(|o| matches!(o, EventOutcome::Fired { .. } | EventOutcome::Question(_))) {
            self.scan_on_close = false;
        }
        self.script_events(out)
    }

    /// A window over the map was closed (not by opening another one, 0x4b8d28(0)): after a
    /// heal, a raise or a trade in the building window the events are checked now
    /// (0x4b8f63), so one those changes allow opens as the window closes, not at the next
    /// step.
    pub fn window_closed(&mut self) -> Vec<Event> {
        if !self.scan_on_close {
            return Vec::new();
        }
        self.run_script()
    }

    fn script_events(&mut self, out: Vec<EventOutcome>) -> Vec<Event> {
        self.record_outcomes(&out);
        // An event that took effect ends with the army recomputed and the hero's pairs with
        // the AI's armies marked to be rescored (0x4ab1ec → 0x497240(0, 1)).
        if out.iter().any(|o| matches!(o, EventOutcome::Fired { .. })) {
            self.mark_dirty(super::ai::HERO);
            self.recount_hero();
        }
        let mut events: Vec<Event> = out.into_iter().map(Event::Script).collect();
        let effects = std::mem::take(&mut self.effect_events);
        // A battle an event started: against the army as it stands after all the effects.
        let mut battle = false;
        for e in effects {
            match e {
                Event::Encounter(_) => battle = true,
                e => events.push(e),
            }
        }
        if battle {
            if let Some(Foe::Army(i)) = self.foe {
                events.push(Event::Encounter(i));
            }
        }
        events
    }

    /// The player's answer to the question waiting in the engine; then the events run on.
    /// Returns what followed.
    pub fn answer_question(&mut self, yes: bool) -> Vec<Event> {
        let Some(mut engine) = self.script.take() else { return Vec::new() };
        let out = engine.answer(self, yes);
        self.script = Some(engine);
        self.script_events(out)
    }

    /// A scenario event's window was closed (its OK): the events after it, which the
    /// original scans only now (0x4c206c → 0x4ab1ec → the chain or a new scan), run.
    pub fn event_window_closed(&mut self) -> Vec<Event> {
        let Some(mut engine) = self.script.take() else { return Vec::new() };
        let out = engine.window_closed(self);
        self.script = Some(engine);
        if out.is_empty() {
            return Vec::new();
        }
        self.script_events(out)
    }

    /// The interface shows no scenario event's window (`shown` false) though the engine waits
    /// for one to be closed, and no event waits to be drained or for its step to play
    /// ([`Game::tick_shown`]): the scan goes on as if it had been closed (a safety net for an
    /// outcome the screen did not turn into a window).
    pub fn release_unshown_window(&mut self, shown: bool) -> Vec<Event> {
        if shown || !self.pending.is_empty() || !self.held.is_empty() || !self.script.as_ref().is_some_and(|s| s.holds_window()) {
            return Vec::new();
        }
        self.event_window_closed()
    }

    /// The question waiting for an answer, if any.
    pub fn pending_question(&self) -> Option<EventId> {
        self.script()?.pending_question()
    }

    /// The main hall's list here (0x4beaac): the building's quests and rumours that pass
    /// their check, in its list order.
    pub fn hall_entries(&self) -> Vec<HallEntry> {
        let Some(engine) = self.script() else { return Vec::new() };
        let quest = |id: EventId| self.world.events.get(id as usize - 1).is_some_and(|e: &EventInfo| e.kind == Some(EventKind::Quest));
        engine.hall(self).into_iter().map(|id| if quest(id) { HallEntry::Quest(id) } else { HallEntry::Rumour(id) }).collect()
    }

    /// The ids of the main hall's list here.
    pub fn hall_here(&self) -> Vec<EventId> {
        self.script().map_or_else(Vec::new, |e| e.hall(self))
    }

    /// Takes entry `id` of this main hall (0x4bb798): its dialog opens at once, without a new
    /// check (its question first if it asks one), then the events run on. There is no flat
    /// price: a rumour that costs something says so in its own event (a gold condition and a
    /// negative gold result), as the original's rumours do.
    pub fn take_hall_entry(&mut self, id: EventId) -> Result<Vec<Event>, ServiceError> {
        if !self.hall_here().contains(&id) || self.pending_question().is_some() {
            return Err(ServiceError::NotHere);
        }
        let Some(mut engine) = self.script.take() else { return Ok(Vec::new()) };
        let out = engine.take(self, id);
        self.script = Some(engine);
        Ok(self.script_events(out))
    }

    /// An army that met the hero (`Met` or `Encounter` from [`Game::ai_contact`]): the
    /// meeting is recorded and the events run with it as the met army (0x4ade3c). An attack
    /// opens the battle only if none of them fired and the army is still there and hostile.
    /// Returns whether an event fired (only then does a greeting stop his walk).
    pub(crate) fn meet(&mut self, e: Event, events: &mut Vec<Event>) -> bool {
        self.meet_as(e, events, false)
    }

    /// The hero stepped onto army `e`'s cell (world.md §4.2 e, 0x4ad94c): the events run with
    /// it as the met army; the battle opens only if none of them fired (an army ill-disposed
    /// to him, attitude 0 included). If one fired, the army forgets him for now: its talk
    /// counter is −500 and it plans again.
    pub(crate) fn engage(&mut self, e: Event, events: &mut Vec<Event>) {
        self.meet_as(e, events, true);
    }

    fn meet_as(&mut self, e: Event, events: &mut Vec<Event>, on_step: bool) -> bool {
        let (Event::Met(i) | Event::Encounter(i)) = e else {
            events.push(e);
            return false;
        };
        let id = self.world.armies[i].id;
        if id == 0 || self.script.is_none() {
            events.push(e);
            return false;
        }
        self.met_armies.insert(id);
        let after = match self.script.take() {
            Some(mut engine) => {
                let out = engine.meet(self, id);
                self.script = Some(engine);
                self.script_events(out)
            }
            None => Vec::new(),
        };
        let now = self.world.armies.iter().position(|a| a.id == id);
        let fired = after.iter().any(|e| matches!(e, Event::Script(EventOutcome::Fired { .. } | EventOutcome::Question(_))));
        let fights = |a: &Army| on_step || a.hostile();
        match e {
            // A battle the events started instead comes with `after`.
            Event::Encounter(_) if after.iter().any(|e| matches!(e, Event::Encounter(_))) => {}
            // An event fired: no battle (0x4ad94c, 0x4ade3c). Stepped onto, the army also
            // forgets him for now; an attacker does not.
            Event::Encounter(_) if fired => {
                self.foe = None;
                if let Some(j) = now.filter(|_| on_step) {
                    let a = &mut self.world.armies[j];
                    a.talk = super::game::TALKED;
                    a.path.clear();
                }
            }
            Event::Encounter(_) => match now.filter(|&j| fights(&self.world.armies[j])) {
                Some(j) => {
                    self.foe = Some(Foe::Army(j));
                    events.push(Event::Encounter(j));
                }
                None => self.foe = None,
            },
            _ => events.extend(now.map(Event::Met)),
        }
        events.extend(after);
        fired
    }

    /// Removes active army `i` from the map, keeping the pending foe pointing at the right army.
    fn take_army(&mut self, i: usize) -> Army {
        match self.foe {
            Some(Foe::Army(j)) if j == i => self.foe = None,
            Some(Foe::Army(j)) if j > i => self.foe = Some(Foe::Army(j - 1)),
            _ => {}
        }
        self.world.armies.remove(i)
    }

    fn army_index(&self, id: ArmyId) -> Option<usize> {
        self.world.armies.iter().position(|a| a.id == id)
    }

    /// An army by id, on the map or waiting.
    fn army_mut(&mut self, id: ArmyId) -> Option<&mut Army> {
        let w = &mut self.world;
        w.armies.iter_mut().chain(w.inactive.iter_mut()).find(|a| a.id == id)
    }

    /// Every army record: on the map, waiting, and beaten waiting for a respawn.
    fn army_records(&self) -> impl Iterator<Item = &Army> {
        let w = &self.world;
        w.armies.iter().chain(w.inactive.iter()).chain(w.respawns.iter().map(|r| &r.army))
    }

    /// Brings a waiting army onto the map. Returns its index. A ship comes onto the water
    /// (`rules::ships`); a land army with no land near its post stays out.
    fn activate(&mut self, id: ArmyId) -> Option<usize> {
        if let Some(i) = self.army_index(id) {
            return Some(i);
        }
        // A waiting army, or a beaten one waiting for its respawn: 0x4969b8 brings back any
        // army off the map, clearing its destroyed and "beaten by" marks.
        let (mut a, respawning) = match self.world.inactive.iter().position(|a| a.id == id) {
            Some(k) => (self.world.inactive[k].clone(), None),
            None => {
                let k = self.world.respawns.iter().position(|r| r.army.id == id)?;
                (self.world.respawns[k].army.clone(), Some(k))
            }
        };
        let tile = self.world.placement(&a)?;
        match respawning {
            Some(k) => {
                self.world.respawns.remove(k);
                self.beaten_armies.remove(&id);
                self.ai_beaten.remove(&id);
            }
            None => {
                let k = self.world.inactive.iter().position(|a| a.id == id)?;
                self.world.inactive.remove(k);
            }
        }
        a.pos = self.world.map.center(tile);
        a.path.clear();
        a.chasing = false;
        if !super::ai::managed(&a) {
            self.world.armies.push(a);
            return Some(self.world.armies.len() - 1);
        }
        // The AI's record as 0x4969b8 sets it: every unit alive (the wounded keep their hit
        // points) and paid now, no path, standing in the building under it with its defence;
        // its home under it is its own again. It takes its place among the armies in their
        // order, its pairs are to be rescored and it draws four wander points.
        let now = self.clock.total_minutes() as u64;
        for t in a.troops.iter_mut() {
            if !t.alive() {
                t.hurt = 0;
            }
            t.died_at = None;
            t.kept_death = None;
            t.unpaid = false;
            t.last_paid = now;
        }
        a.mind.walked = 0;
        a.mind.no_path = true;
        a.mind.free_step = true;
        let here = self.world.location_covering(tile);
        a.mind.standing = here;
        if let Some(l) = here {
            a.mind.defence = self.world.locations[l].garrison_defence;
            if a.home == Some(l) {
                let loc = &mut self.world.locations[l];
                loc.owner = super::world::Owner::Army(a.id);
                loc.take_sides(a.faction, a.ai.relations);
            }
        }
        let uid = a.uid;
        self.insert_army(a);
        self.mark_dirty(uid);
        let i = self.army_index(id)?;
        self.ai_wander(i);
        Some(i)
    }

    /// A text of the scenario with its escape filled in: every `#HERONAME` becomes the hero's
    /// name ([`Game::hero_name`]), case-sensitive (0x4aa3de). There is no other escape.
    pub fn fill_text(&self, s: &str) -> String {
        s.replace("#HERONAME", &self.hero_name()).replace('\r', "")
    }

    /// The name shown for squad member `u`: a named character's own name, else its class.
    pub fn unit_label(&self, u: &Unit) -> String {
        let named = (u.named as usize).checked_sub(1).and_then(|k| self.world.named_characters.get(k));
        match named {
            Some(n) if !n.is_empty() => n.clone(),
            _ => u.name(&self.content).to_string(),
        }
    }

    /// Reveals the cells within `radius` of `at` (lanterns, a shown army) in the fog of war,
    /// and records it in [`Game::pending_reveals`], and the cells it uncovered in
    /// [`Game::shown`] for the map to show.
    pub fn reveal_area(&mut self, at: (i32, i32), radius: i32) {
        let r = radius.max(1);
        self.pending_reveals.push((at.0, at.1, r));
        // Every cell the reveal can reach: the fog's own shape lies within r + 1.
        let reach = r + 1;
        let square = |fog: &crate::rules::fog::Fog| -> Vec<(i32, i32)> {
            let (x0, x1) = ((at.0 - reach).max(0), (at.0 + reach).min(fog.w - 1));
            let (y0, y1) = ((at.1 - reach).max(0), (at.1 + reach).min(fog.h - 1));
            (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).collect()
        };
        let dark: Vec<(i32, i32)> = if self.fog.enabled { square(&self.fog).into_iter().filter(|&t| !self.fog.explored(t)).collect() } else { Vec::new() };
        self.reveal(at.0, at.1, r);
        let cells: Vec<(i32, i32)> = dark.into_iter().filter(|&t| self.fog.explored(t)).collect();
        self.shown.push(super::game::Shown { at, cells, event: None });
    }
}

impl EventWorld for Game {
    fn now(&self) -> u64 {
        self.clock.total_minutes() as u64
    }

    fn hero_archetype(&self) -> u8 {
        self.archetype
    }

    /// The original compares its 0-based level: a level condition of 2 means our level 3.
    fn hero_level(&self) -> i64 {
        self.hero().level as i64 - 1
    }

    fn gold(&self) -> i64 {
        self.gold as i64
    }

    fn mana(&self) -> i64 {
        self.mana as i64
    }

    fn hero_hp(&self) -> i64 {
        self.hero().hp as i64
    }

    /// Every unit record of the army, the hero and the dead included.
    fn squad_count(&self) -> i64 {
        self.squad.len() as i64
    }

    /// The army strength (army +0x1648, 0x4a182b): the tactical cost of every unit record,
    /// with its items and the defence of the building the army stands in (experience.md §1).
    /// The original adds a unit's value before it tests its HP, so the dead count too.
    fn army_strength(&self) -> i64 {
        let bd = self.hero_building_defence();
        self.squad.iter().map(|u| u.tactical(&self.content, bd) as i64).sum()
    }

    /// Whether it is the player's, and its faction (a captured building takes faction 1).
    fn building_state(&self, building: u16) -> Option<(bool, u8)> {
        let l = self.world.locations.iter().find(|l| l.id == building)?;
        Some((l.owner == super::world::Owner::Player, l.faction))
    }

    fn player_units(&self) -> Vec<UnitRecord> {
        self.squad
            .iter()
            .map(|u| UnitRecord { unit: u.def.0, named: u.named, hp: u.hp, from_event: u.wage_kind == WageKind::Event })
            .collect()
    }

    /// Every army of the faction, on the map, waiting or beaten (the original walks all its
    /// army records). A named character leads his army: he is its first unit.
    fn faction_units(&self, faction: u8) -> Vec<Vec<UnitRecord>> {
        self.army_records().filter(|a| a.faction == faction).map(army_units).collect()
    }

    fn player_items(&self) -> (Vec<u8>, Vec<u8>) {
        let id = |i: &ItemId| u8::try_from(i.0).ok();
        let worn = self.squad.iter().flat_map(|u| u.items.iter().flatten()).filter_map(id).collect();
        (self.pack.iter().filter_map(id).collect(), worn)
    }

    fn faction_worn_items(&self, faction: u8) -> Vec<u8> {
        let armies = self.army_records().filter(|a| a.faction == faction);
        armies.flat_map(|a| a.troops.iter().flat_map(|t| t.worn.iter().flatten())).filter_map(|i| u8::try_from(i.0).ok()).collect()
    }

    fn player_defeated(&self, army: ArmyId) -> bool {
        self.beaten_armies.contains(&army)
    }

    /// Beaten by the player or by an AI army (`rules::ai` records AI battles).
    fn army_beaten(&self, army: ArmyId) -> bool {
        self.army_beaten_by_anyone(army)
    }

    fn army_active(&self, army: ArmyId) -> bool {
        self.army_index(army).is_some()
    }

    /// Waiting off the map; a beaten army waiting for its respawn is destroyed, so it is
    /// neither active nor inactive.
    fn army_inactive(&self, army: ArmyId) -> bool {
        self.world.inactive.iter().any(|a| a.id == army)
    }

    /// Its current building is its home (0x4a7b80: army +0x3788 = +0x16b3); an army with
    /// no home passes. An army Razdor no longer keeps passes too *(guess)*.
    fn army_at_home(&self, army: ArmyId) -> bool {
        self.army_records().find(|a| a.id == army).is_none_or(|a| a.home.is_none() || a.mind.standing == a.home)
    }

    fn place(&self) -> Option<Place> {
        if let Some(l) = self.location {
            let id = self.world.locations[l].id;
            if id != 0 {
                return Some(Place::Building(id));
            }
        }
        let t = self.tile();
        self.world.points.iter().find(|p| p.tile == t).map(|p| Place::Point(p.id))
    }

    /// Event XP goes to the hero alone, as it is: no modifier, no cap; a negative amount
    /// does nothing (experience.md §5).
    fn add_experience(&mut self, xp: i64) {
        self.unit_gains(0, xp);
    }

    /// The event's gold is added, the total held at 0 and above (0x4ab1be): a rumour that
    /// costs more than the player has leaves him at 0. Mana alike.
    fn add_gold(&mut self, gold: i64) {
        self.gold = (self.gold as i64 + gold).clamp(0, i32::MAX as i64) as i32;
    }

    fn add_mana(&mut self, mana: i64) {
        self.mana = (self.mana as i64 + mana).clamp(0, i32::MAX as i64) as i32;
    }

    /// A unit joins (0x4a8fe5) at level 1, unhurt, as an event's unit (no wage), paid as of
    /// now. A full army first dismisses its weakest unit after the hero: the lowest level
    /// value ([`experience::level_value`]), the first of a tie, its worn items to the pack.
    /// Taken from `from_army`, the army's unit (the same search as a removal) brings its whole
    /// record and leaves that army; an army left empty goes off the map.
    fn add_unit(&mut self, unit: u8, named: u8, from_army: Option<ArmyId>) {
        let id = UnitId(unit as u32);
        if self.content.try_unit(id).is_none() {
            return;
        }
        let c = self.content.clone();
        if self.squad.len() >= self.max_squad() {
            let value = |u: &Unit| super::experience::level_value(&c, u.def, &u.base_stats(&c));
            let mut weakest = 1;
            for k in 2..self.squad.len() {
                if value(&self.squad[k]) < value(&self.squad[weakest]) {
                    weakest = k;
                }
            }
            if weakest < self.squad.len() {
                let u = self.squad.remove(weakest);
                for item in u.items.iter().flatten() {
                    if self.pack.len() < PACK_SIZE {
                        self.pack.push(*item);
                    }
                }
            }
        }
        let taken: Vec<_> = self.squad.iter().map(|u| u.slot).collect();
        let Some(slot) = self.content.formation.new_unit_slot(&taken) else { return };
        let now = self.clock.total_minutes() as u64;
        let mut u = troop_unit(&c, &Troop::new(id, 1, slot));
        u.named = named;
        u.from_event = true;
        // Kind 3: an event's unit draws no wage.
        u.wage_kind = WageKind::Event;
        u.last_paid = now;
        u.heal_full(&c);
        let mut emptied = None;
        if let Some(a) = from_army.and_then(|a| self.army_mut(a)) {
            let found = find_unit(&army_units(a), unit, named, 0);
            if let Some(k) = found {
                let t = a.troops.remove(k);
                let leader = if k == 0 { std::mem::take(&mut a.named) } else { 0 };
                // The whole record: level, XP, wounds, items, name and kind.
                u = troop_unit(&c, &t);
                u.slot = slot;
                u.named = leader;
                u.wage_kind = t.kind;
                u.from_event = t.kind == WageKind::Event;
                if a.troops.is_empty() {
                    emptied = Some(a.id);
                }
            }
        }
        self.squad.push(u);
        if let Some(a) = emptied {
            self.deactivate_army(a);
        }
    }

    /// The unit leaves the army (0x496310), its record and worn items with it: with no
    /// army to go to, its items are lost, as the original's removal only deletes the record.
    /// Sent to `to_army`, it goes with its whole record: appended, over the last one of a full
    /// army; when the slot names a character (`lead`), it becomes the army's leader instead,
    /// the others moving down (the last of a full army is lost). The original decides by the
    /// slot's name, not the unit's: an unnamed unit picked by type 255 under a named slot
    /// also leads.
    fn remove_unit(&mut self, index: usize, lead: bool, to_army: Option<ArmyId>) {
        if index == 0 || index >= self.squad.len() {
            return;
        }
        let u = self.squad.remove(index);
        let c = self.content.clone();
        let now = self.clock.total_minutes() as u64;
        if let Some(a) = to_army.and_then(|a| self.army_mut(a)) {
            let cap = c.formation.capacity();
            let mut t = Troop::new(u.def, u.level, u.slot);
            unit_into_troop(&c, &mut t, &u, now);
            t.kind = u.wage_kind;
            let full = a.troops.len() >= cap;
            if !lead {
                match a.troops.last_mut() {
                    Some(last) if full => {
                        t.slot = last.slot;
                        *last = t;
                    }
                    _ => {
                        let taken: Vec<_> = a.troops.iter().map(|t| t.slot).collect();
                        let Some(slot) = c.formation.new_unit_slot(&taken) else { return };
                        t.slot = slot;
                        a.troops.push(t);
                    }
                }
            } else {
                if full {
                    let lost = a.troops.pop().map(|t| t.slot);
                    t.slot = lost.unwrap_or(t.slot);
                } else {
                    let taken: Vec<_> = a.troops.iter().map(|t| t.slot).collect();
                    let Some(slot) = c.formation.new_unit_slot(&taken) else { return };
                    t.slot = slot;
                }
                a.troops.insert(0, t);
                // Razdor keeps an army's character on its leader: the new leader's name.
                a.named = u.named;
            }
        }
    }

    fn give_item(&mut self, artifact: u8) {
        let item = ItemId(artifact as u32);
        if self.content.try_item(item).is_some() && self.pack.len() < PACK_SIZE {
            self.pack.push(item);
        }
    }

    /// From the pack, else from whoever wears it.
    fn take_item(&mut self, artifact: u8) {
        let item = ItemId(artifact as u32);
        if let Some(k) = self.pack.iter().position(|&i| i == item) {
            self.pack.remove(k);
            return;
        }
        let c = self.content.clone();
        for u in &mut self.squad {
            if let Some(slot) = u.items.iter().position(|s| *s == Some(item)) {
                crate::rules::items::take_off(&c, u, slot);
                return;
            }
        }
    }

    /// Appends the spell to the book unless it is there (0x49c144). There is no limit: the
    /// spells past the 15th are known but the magic window never shows them.
    fn learn_spell(&mut self, spell: u8) {
        if spell != 0 && !self.spells.contains(&spell) {
            self.spells.push(spell);
        }
    }

    /// The spell takes effect on the player's army at once and for free, whatever its
    /// target ([`Game::apply_spell_to_army_ext`]); its number is held to the number of
    /// spells (0x4ab1ec). While the hero reads a spell it waits in the queue behind his
    /// (and turns his onto his own army: [`Game::end_reading`]).
    fn apply_spell(&mut self, spell: u8) {
        let id = (spell as u32).min(self.content.spells.len() as u32);
        if self.reading.is_some() {
            self.queued_casts.push(id);
        } else if let Some(def) = self.spell(id).cloned() {
            self.apply_spell_to_army_ext(&def, true);
        }
    }

    fn activate_army(&mut self, army: ArmyId) {
        self.activate(army);
    }

    fn deactivate_army(&mut self, army: ArmyId) {
        if let Some(i) = self.army_index(army) {
            let mut a = self.take_army(i);
            a.path.clear();
            a.chasing = false;
            self.world.inactive.push(a);
        } else if let Some(k) = self.world.respawns.iter().position(|r| r.army.id == army) {
            // A beaten army waiting for its respawn is no longer destroyed (0x496900): it
            // never comes back by itself, only an activation brings it (its "beaten by"
            // mark stays).
            let a = self.world.respawns.remove(k).army;
            self.world.inactive.push(a);
        }
    }

    fn show_army(&mut self, army: ArmyId) {
        if let Some(i) = self.army_index(army) {
            let t = self.world.armies[i].tile(&self.world.map);
            self.reveal_area(t, SHOW_ARMY_RADIUS);
        }
    }

    /// The army is moved next to the hero (world.md §4.4, 0x4980d8): of his 8 neighbours,
    /// in the original's direction order, the one with the lowest score, its cost on the LAND
    /// map (the SHIP map when he is at sea; blocked 100 000) + 50 000 on a building's cell +
    /// 100 000 when someone stands there, the first lowest winning. Only a free, open cell
    /// (below 100 000) is used: the army's position and home cell (its post) move there. A
    /// waiting army is not brought onto the map.
    fn move_army_to_hero(&mut self, army: ArmyId) {
        const BLOCKED: i64 = 100_000;
        let here = self.tile();
        let w = &self.world;
        let map = &w.map;
        let at_sea = self.aboard();
        let parked = self.parked_ship();
        let occupied = |t: super::map::Tile| t == here || parked == Some(t) || w.armies.iter().any(|a| a.tile(map) == t);
        let mut best: Option<(i64, super::map::Tile)> = None;
        for (dx, dy) in super::map::DIRECTIONS {
            let t = (here.0 + dx, here.1 + dy);
            if !map.in_bounds(t) {
                continue;
            }
            let cost = if at_sea { map.water_cost(t) } else { map.cost(t) };
            let score = cost.map_or(BLOCKED, i64::from) + if w.location_covering(t).is_some() { 50_000 } else { 0 } + if occupied(t) { BLOCKED } else { 0 };
            if best.is_none_or(|(b, _)| score < b) {
                best = Some((score, t));
            }
        }
        let Some((_, t)) = best.filter(|&(score, _)| score < BLOCKED) else { return };
        let pos = map.center(t);
        if let Some(a) = self.army_mut(army) {
            // The patrol box stays where it was.
            a.box_centre = Some(a.patrol_centre());
            a.pos = pos;
            a.post = t;
            a.path.clear();
        }
    }

    fn light_lantern(&mut self, point: u16) {
        // A lantern without a radius reveals nothing (world.md §3, 0x4ab762).
        if let Some(p) = self.world.points.iter().find(|p| p.id as u16 == point && p.radius > 0).copied() {
            self.reveal_area(p.tile, p.radius);
        }
    }

    fn shown_by(&mut self, event: EventId) {
        for s in self.shown.iter_mut().filter(|s| s.event.is_none()) {
            s.event = Some(event);
        }
    }

    /// radius := max(0, radius + delta), and the patrol box is recomputed around the army's
    /// home cell (0x4ab51b): a box an event left behind (move to the hero) follows it again.
    fn change_patrol(&mut self, army: ArmyId, delta: i8) {
        if let Some(a) = self.army_mut(army) {
            a.patrol_radius = (a.patrol_radius + delta as i32).max(0);
            a.patrols = a.patrol_radius > 0;
            a.box_centre = None;
        }
    }

    /// The hero becomes unit type `unit`, keeping level, XP and items; the class bonuses
    /// (tied to the three hero types) are lost.
    fn set_hero_class(&mut self, unit: u8) {
        let id = UnitId(unit as u32);
        if self.content.try_unit(id).is_none() {
            return;
        }
        let c = self.content.clone();
        let h = &mut self.squad[0];
        h.def = id;
        h.hp = h.hp.min(h.max_hp(&c)).max(1);
    }

    /// A battle with the army (brought onto the map if waiting); the UI opens it.
    fn start_battle(&mut self, army: ArmyId) {
        if let Some(i) = self.activate(army) {
            self.path.clear();
            self.foe = Some(Foe::Army(i));
            self.effect_events.push(Event::Encounter(i));
        }
    }

    /// Time passes for the player alone: the world goes on (armies, noon reports).
    fn delay_player(&mut self, minutes: u64) {
        let mut events = Vec::new();
        self.path.clear();
        self.pass_time(minutes as f32, &mut events);
        self.effect_events.extend(events);
    }

    // --- Community Update extensions (events.md §15) -------------------------------------------

    /// The player's unit wears exactly these items; what it wore goes to the pack. AI units
    /// carry no items of their own in Razdor: for an army the items join its items (the
    /// loot) without changing its fight; a garrison takes none *(recorded, not simulated)*.
    fn equip_unit(&mut self, holder: Holder, unit: u8, items: [u8; 4]) {
        let c = self.content.clone();
        let valid = |i: u8| (i != 0).then_some(ItemId(i as u32)).filter(|&id| c.try_item(id).is_some());
        match holder {
            Holder::Player => {
                let Some(u) = self.squad.get_mut(unit as usize) else { return };
                let old: Vec<ItemId> = u.items.iter().flatten().copied().collect();
                let before = u.max_hp(&c);
                for (k, slot) in u.items.iter_mut().enumerate() {
                    *slot = items.get(k).copied().and_then(valid);
                }
                // Rebuilt: its HP follows its maximum.
                u.follow_max(&c, before);
                for item in old {
                    if self.pack.len() < PACK_SIZE {
                        self.pack.push(item);
                    }
                }
            }
            Holder::Army(a) => {
                if let Some(a) = self.army_mut(a) {
                    a.items.extend(items.iter().copied().filter_map(valid));
                }
            }
            Holder::Building(_) => {}
        }
    }

    /// The unit keeps its level (and, in the player's army, its XP, items and HP fraction).
    fn replace_unit(&mut self, holder: Holder, unit: u8, with: u8) {
        let id = UnitId(with as u32);
        if self.content.try_unit(id).is_none() {
            return;
        }
        if holder == Holder::Player {
            let c = self.content.clone();
            if let Some(u) = self.squad.get_mut(unit as usize) {
                let (hp, max) = (u.hp, u.max_hp(&c).max(1));
                u.def = id;
                if u.alive() {
                    u.hp = (hp * u.max_hp(&c) / max).max(1);
                }
            }
        } else if let Some(t) = self.troops_of(holder).and_then(|t| t.get_mut(unit as usize)) {
            t.unit = id;
        }
    }

    /// The army walks at the new speed; the Community patch writes the hero's speed too
    /// (world.md §2.1, 0xc279e6), which his steps then use instead of his class's
    /// *(guess: the same `max(1, 5 − correction)`, without the loader's archmage rule)*.
    fn set_army_speed(&mut self, holder: Holder, correction: i8) {
        match holder {
            Holder::Army(a) => {
                if let Some(a) = self.army_mut(a) {
                    a.speed = Army::speed_for(correction, a.troops.first().map_or(0, |t| t.unit.0));
                }
            }
            Holder::Player => self.speed_set = Some(Army::speed_for(correction, 0)),
            _ => {}
        }
    }

    /// The army or building takes the group's attitude towards the player from the
    /// scenario's relations; a building that joins the player's group becomes his *(guess)*.
    fn set_faction(&mut self, holder: Holder, group: u8) {
        let attitude = self.world.relations[(group - 1) as usize][0];
        let army_attitude = super::world::relation(super::world::player_attitude_to(&self.world.relations, group), attitude);
        match holder {
            Holder::Army(a) => {
                if let Some(a) = self.army_mut(a) {
                    a.faction = group;
                    a.attitude = if group == 1 { 3 } else { army_attitude };
                }
            }
            Holder::Building(b) => {
                if let Some(l) = self.world.locations.iter_mut().find(|l| l.id == b) {
                    l.faction = group;
                    l.attitude = if group == 1 { 3 } else { attitude };
                    if group == 1 {
                        l.owner = super::world::Owner::Player;
                    }
                }
            }
            Holder::Player => {}
        }
    }

    /// Only the relation towards the player is kept (there is no AI diplomacy yet): other
    /// groups are a recorded no-op.
    fn set_relation(&mut self, holder: Holder, group: u8, value: i8) {
        if group != 0 {
            return;
        }
        match holder {
            Holder::Army(a) => {
                let relations = self.world.relations;
                if let Some(a) = self.army_mut(a) {
                    a.attitude = super::world::relation(super::world::player_attitude_to(&relations, a.faction), value);
                    a.ai.relations[0] = value;
                }
            }
            Holder::Building(b) => {
                if let Some(l) = self.world.locations.iter_mut().find(|l| l.id == b) {
                    l.attitude = value;
                }
            }
            Holder::Player => {}
        }
    }

    /// Opcode 11: slot k of the unit (or of every unit) holds spell k "for good" — until
    /// [`Game::opcode_spell_end`], an absolute time (0xEEEEEE) — or is emptied for a 0.
    /// Garrisons take them too.
    fn set_spells(&mut self, holder: Holder, unit: Option<u8>, spells: &[u8]) {
        let c = self.content.clone();
        let now = self.clock.total_minutes() as u64;
        let end = self.opcode_spell_end();
        let slots: [Option<SpellSlot>; SPELL_SLOTS] = std::array::from_fn(|k| match spells.get(k) {
            Some(&s) if s != 0 => Some(SpellSlot { spell: s as u32, until: end }),
            _ => None,
        });
        let pick = |k: usize| unit.is_none_or(|n| n as usize == k);
        if holder == Holder::Player {
            for (_, u) in self.squad.iter_mut().enumerate().filter(|(k, _)| pick(*k)) {
                let before = u.max_hp(&c);
                u.spells = slots;
                u.follow_max(&c, before);
            }
            return;
        }
        let Some(troops) = self.troops_of(holder) else { return };
        for (_, t) in troops.iter_mut().enumerate().filter(|(k, _)| pick(*k)) {
            let mut u = troop_unit(&c, t);
            let before = u.max_hp(&c);
            u.spells = slots;
            u.follow_max(&c, before);
            unit_into_troop(&c, t, &u, now);
        }
    }

    fn set_named_unit(&mut self, holder: Holder, unit: u8, named: u8, class: u8) {
        let class = Some(UnitId(class as u32)).filter(|&id| class != 0 && self.content.try_unit(id).is_some());
        match holder {
            Holder::Player => {
                if let Some(u) = self.squad.get_mut(unit as usize) {
                    u.named = named;
                    if let Some(id) = class {
                        u.def = id;
                    }
                }
            }
            Holder::Army(a) => {
                if let Some(a) = self.army_mut(a) {
                    a.named = named;
                    if let (Some(id), Some(t)) = (class, a.troops.get_mut(unit as usize)) {
                        t.unit = id;
                    }
                }
            }
            Holder::Building(_) => {
                if let (Some(id), Some(t)) = (class, self.troops_of(holder).and_then(|t| t.get_mut(unit as usize))) {
                    t.unit = id;
                }
            }
        }
    }

    /// Opcode 13: the unit (or every unit, the dead too, as the original walks all the
    /// records) gains the XP as it is; an AI troop banks it towards its levels like the
    /// player's units (experience.md §5).
    fn give_unit_xp(&mut self, holder: Holder, unit: Option<u8>, xp: i64) {
        let c = self.content.clone();
        if holder == Holder::Player {
            for k in 0..self.squad.len() {
                if unit.is_none_or(|n| n as usize == k) {
                    self.unit_gains(k, xp);
                }
            }
            return;
        }
        let xp = xp.clamp(0, i32::MAX as i64) as i32;
        let Some(troops) = self.troops_of(holder) else { return };
        for (k, t) in troops.iter_mut().enumerate() {
            if unit.is_none_or(|n| n as usize == k) {
                super::ai::troop_gain_xp(&c, t, xp);
            }
        }
    }

    /// Opcode 14: the unit (or every unit) holds each of the spells in one of its slots.
    fn has_spells(&self, holder: Holder, unit: Option<u8>, spells: &[u8]) -> bool {
        let holds = |slots: &[Option<SpellSlot>; SPELL_SLOTS]| spells.iter().all(|&s| slots.iter().flatten().any(|x| x.spell == s as u32));
        let pick = |k: usize| unit.is_none_or(|n| n as usize == k);
        let w = &self.world;
        let troops = match holder {
            Holder::Player => return self.squad.iter().enumerate().filter(|(k, _)| pick(*k)).all(|(_, u)| holds(&u.spells)),
            Holder::Army(a) => w.armies.iter().chain(w.inactive.iter()).find(|x| x.id == a).map(|x| &x.troops),
            Holder::Building(b) => w.locations.iter().find(|l| l.id == b).map(|l| &l.garrison),
        };
        troops.is_some_and(|t| t.iter().enumerate().filter(|(k, _)| pick(*k)).all(|(_, t)| holds(&t.spells)))
    }

    fn forget_spell(&mut self, spell: u8) {
        self.spells.retain(|&s| s != spell);
    }

    /// The hero's figure is not a model of the scenario: a recorded no-op for him.
    fn set_army_model(&mut self, holder: Holder, model: u8) {
        if let Holder::Army(a) = holder {
            if let Some(a) = self.army_mut(a) {
                // Army +0x169d, the figure itself (c28662), not the editor's byte 5.
                a.figure = model;
            }
        }
    }

    fn random(&mut self, lo: i64, hi: i64) -> i64 {
        self.event_rng.range(lo.clamp(i32::MIN as i64, i32::MAX as i64) as i32, hi.clamp(i32::MIN as i64, i32::MAX as i64) as i32) as i64
    }

    /// The army's post moves to the cell and, on the map, it sets off there (its patrol
    /// then goes on around it) *(guess)*.
    fn set_army_target(&mut self, army: ArmyId, x: i32, y: i32) {
        let to = (x, y);
        if let Some(i) = self.army_index(army) {
            let w = &self.world;
            let here = w.armies[i].tile(&w.map);
            let path = if w.map.passable(to) { super::ai::army_path(w, &w.armies[i], here, to, AI_TARGET_NODES) } else { Vec::new() };
            let a = &mut self.world.armies[i];
            a.post = to;
            a.chasing = false;
            a.path = path;
        } else if let Some(a) = self.army_mut(army) {
            a.post = to;
        }
    }

    fn army_at(&self, army: ArmyId, x: i32, y: i32) -> bool {
        self.army_index(army).is_some_and(|i| self.world.armies[i].tile(&self.world.map) == (x, y))
    }

    /// To the cell, or the nearest passable one within 8 cells (else he stays); the walk
    /// stops and the hero looks around.
    fn teleport_player(&mut self, x: i32, y: i32) {
        let map = &self.world.map;
        let to = if map.passable((x, y)) { Some((x, y)) } else { map.nearest_passable((x, y), 8) };
        let Some(t) = to else { return };
        self.pos = map.center(t);
        self.path.clear();
        self.goal = None;
        self.location = None;
        // His next step is priced on his new cell (0x497c68, from the Community's 0xc27862).
        let cost = if self.aboard() { self.world.mixed_cost(t) } else { self.world.map.cost(t).unwrap_or(0) };
        self.step_base = Some(u32::from(cost) * self.hero_speed());
        self.look_around();
    }
}

/// An AI army's units as the event engine sees them: its named character leads it (its
/// first unit); a dead unit has no hit points.
fn army_units(a: &Army) -> Vec<UnitRecord> {
    a.troops
        .iter()
        .enumerate()
        .map(|(k, t)| UnitRecord {
            unit: t.unit.0,
            named: if k == 0 { a.named } else { 0 },
            hp: if t.alive() { 1 } else { 0 },
            from_event: t.kind == WageKind::Event,
        })
        .collect()
}

/// Search limit of the path to an AI army's scripted target.
const AI_TARGET_NODES: usize = 4000;

/// What the next map of a campaign starts with ([`Game::next_map`]): the hand-over of the
/// original (0x4b5b64). It holds all the old map could hand over; the **next** map's header
/// bytes choose what is taken ([`Game::apply_carry_over`], events.md §12). A field is `None`
/// in a hand-over saved before that (which read the old map's bytes).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NextMap {
    /// The map to load: the scenario's next-map name; after an opcode 15 branch its leading
    /// "number-variant" (or the whole name) is the chosen `N-V` *(guess: the guide names maps
    /// "N-V …" and gives the next map as "0-0")*.
    pub name: String,
    pub branch: Option<(i16, i16)>,
    /// Header byte 0: the gold, which the next map's gold is set to.
    pub gold: Option<i32>,
    /// Gods' favour: Razdor's mana *(guess)*, set alike.
    pub mana: Option<i32>,
    /// Fame carries over (no code reads it).
    pub fame: bool,
    /// The hero's whole unit record, which always replaces the next map's hero: type (an
    /// event-changed class stays), level, XP, HP (wounds stay), items, drain. Its spell slots
    /// are emptied.
    pub hero: Unit,
    /// Byte 3: on, the hero keeps his level and XP and his spell book replaces the next
    /// map's; off, he starts at level 1 with no XP and the next map's book.
    pub spells: Option<Vec<u8>>,
    /// Byte 4: the hero keeps his four worn items; off, his slots are emptied.
    pub hero_items: bool,
    /// Byte 5: the pack, which replaces the next map's.
    pub inventory: Option<Vec<ItemId>>,
    /// Byte 6: the army after the hero, which replaces the next map's (its preset troops are
    /// lost); the dead are dropped there. Every unit's spell slots are emptied.
    pub army: Option<Vec<Unit>>,
    /// The flag string: it always carries over, as the next map's loading does not touch it
    /// (and the restart snapshot keeps it, 4b5ef8, 4b5ff8).
    pub flags: String,
    /// The hero's class (his preset on the next map) and name.
    pub class: HeroClass,
    pub hero_name: Option<String>,
    /// The journal's history (`rules::journal`, a Razdor extra), always carried: the next map
    /// is its next chapter. The quest journal itself is not (0x4b2204).
    pub journal: crate::rules::journal::History,
}

/// The name of branch (map, variant) for a next-map name such as "0-0 …".
pub fn branch_name(next: &str, (map, variant): (i16, i16)) -> String {
    let t = next.trim();
    let digits = |s: &str| s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let a = digits(t);
    if a > 0 && t[a..].starts_with('-') {
        let rest = &t[a + 1..];
        let b = digits(rest);
        if b > 0 {
            return format!("{map}-{variant}{}", &rest[b..]);
        }
    }
    format!("{map}-{variant}")
}

impl Game {
    /// After a scenario victory, the next campaign map (named by the scenario, or chosen by
    /// an opcode 15 branch) and what carries over to it (header 0x110). `None` before a
    /// victory or when the scenario names no next map.
    pub fn next_map(&self) -> Option<NextMap> {
        let engine = self.script()?;
        if !matches!(self.script_end(), Some(ScriptEnd::Victory(_))) {
            return None;
        }
        let branch = engine.campaign_branch();
        let name = match branch {
            Some(b) => branch_name(engine.next_map_name(), b),
            None if engine.next_map_name().trim().is_empty() => return None,
            None => engine.next_map_name().trim().to_string(),
        };
        Some(self.hand_over(name, branch))
    }

    /// All this game hands over to map `name`: the next map's header chooses what it takes.
    fn hand_over(&self, name: String, branch: Option<(i16, i16)>) -> NextMap {
        // Every unit of the army loses its lasting spells at the hand-over.
        let unspelled = |u: &Unit| Unit { spells: [None; SPELL_SLOTS], ..u.clone() };
        NextMap {
            name,
            branch,
            gold: Some(self.gold),
            mana: Some(self.mana),
            fame: true,
            hero: unspelled(self.hero()),
            spells: Some(self.spells.clone()),
            hero_items: true,
            inventory: Some(self.pack.clone()),
            army: Some(self.squad.iter().skip(1).map(unspelled).collect()),
            flags: self.script().map(|e| e.flag_string()).unwrap_or_default(),
            class: match self.archetype {
                2 => HeroClass::Archmage,
                3 => HeroClass::Ranger,
                _ => HeroClass::Knight,
            },
            hero_name: self.hero_name.clone(),
            journal: self.journal.clone(),
        }
    }

    /// The next map of a campaign, started with what the last one carries over
    /// ([`Game::apply_carry_over`]) before its opening events run: they may look for the
    /// carried army (РК2 checks the herald at once) or the last map's flags.
    pub fn from_campaign(content: std::sync::Arc<crate::rules::content::Content>, scenario: &crate::dt::dtm::Scenario, prev: &NextMap) -> Game {
        let mut g = Game::unstarted(content, scenario, prev.class);
        g.apply_carry_over(prev);
        g.carried = Some(Box::new(prev.clone()));
        g.start_script();
        g
    }

    /// "Рестарт" (0x4b5ff8): the map of this game loaded again and its restart snapshot put
    /// back: the class, the hero's army, gold, mana, book, pack and flags as the map began.
    /// On a campaign map that is what the previous map carried over, not the map's own
    /// preset; on a map started as a new game, the preset of the class it started with.
    pub fn restart(&self, content: std::sync::Arc<crate::rules::content::Content>, scenario: &crate::dt::dtm::Scenario) -> Game {
        let mut g = match &self.carried {
            Some(prev) => Game::from_campaign(content, scenario, prev),
            None => {
                let mut g = Game::from_scenario(content, scenario, self.start_class());
                g.hero_name.clone_from(&self.hero_name);
                g
            }
        };
        g.origin.clone_from(&self.origin);
        g
    }

    /// Squad member `k` gains `xp` outside battle; a new level is reported on the map.
    fn unit_gains(&mut self, k: usize, xp: i64) {
        let c = self.content.clone();
        let Some(u) = self.squad.get_mut(k) else { return };
        if u.gain_xp(&c, xp.clamp(0, i32::MAX as i64) as i32) > 0 {
            let level = u.level;
            self.effect_events.push(Event::LevelUp(k, level));
        }
    }

    /// Starts this (next) campaign map with what `prev` carries over (0x4b5b64): the old
    /// hero's whole record replaces the new hero; then, by the header bytes, the gold and mana
    /// are set to the old amounts; without byte 3 the hero is back to level 1 with no XP
    /// (with it his book replaces this map's); without byte 4 his worn slots are emptied; the
    /// old pack and the old army replace this map's (the army paid as of now, its dead
    /// dropped). The flags always carry over.
    pub fn apply_carry_over(&mut self, prev: &NextMap) {
        let c = self.content.clone();
        let now = self.clock.total_minutes() as u64;
        // This (the next) map's header bytes say what is taken (0x4b5b64 reads the map it
        // has just loaded): РК4 takes neither the pack nor the army of РК3.
        let carry = self.script.as_ref().map_or([1; 7], |e| e.carry_over()).map(|b| b != 0);
        let gold = prev.gold.filter(|_| carry[0]);
        let mana = prev.mana.filter(|_| carry[1]);
        let spells = prev.spells.as_ref().filter(|_| carry[3]);
        let hero_items = prev.hero_items && carry[4];
        let inventory = prev.inventory.as_ref().filter(|_| carry[5]);
        let army = prev.army.as_ref().filter(|_| carry[6]);
        if let Some(engine) = self.script.as_mut() {
            engine.set_flag_string(&prev.flags);
        }
        if prev.hero_name.is_some() {
            self.hero_name = prev.hero_name.clone();
        }
        self.journal = prev.journal.clone();
        self.journal.next_chapter();
        let slot = self.squad[0].slot;
        let mut hero = prev.hero.clone();
        if c.try_unit(hero.def).is_none() {
            hero.def = self.squad[0].def;
        }
        // With the old army he keeps his place among it; else he leads this map's preset.
        if army.is_none() {
            hero.slot = slot;
        }
        self.squad[0] = hero;
        if let Some(g) = gold {
            self.gold = g;
        }
        if let Some(m) = mana {
            self.mana = m;
        }
        match spells {
            Some(book) => self.spells = book.clone(),
            None => (self.squad[0].level, self.squad[0].xp) = (1, 0),
        }
        if !hero_items {
            self.squad[0].items = [None; crate::rules::items::SLOTS];
        }
        if let Some(pack) = inventory {
            self.pack = pack.clone();
        }
        if let Some(army) = army {
            self.squad.truncate(1);
            for u in army.iter().filter(|u| u.alive()) {
                let mut u = u.clone();
                u.unpaid = false;
                u.last_paid = now;
                self.squad.push(u);
            }
        }
        // His wounds stay; HP above what his record now gives him (back at level 1, his
        // items gone) is cut to it *(guess: the hand-over is not traced for it)*.
        let max = self.squad[0].max_hp(&c);
        self.squad[0].hp = self.squad[0].hp.min(max);
    }

    /// The troops of an AI army (on the map or waiting) or of a building's garrison.
    fn troops_of(&mut self, holder: Holder) -> Option<&mut Vec<Troop>> {
        match holder {
            Holder::Army(a) => self.army_mut(a).map(|a| &mut a.troops),
            Holder::Building(b) => self.world.locations.iter_mut().find(|l| l.id == b).map(|l| &mut l.garrison),
            Holder::Player => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::events::Answer;
    use crate::dt::dtm::{BuildingType, Event as DtEvent, Scenario};
    use crate::rules::content::HeroClass;
    use crate::rules::world::testkit::*;
    use std::sync::Arc;

    const DAY: u16 = 1440;

    /// A once-event of `kind`, open all day every day.
    /// The opening events drained and every window read (OK), the scan going on after each.
    fn read(g: &mut Game) -> Vec<Event> {
        let mut events = g.drain_events();
        while g.script().is_some_and(|s| s.holds_window()) {
            events.extend(g.event_window_closed());
        }
        events
    }

    fn ev(kind: EventKind) -> DtEvent {
        DtEvent { kind: kind as u8, repeat: DAY, duration: DAY, once: 1, message: "m".into(), title: "t".into(), ..DtEvent::default() }
    }

    fn world(events: Vec<DtEvent>) -> Scenario {
        let mut s = scenario(16, 12);
        s.header.heroes[0] = hero(2, 2, 100, &[troop(4, 0, 1)]);
        s.events = events;
        s
    }

    fn point(id: u8, x: u16, y: u16, radius: u8) -> crate::dt::dtm::Point {
        crate::dt::dtm::Point {
            x,
            y,
            id,
            model: 9,
            serial: id as u16,
            event_slots: [0; 10],
            priorities: [0; 4],
            active_duration: 0,
            radius,
            event_count: 0,
            active: 0,
            unknown_41: [0; 58],
        }
    }

    fn start(s: &Scenario) -> Game {
        Game::from_scenario(Arc::new(content()), s, HeroClass::Knight)
    }

    fn fired(events: &[Event]) -> Vec<EventId> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Script(EventOutcome::Fired { event, .. }) => Some(*event),
                _ => None,
            })
            .collect()
    }

    fn walk(g: &mut Game) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..10_000 {
            if !g.moving() {
                break;
            }
            events.extend(g.tick(0.05));
        }
        events
    }

    #[test]
    fn an_event_casts_its_spell_on_the_army_through_the_world_spell_path() {
        use crate::rules::content::{testkit as ck, Content, Stat, StatMods};
        let mut e = ev(EventKind::Global);
        e.results.cast_spell = 1;
        let s = world(vec![e]);
        let base = content();
        // Spell 1: +10 hits at once, +2 initiative for 5 hours.
        let spell = crate::rules::content::SpellDef { time_work: Some(5), add: StatMods::from([(Stat::Initiative, 2)]), ..ck::spell(1, 0) };
        let c = Content::new(base.units.clone(), base.items.clone(), vec![spell], base.options.clone(), base.formation);
        let mut g = Game::from_scenario(Arc::new(c), &s, HeroClass::Knight);
        let now = g.clock.total_minutes() as u64;
        assert_eq!(fired(&g.drain_events()), vec![1]);
        // An event's spell lasts TimeWork × 10 (5 h → 50 h).
        assert_eq!(g.active_spells(), vec![SpellSlot { spell: 1, until: now + 50 * 60 }]);
        let plain = crate::rules::units::Stats::of_level(&g.content, g.squad[1].def, g.squad[1].level)[Stat::Initiative];
        assert_eq!(g.squad[1].stats(&g.content)[Stat::Initiative], plain + 2);
        g.wait(5);
        assert_eq!(g.active_spells().len(), 1, "longer than a cast of it");
        g.wait(45);
        assert!(g.active_spells().is_empty());
    }

    #[test]
    fn army_strength_counts_the_dead_as_the_original() {
        let s = world(vec![]);
        let mut g = start(&s);
        let c = g.content.clone();
        let all: i64 = g.squad.iter().map(|u| u.tactical(&c, 0) as i64).sum();
        assert_eq!(g.army_strength(), all);
        // A dead unit's value is added before its HP is tested (0x4a182b).
        g.squad[1].hp = 0;
        assert_eq!(g.army_strength(), all);
    }

    #[test]
    fn opening_events_change_the_game() {
        let mut e = ev(EventKind::Global);
        let r = &mut e.results;
        (r.gold, r.mana, r.experience) = (50, 7, 10);
        r.units_add = [5, 0, 0, 0];
        r.artifacts_add = [7, 0, 0, 0];
        r.activate_armies = [3, 0];
        r.spells_learned = [4, 0, 0, 0];
        let mut s = world(vec![e]);
        let mut sleeper = army(3, 10, 8, -2, &[troop(4, 0, 2)]);
        sleeper.inactive = 1;
        s.armies = vec![sleeper];
        let mut g = start(&s);
        let events = g.drain_events();
        assert_eq!(fired(&events), vec![1]);
        assert!(events.iter().any(Event::needs_reading));
        assert_eq!((g.gold, g.mana), (150, 7));
        assert_eq!(g.squad.len(), 3, "hero, the preset's unit and the one the event adds");
        assert!(g.squad[2].from_event && g.squad[2].def == UnitId(5));
        assert_eq!(g.pack, vec![ItemId(7)]);
        assert_eq!(g.spells, vec![4]);
        assert!(g.world.armies.iter().any(|a| a.id == 3) && g.world.inactive.is_empty());
        assert!(g.drain_events().is_empty());
    }

    #[test]
    fn an_activated_army_comes_in_its_place_alive_paid_and_with_wander_points() {
        // 0x4969b8: its units revived and paid, no path, its home under it its own, its pairs
        // dirty, four wander points drawn; it acts in its index order among the armies.
        let mut s = world(Vec::new());
        let mut home = building(BuildingType::Village, 10, 8, (1, 1));
        home.faction = 2;
        s.buildings = vec![home];
        let mut sleeper = army(3, 10, 8, -2, &[troop(4, 0, 2)]);
        sleeper.inactive = 1;
        sleeper.home_building = 1;
        s.armies = vec![army(1, 2, 9, 0, &[troop(4, 0, 1)]), sleeper, army(5, 14, 2, 0, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        read(&mut g);
        let k = g.world.inactive.iter().position(|a| a.id == 3).unwrap();
        g.world.inactive[k].troops[0].died_at = Some(1);
        g.world.inactive[k].troops[0].hurt = 5;
        g.world.inactive[k].troops[1].unpaid = true;
        g.world.armies[0].mind.clean.insert(3);
        let draws = g.rng.clone();
        EventWorld::activate_army(&mut g, 3);
        assert_eq!(g.world.armies.iter().map(|a| a.id).collect::<Vec<_>>(), [1, 3, 5]);
        let a = &g.world.armies[1];
        assert!(a.troops.iter().all(|t| t.alive() && t.hurt == 0 && !t.unpaid));
        assert!(a.mind.no_path && a.mind.free_step && a.mind.standing == Some(0), "no path, its first step free");
        assert_ne!(a.mind.wander, [(0, 0); 4]);
        assert_ne!(g.rng.state(), draws.state(), "the wander points drawn");
        assert!(!g.world.armies[0].mind.clean.contains(&3), "its pairs dirty");
        assert_eq!(g.world.locations[0].owner, crate::rules::world::Owner::Army(3));
    }

    #[test]
    fn an_event_brings_back_a_beaten_army_and_stops_one_from_respawning() {
        // 0x4969b8 brings back any army off the map, a destroyed one too (its marks cleared);
        // 0x496900 on a destroyed army clears that flag, so it never respawns by itself.
        let mut s = world(Vec::new());
        s.armies = vec![army(3, 10, 8, -2, &[troop(4, 0, 2)])];
        let mut g = start(&s);
        Game::army_beaten(&mut g, 0, crate::rules::ai::Beaten::ByAi);
        assert!(EventWorld::army_beaten(&g, 3) && g.world.armies.is_empty(), "no home: it stays destroyed");
        EventWorld::activate_army(&mut g, 3);
        assert!(g.world.armies.iter().any(|a| a.id == 3) && g.world.respawns.is_empty());
        assert!(!EventWorld::army_beaten(&g, 3), "its mark cleared");
        Game::army_beaten(&mut g, 0, crate::rules::ai::Beaten::ByPlayer);
        g.world.respawns[0].due = 0.0;
        EventWorld::deactivate_army(&mut g, 3);
        assert!(g.world.respawns.is_empty() && g.world.inactive.iter().any(|a| a.id == 3));
        assert!(EventWorld::player_defeated(&g, 3), "the mark stays");
    }

    #[test]
    fn an_events_spell_lands_after_its_other_results() {
        // 0x4ab1ec queues the spell with the results and casts it in the event's finish: the
        // unit the event adds holds it too.
        use crate::rules::content::{testkit as ck, Content};
        let mut e = ev(EventKind::Global);
        e.results.cast_spell = 1;
        e.results.units_add[0] = 4;
        let s = world(vec![e]);
        let base = content();
        let spell = crate::rules::content::SpellDef { time_work: Some(5), ..ck::spell(1, 0) };
        let c = Content::new(base.units.clone(), base.items.clone(), vec![spell], base.options.clone(), base.formation);
        let mut g = Game::from_scenario(Arc::new(c), &s, HeroClass::Knight);
        assert_eq!(fired(&g.drain_events()), vec![1]);
        let added = g.squad.last().unwrap();
        assert!(added.from_event && added.spells[0].is_some_and(|x| x.spell == 1), "{:?}", added.spells);
    }

    #[test]
    fn an_event_that_fires_during_a_cast_lands_the_spell_at_once() {
        // 0x4ae4f2: an event fired by the scan of a casting step pops the wait off the
        // queue, and the spell lands at once: after 1 h here, not after the 4 h of its cast.
        use crate::rules::content::{testkit as ck, Content, SpellDef};
        use crate::rules::magic::{CastOutcome, CastTarget};
        let mut e = ev(EventKind::Global);
        e.start_time = 624_354_300 + 60;
        let s = world(vec![e]);
        let base = content();
        let heal = SpellDef { time_cast: Some(4), cost_mana: 10, delta_fixed_hits: Some(10), ..ck::spell(1, 0) };
        let c = Content::new(base.units.clone(), base.items.clone(), vec![heal], base.options.clone(), base.formation);
        let mut g = Game::from_scenario(Arc::new(c), &s, HeroClass::Knight);
        read(&mut g);
        (g.mana, g.spells) = (100, vec![1]);
        g.squad[1].hp = 5;
        let t0 = g.clock.total_minutes();
        let cast = g.cast(1, CastTarget::Own).unwrap();
        assert_eq!(fired(&cast.events), vec![1]);
        assert_eq!(cast.outcome, CastOutcome::Done { hits: 10, killed: 0, destroyed: false });
        assert_eq!((g.clock.total_minutes() - t0, g.mana, g.squad[1].hp), (60.0, 90, 15));
    }

    #[test]
    fn an_event_teaches_spells_past_the_15th() {
        // 0x49c144 has no limit; the 16th is known but not castable (`rules::magic`).
        use crate::rules::events::EventWorld;
        let mut g = start(&world(vec![]));
        g.spells = (20..35).collect();
        EventWorld::learn_spell(&mut g, 3);
        assert_eq!(g.spells.len(), 16);
        EventWorld::learn_spell(&mut g, 3);
        assert_eq!(g.spells.len(), 16, "known: not added twice");
    }

    #[test]
    fn events_take_units_and_items_and_armies_away() {
        let mut give = ev(EventKind::Global);
        (give.results.units_add, give.results.units_add_named) = ([5, 4, 0, 0], [1, 0, 0, 0]);
        give.results.artifacts_add = [7, 0, 0, 0];
        let mut take = ev(EventKind::Global);
        take.start_time = 624_354_300 + 60;
        // 0xFE: the last unnamed event unit (not the Aide); then the Aide by his name.
        (take.results.units_remove, take.results.units_remove_named) = ([0xFE, 9, 0, 0], [0, 1, 0, 0]);
        take.results.removed_units_to_army = 2;
        take.results.artifacts_remove = [7, 0, 0, 0];
        take.results.deactivate_army = 2;
        let mut s = world(vec![give, take]);
        s.named_characters = vec![crate::dt::dtm::NamedCharacter { unit: 5, name: "Aide".into() }];
        s.armies = vec![army(2, 12, 10, 1, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        read(&mut g);
        assert_eq!(g.unit_label(&g.squad[2]), "Aide");
        assert_eq!(g.squad.len(), 4);
        g.wait(1);
        assert_eq!(g.squad.len(), 2, "the units the event added leave again");
        assert!(g.pack.is_empty());
        assert!(g.world.armies.is_empty());
        let a = &g.world.inactive[0];
        assert_eq!((a.id, a.named), (2, 1), "it joined army 2, now waiting");
        assert_eq!(a.troops.iter().map(|t| t.unit).collect::<Vec<_>>(), [UnitId(5), UnitId(4), UnitId(4)], "the named one leads it");
        let faction = a.faction;
        let units = g.faction_units(faction);
        assert!(units.iter().any(|u| u[0].named == 1), "the character is with the faction's army: {units:?}");
    }

    /// The original's removal (0x496310) only deletes the record: a unit removed with no
    /// army to go to takes its worn items with it. Given to an army, it leads that army when
    /// the slot names a character, even an unnamed unit (type 255 under a named slot).
    #[test]
    fn a_removed_unit_keeps_its_items_and_leads_by_the_slot_name() {
        let mut s = world(vec![]);
        s.armies = vec![army(2, 12, 10, 1, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        read(&mut g);
        g.squad[1].items[0] = Some(ItemId(7));
        EventWorld::remove_unit(&mut g, 1, false, None);
        assert!(g.pack.is_empty(), "the items went with the unit");
        let c = g.content.clone();
        let taken: Vec<_> = g.squad.iter().map(|u| u.slot).collect();
        let mut u = Unit::new(&c, UnitId(5), c.formation.new_unit_slot(&taken).unwrap());
        u.named = 0;
        g.squad.push(u);
        let last = g.squad.len() - 1;
        EventWorld::remove_unit(&mut g, last, true, Some(2));
        let a = g.world.armies.iter().find(|a| a.id == 2).unwrap();
        assert_eq!(a.troops.iter().map(|t| t.unit).collect::<Vec<_>>(), [UnitId(5), UnitId(4)], "it leads");
    }

    /// A full army (0x4a8fe5): the unit with the lowest level value after the hero is
    /// dismissed (its items to the pack), the first of a tie; then the event's unit joins.
    /// Taken from an army, the unit brings its whole record and an emptied army leaves.
    #[test]
    fn a_unit_joining_a_full_army_dismisses_the_weakest() {
        let mut join = ev(EventKind::Global);
        join.results.units_add = [5, 0, 0, 0];
        let mut s = world(vec![join]);
        s.armies = vec![army(2, 12, 10, 1, &[troop(5, 0, 1)])];
        let mut g = start(&world(vec![]));
        let c = g.content.clone();
        while g.squad.len() < g.max_squad() {
            let taken: Vec<_> = g.squad.iter().map(|u| u.slot).collect();
            let slot = c.formation.new_unit_slot(&taken).unwrap();
            g.squad.push(Unit::new(&c, UnitId(1), slot));
        }
        // Warriors of type 1; two weaker ones of type 4.
        g.squad[1].def = UnitId(1);
        let weak = 4;
        (g.squad[weak].def, g.squad[weak + 1].def) = (UnitId(4), UnitId(4));
        g.squad[weak].items[0] = Some(ItemId(7));
        let kept = g.squad[weak + 1].clone();
        EventWorld::add_unit(&mut g, 5, 0, None);
        assert_eq!(g.squad.len(), g.max_squad());
        assert_eq!(g.squad[weak].slot, kept.slot, "the first of the two weakest left");
        assert_eq!(g.pack, vec![ItemId(7)]);
        assert_eq!((g.squad.last().unwrap().def, g.squad.last().unwrap().level), (UnitId(5), 1));

        let mut g = start(&s);
        read(&mut g);
        let k = g.world.armies.iter().position(|a| a.id == 2).unwrap();
        (g.world.armies[k].troops[0].level, g.world.armies[k].troops[0].xp) = (3, 9);
        EventWorld::add_unit(&mut g, 5, 0, Some(2));
        let u = g.squad.last().unwrap();
        assert_eq!((u.def, u.level, u.xp), (UnitId(5), 3, 9), "the army's unit, its whole record");
        assert!(g.world.armies.iter().all(|a| a.id != 2) && g.world.inactive.iter().any(|a| a.id == 2), "emptied: off the map");
    }

    #[test]
    fn event_gold_stops_at_0_and_the_world_answers_the_conditions() {
        let mut g = start(&world(vec![]));
        EventWorld::add_gold(&mut g, -700);
        assert_eq!(g.gold, 0, "a rumour dearer than his purse leaves him at 0 (0x4ab1be)");
        g.squad[1].hp = 0;
        assert_eq!(EventWorld::squad_count(&g), 2, "the dead count");
        g.squad[0].hp = 1;
        assert_eq!(EventWorld::hero_hp(&g), 1);
    }

    /// Army conditions: inactive is off the map and not destroyed; at home is standing in
    /// the home building (no home passes).
    #[test]
    fn army_conditions_read_the_army_record() {
        let mut s = world(Vec::new());
        s.buildings = vec![building(BuildingType::Village, 10, 8, (1, 1))];
        let mut homed = army(3, 10, 8, -2, &[troop(4, 0, 2)]);
        homed.home_building = 1;
        s.armies = vec![homed, army(4, 14, 2, -2, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        read(&mut g);
        assert!(EventWorld::army_at_home(&g, 4), "no home");
        let k = g.world.armies.iter().position(|a| a.id == 3).unwrap();
        g.world.armies[k].mind.standing = Some(0);
        assert!(EventWorld::army_at_home(&g, 3));
        g.world.armies[k].mind.standing = None;
        assert!(!EventWorld::army_at_home(&g, 3));
        EventWorld::deactivate_army(&mut g, 4);
        assert!(EventWorld::army_inactive(&g, 4) && !EventWorld::army_active(&g, 4));
        let k = g.world.armies.iter().position(|a| a.id == 3).unwrap();
        Game::army_beaten(&mut g, k, crate::rules::ai::Beaten::ByPlayer);
        assert!(!EventWorld::army_inactive(&g, 3) && !EventWorld::army_active(&g, 3), "destroyed: neither");
    }

    #[test]
    fn the_hero_keeps_his_starting_class_whatever_unit_he_becomes() {
        // World.md §2.1 (0x4b4300): sight, speed and the cast divisor are set from the class
        // at the start; an event that changes his unit does not change them. A Community
        // speed event (0xc279e6) sets his speed itself.
        use crate::rules::events::EventWorld as _;
        let mut g = start(&world(vec![]));
        assert_eq!((g.hero_speed(), g.sight_radius()), (5, 9));
        g.set_hero_class(3);
        assert_eq!(g.hero_class(), Some(HeroClass::Ranger), "his unit is the ranger's now");
        assert_eq!((g.start_class(), g.hero_speed(), g.sight_radius()), (HeroClass::Knight, 5, 9));
        assert_eq!(g.step_time((2, 2), (3, 2)), 25.0);
        g.set_army_speed(Holder::Player, -3);
        assert_eq!((g.hero_speed(), g.step_time((2, 2), (3, 2))), (8, 40.0));
    }

    #[test]
    fn a_local_event_fires_on_entering_its_building() {
        let mut e = ev(EventKind::Local);
        e.results.gold = 25;
        let mut s = world(vec![e]);
        s.header.heroes[0] = hero(2, 2, 100, &[]); // no wages on the way
        let mut town = building(BuildingType::Town, 9, 2, (1, 1));
        town.event_slots[0] = 1;
        town.event_count = 1;
        s.buildings = vec![town];
        let mut g = start(&s);
        assert!(fired(&g.drain_events()).is_empty(), "not at the start: the hero is elsewhere");
        assert!(g.set_destination((9, 2)));
        let events = walk(&mut g);
        assert_eq!(fired(&events), vec![1]);
        assert_eq!(g.gold, 125);
        // Its window open, the town is entered only once it is read (0x4ed42c).
        assert!(!events.contains(&Event::Arrived(0)));
        assert_eq!(g.enter_waiting_building(), vec![Event::Arrived(0)]);
        assert!(g.enter_waiting_building().is_empty(), "once");
    }

    #[test]
    fn a_village_entered_as_an_event_opens_waits_for_it_to_be_read() {
        // 0x4ae6dc: the arrival's event scan opens the event's window; the village he
        // clicked waits (0x4ed42c) and is entered when the window is read (0x4bbc84): only
        // then the offer rolls and the tribute.
        let e = ev(EventKind::Local);
        let mut s = world(vec![e]);
        s.header.heroes[0] = hero(2, 2, 100, &[]);
        let mut v = building(BuildingType::Village, 9, 2, (1, 1));
        (v.gold_per_day, v.gold_max, v.relations) = (25, 60, [1, 0, 0, 0]);
        v.event_slots[0] = 1;
        v.event_count = 1;
        s.buildings = vec![v];
        let mut g = start(&s);
        read(&mut g);
        assert!(g.set_destination((9, 2)));
        crate::rules::rng::trace::start();
        let events = walk(&mut g);
        let rolls = |d: &[crate::rules::rng::trace::Draw]| d.iter().filter(|d| d.site.file().ends_with("economy.rs")).count();
        assert_eq!(fired(&events), vec![1]);
        assert!(!events.iter().any(|e| matches!(e, Event::Arrived(_) | Event::Tribute { .. })), "{events:?}");
        assert_eq!((g.gold, rolls(&crate::rules::rng::trace::take())), (100, 0), "no tribute, no offer rolls yet");
        let entered = g.enter_waiting_building();
        let draws = crate::rules::rng::trace::take();
        crate::rules::rng::trace::stop();
        assert_eq!(entered.first(), Some(&Event::Arrived(0)));
        assert!(rolls(&draws) > 0, "the offer rolls now");
        assert!(g.village_offer().is_some() || g.gold == 125, "the offer, or the tribute");
    }

    #[test]
    fn a_point_event_stops_the_walk_to_be_read() {
        let e = ev(EventKind::Local);
        let mut s = world(vec![e]);
        let mut p = point(4, 6, 2, 0);
        p.event_slots[0] = 1;
        p.event_count = 1;
        s.points = vec![p];
        let mut g = start(&s);
        read(&mut g);
        assert!(g.set_destination((11, 2)));
        let events = walk(&mut g);
        assert_eq!(fired(&events), vec![1]);
        assert_eq!(g.tile(), (6, 2), "stopped on the point");
        assert!(!g.moving());
    }

    #[test]
    fn a_question_is_answered_through_the_game() {
        let mut ask = ev(EventKind::Global);
        ask.conditions.confirm_question = 1;
        ask.question = "q".into();
        ask.results.gold = -40;
        ask.results.artifacts_add = [9, 0, 0, 0];
        let mut s = world(vec![ask.clone()]);
        s.armies = vec![army(1, 14, 10, 0, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        assert_eq!(g.drain_events(), vec![Event::Script(EventOutcome::Question(1))]);
        assert_eq!(g.pending_question(), Some(1));
        g.world.armies[0].mind.clean.insert(crate::rules::ai::HERO);
        let events = g.answer_question(true);
        assert_eq!(fired(&events), vec![1]);
        assert_eq!((g.gold, g.pack.clone()), (60, vec![ItemId(9)]));
        assert_eq!(g.pending_question(), None);
        // The event took effect: the AI rescores the hero (0x4ab1ec → 0x497240(0, 1)).
        assert!(!g.world.armies[0].mind.clean.contains(&crate::rules::ai::HERO));

        let mut g = start(&s);
        read(&mut g);
        g.world.armies[0].mind.clean.insert(crate::rules::ai::HERO);
        let events = g.answer_question(false);
        assert!(g.world.armies[0].mind.clean.contains(&crate::rules::ai::HERO), "declined: no effect");
        assert_eq!(events, vec![Event::Script(EventOutcome::Declined(1))]);
        assert_eq!(g.gold, 100);
        assert_eq!(g.script().unwrap().happened(1), Some(Answer::No));
    }

    #[test]
    fn hearing_a_rumour_is_free_its_event_sets_any_cost() {
        let mut rumour = ev(EventKind::Rumour);
        rumour.results.mana = 3;
        rumour.results.gold = -4; // the rumour's own price, from its event
        let mut quest = ev(EventKind::Quest);
        quest.title = "q%+Q".into();
        quest.flags = crate::dt::dtm::FlagScript::from_title(&quest.title);
        let mut s = world(vec![rumour, quest]);
        s.header.heroes[0] = hero(9, 3, 15, &[]);
        let mut town = building(BuildingType::Town, 9, 2, (1, 1));
        town.event_slots[..2].copy_from_slice(&[1, 2]);
        town.event_count = 2;
        s.buildings = vec![town];
        let mut g = start(&s);
        g.set_destination((9, 2));
        walk(&mut g);
        assert_eq!(g.location, Some(0));
        assert_eq!(g.hall_entries(), vec![HallEntry::Rumour(1), HallEntry::Quest(2)], "the quest waits in the hall");
        let events = g.take_hall_entry(1).unwrap();
        assert_eq!(fired(&events), vec![1]);
        assert_eq!((g.gold, g.mana), (11, 3), "no flat price: only the event's own -4 gold");
        assert_eq!(g.take_hall_entry(1), Err(ServiceError::NotHere), "heard");
        assert_eq!(g.hall_entries(), vec![HallEntry::Quest(2)]);
    }

    #[test]
    fn the_journal_history_records_quests_rumours_and_messages_with_dates() {
        use crate::rules::journal::{EntryKind, Tab};
        let mut rumour = ev(EventKind::Rumour);
        (rumour.title, rumour.message) = ("Word in the inn".into(), "The mill is haunted.".into());
        let mut quest = ev(EventKind::Quest);
        (quest.title, quest.message) = ("The mill".into(), "Free the mill, #HERONAME.".into());
        quest.results.chained_event = 3;
        let mut done = ev(EventKind::Global);
        (done.title, done.message) = ("Freed".into(), "The miller thanks you.".into());
        done.results.completes_quest = 2;
        done.subordinate = 1; // only through the quest's chain
        let mut silent = ev(EventKind::Global);
        silent.message = String::new();
        let mut hello = ev(EventKind::Global);
        (hello.title, hello.message) = ("Dawn".into(), "Hello, #HERONAME.".into());
        let mut s = world(vec![rumour, quest, done, silent, hello]);
        s.header.heroes[0] = hero(9, 3, 15, &[]);
        let mut town = building(BuildingType::Town, 9, 2, (1, 1));
        town.event_slots[..2].copy_from_slice(&[1, 2]);
        town.event_count = 2;
        s.buildings = vec![town];
        let mut g = start(&s);
        read(&mut g);
        g.set_hero_name("Ivan");
        let opened = g.clock.total_minutes() as u64;
        let messages = g.journal_rows(Tab::Messages);
        assert!(messages.iter().any(|r| r.title == "Dawn" && r.text == "Hello, Ivan."), "the opening message, the name filled in when shown: {messages:?}");
        assert!(g.journal.entries.iter().all(|e| e.event != 4), "a silent event is not recorded");
        assert!(messages.iter().all(|r| r.date.is_some_and(|d| d.total_minutes() as u64 == opened)));

        g.set_destination((9, 2));
        walk(&mut g);
        assert!(g.journal.find(EntryKind::Quest, 2).is_none(), "a building's quest is taken in its hall");
        g.take_hall_entry(2).unwrap();
        read(&mut g);
        let arrived = g.clock.total_minutes() as u64;
        assert!(arrived > opened);
        assert_eq!(g.journal.find(EntryKind::Quest, 2).map(|e| (e.minutes, e.title.as_str())), Some((arrived, "The mill")));
        assert_eq!(g.journal.find(EntryKind::Completed, 2).map(|e| e.minutes), Some(arrived), "the chained event completed it");
        assert!(g.journal_rows(Tab::Active).is_empty());
        let completed = g.journal_rows(Tab::Completed);
        assert_eq!(completed.len(), 1);
        assert_eq!((completed[0].title.as_str(), completed[0].text.as_str()), ("The mill", "Free the mill, Ivan."));
        assert_eq!(g.journal_rows(Tab::Messages)[0].title, "Freed", "newest first");

        g.take_hall_entry(1).unwrap();
        let rumours = g.journal_rows(Tab::Rumours);
        assert_eq!(rumours.len(), 1);
        assert_eq!((rumours[0].title.as_str(), rumours[0].text.as_str()), ("Word in the inn", "The mill is haunted."));
        assert!(g.journal_rows(Tab::Messages).iter().all(|r| r.title != "The mill"), "a quest is not a message too");
    }

    #[test]
    fn active_quests_come_from_the_engine_even_without_history() {
        use crate::rules::journal::Tab;
        let mut quest = ev(EventKind::Quest);
        (quest.title, quest.message) = ("Old quest".into(), "From an old save.".into());
        let mut s = world(vec![quest]);
        s.header.heroes[0] = hero(9, 3, 15, &[]);
        let mut town = building(BuildingType::Town, 9, 2, (1, 1));
        town.event_slots[0] = 1;
        town.event_count = 1;
        s.buildings = vec![town];
        let mut g = start(&s);
        g.set_destination((9, 2));
        walk(&mut g);
        g.take_hall_entry(1).unwrap();
        assert_eq!(g.journal_rows(Tab::Active).len(), 1);
        g.journal = Default::default(); // a save from before the history
        let rows = g.journal_rows(Tab::Active);
        assert_eq!((rows[0].title.as_str(), rows[0].text.as_str(), rows[0].date), ("Old quest", "From an old save.", None));
    }

    #[test]
    fn the_history_carries_over_to_the_next_map_as_a_new_chapter() {
        let mut g = start(&world(vec![]));
        g.journal.record(crate::rules::journal::EntryKind::Message, 1, 0, "t", "x");
        let next = NextMap {
            name: "Road".into(),
            branch: None,
            gold: None,
            mana: None,
            fame: false,
            hero: g.squad[0].clone(),
            spells: None,
            hero_items: false,
            inventory: None,
            army: None,
            flags: String::new(),
            class: HeroClass::Knight,
            hero_name: None,
            journal: g.journal.clone(),
        };
        let mut fresh = start(&world(vec![]));
        fresh.apply_carry_over(&next);
        assert_eq!(fresh.journal.entries.len(), 1);
        assert_eq!(fresh.journal.chapter, 1);
    }

    #[test]
    fn a_free_rumour_needs_no_gold() {
        let mut s = world(vec![ev(EventKind::Rumour)]);
        s.header.heroes[0] = hero(9, 3, 0, &[]);
        let mut town = building(BuildingType::Town, 9, 2, (1, 1));
        town.event_slots[0] = 1;
        town.event_count = 1;
        s.buildings = vec![town];
        let mut g = start(&s);
        g.set_destination((9, 2));
        walk(&mut g);
        assert_eq!(g.gold, 0);
        let events = g.take_hall_entry(1).unwrap();
        assert_eq!(fired(&events), vec![1]);
        assert_eq!(g.gold, 0);
    }

    #[test]
    fn victory_and_defeat_events_end_the_game() {
        let mut win = ev(EventKind::Global);
        (win.conditions.defeated_check, win.conditions.defeated_armies) = (1, [2, 0]);
        let mut s = world(vec![win]);
        s.header.victory_event = 1;
        s.armies = vec![army(2, 3, 2, -2, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        read(&mut g);
        assert_eq!(g.script_end(), None);
        // He walks into the hostile army next door; the player wins.
        assert!(g.set_destination((3, 2)));
        let events = walk(&mut g);
        assert!(matches!(events.last(), Some(Event::Encounter(0))), "{events:?}");
        let mut b = g.start_battle();
        for f in b.fighters.iter_mut().filter(|f| f.team == crate::rules::battle::Team::Enemy) {
            f.hp = 0;
        }
        assert!(matches!(g.resolve_battle(&b), crate::rules::game::BattleResult::Victory { .. }));
        let events = g.drain_events();
        assert!(events.contains(&Event::Script(EventOutcome::Victory(1))));
        assert_eq!(g.script_end(), Some(ScriptEnd::Victory(1)));

        let mut lose = ev(EventKind::Global);
        lose.start_time = 624_354_300 + 120;
        let mut s = world(vec![lose]);
        s.header.defeat_event = 1;
        let mut g = start(&s);
        g.wait(4);
        assert_eq!(g.script_end(), Some(ScriptEnd::Defeat(1)));
    }

    #[test]
    fn an_event_can_start_a_battle_and_delay_the_player() {
        let mut e = ev(EventKind::Global);
        e.results.start_battle_with = 5;
        e.results.delay_hours = 15;
        let mut s = world(vec![e]);
        // 22:00: the delay runs into the next day's noon, the hero's first.
        s.header.start_time = 624_354_300 - 11 * 60;
        let mut sleeper = army(5, 12, 10, -2, &[troop(4, 0, 1)]);
        sleeper.inactive = 1;
        s.armies = vec![sleeper];
        let mut g = start(&s);
        let events = g.drain_events();
        assert!(events.iter().any(|e| matches!(e, Event::NewDay(_))), "the delay crosses noon: {events:?}");
        assert_eq!(events.last(), Some(&Event::Encounter(0)));
        assert_eq!(g.foe, Some(Foe::Army(0)));
        assert_eq!(g.world.armies[0].id, 5);
    }

    #[test]
    fn a_shown_place_lists_only_the_cells_it_uncovered() {
        let mut g = start(&world(vec![]));
        g.fog = crate::rules::fog::Fog::new(g.fog.w, g.fog.h);
        g.fog.reveal(10, 4, 1);
        g.shown.clear();
        g.reveal_area((10, 4), 3);
        let first = g.shown.pop().unwrap();
        assert_eq!(first.at, (10, 4));
        assert!(!first.cells.is_empty() && first.cells.iter().all(|&t| g.fog.explored(t)));
        assert!(!first.cells.contains(&(10, 4)), "the cell already seen is not faded in again");
        g.reveal_area((10, 4), 3);
        assert!(g.shown.pop().unwrap().cells.is_empty(), "nothing new the second time");
    }

    #[test]
    fn lanterns_are_recorded_for_the_fog() {
        let mut e = ev(EventKind::Global);
        e.results.light_lanterns = [7, 0, 0, 0];
        let mut s = world(vec![e]);
        s.points = vec![point(7, 10, 4, 6)];
        let g = start(&s);
        assert_eq!(g.pending_reveals, vec![(10, 4, 6)]);
        let shown = g.shown.last().expect("the lantern is shown on the map");
        assert_eq!(shown.at, (10, 4));
        assert!(shown.cells.iter().all(|&t| g.fog.explored(t)), "only cells it uncovered, all lit now");
        assert!(!g.fog.enabled || g.fog.explored((10, 4)), "the lantern lights the fog");
    }

    #[test]
    fn an_event_on_stepping_onto_an_army_takes_the_place_of_the_battle() {
        // World.md §4.2 e (0x4ad94c): the events run with the army met; if one fires, no
        // battle, and the army's talk counter towards him drops to −500.
        let mut talk = ev(EventKind::Global);
        talk.conditions.meet_army = 2;
        let mut s = world(vec![talk]);
        let mut guard = army(2, 6, 2, -2, &[troop(4, 0, 1)]);
        (guard.patrols, guard.patrol_radius) = (1, 0);
        s.armies = vec![guard];
        let mut g = start(&s);
        read(&mut g);
        assert!(g.set_destination((6, 2)));
        let events = walk(&mut g);
        assert_eq!(fired(&events), vec![1]);
        assert!(!events.iter().any(|e| matches!(e, Event::Encounter(_))), "{events:?}");
        assert_eq!((g.foe, g.tile(), g.world.armies[0].talk), (None, (5, 2), -500));
        // Without an event: the battle.
        let mut s = world(vec![]);
        s.armies = vec![army(2, 6, 2, -2, &[troop(4, 0, 1)])];
        s.armies[0].patrols = 1;
        let mut g = start(&s);
        g.world.armies[0].patrol_radius = 0;
        assert!(g.set_destination((6, 2)));
        let events = walk(&mut g);
        assert_eq!(events.last(), Some(&Event::Encounter(0)));
        assert_eq!(g.foe, Some(Foe::Army(0)));
    }

    #[test]
    fn an_army_with_a_meeting_waiting_next_to_the_hero_closes_his_route() {
        // World.md §1.3 (0x4cc601): an army a meeting event waits for closes its cell when it
        // stands next to the hero (any of the 8 neighbours), unless it is the army clicked;
        // further away, or without the event, it is walked through like any moving army.
        let mut talk = ev(EventKind::Global);
        talk.conditions.meet_army = 2;
        let routes = |events: Vec<DtEvent>, at: (u16, u16), to: (i32, i32)| {
            let mut s = world(events);
            let mut friend = army(2, at.0, at.1, 1, &[troop(4, 0, 1)]);
            (friend.patrols, friend.patrol_radius) = (1, 5);
            s.armies = vec![friend];
            let mut g = start(&s);
            read(&mut g);
            g.fog = crate::rules::fog::Fog::disabled(16, 12);
            (g.route_to(to), g.route_to(g.world.armies[0].tile(&g.world.map)))
        };
        let (route, clicked) = routes(vec![talk.clone()], (3, 2), (4, 2));
        assert!(!route.is_empty() && !route.contains(&(3, 2)), "{route:?}");
        assert_eq!(clicked, vec![(3, 2)], "the army clicked stays open");
        let (route, _) = routes(vec![talk.clone()], (3, 3), (4, 4));
        assert!(!route.contains(&(3, 3)), "a diagonal neighbour too: {route:?}");
        let (route, _) = routes(vec![talk], (4, 2), (6, 2));
        assert!(route.contains(&(4, 2)), "two cells away: {route:?}");
        let (route, _) = routes(vec![], (3, 2), (4, 2));
        assert_eq!(route, vec![(3, 2), (4, 2)], "no meeting waits for it");
    }

    /// The hero walks from (2, 2) to (8, 4) while army 2 (`attitude`) steps from (5, 2) to
    /// (4, 2), next to him after his first step, to (3, 3); an event fires on meeting it.
    fn walk_past_army_with_event(attitude: i8) -> (Game, Vec<Event>) {
        let mut talk = ev(EventKind::Global);
        talk.conditions.meet_army = 2;
        let mut s = world(vec![talk]);
        s.armies = vec![army(2, 5, 2, attitude, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        read(&mut g);
        let a = &mut g.world.armies[0];
        a.mind.scripted = true;
        a.path = vec![(4, 2)];
        // A hostile one's cached battle score says it wins.
        a.mind.scores.insert(super::super::ai::HERO, 1);
        assert!(g.set_destination((8, 4)));
        let events = walk(&mut g);
        (g, events)
    }

    #[test]
    fn a_greeting_whose_event_fires_stops_the_walk() {
        // 0x4ade3c: the greeting runs the events with the army met; one fired, he stops.
        let (g, events) = walk_past_army_with_event(1);
        assert_eq!(fired(&events), vec![1]);
        assert_eq!((g.tile(), g.moving(), g.world.armies[0].talk), ((3, 3), false, -500));
    }

    #[test]
    fn a_victory_met_on_a_step_is_over_before_its_window_is_on_screen() {
        // Обучающий1 ends with victory event 21, "meet army 11" (the royal herald walking up).
        // The rules end the scenario inside the step, but the screen gets the step's events,
        // the event's window among them, only once the step has played (`tick_shown`): until
        // then the game holds them, and the end must wait for that window (events.md §11:
        // the window is read, then the hand-over).
        let mut win = ev(EventKind::Global);
        win.conditions.meet_army = 2;
        let mut s = world(vec![win]);
        s.header.victory_event = 1;
        s.armies = vec![army(2, 5, 2, 1, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        read(&mut g);
        let a = &mut g.world.armies[0];
        a.mind.scripted = true;
        a.path = vec![(4, 2)];
        assert!(g.set_destination((8, 4)));
        let mut ended_unshown = false;
        for _ in 0..10_000 {
            let events = g.tick_shown(0.01);
            if fired(&events).contains(&1) {
                assert!(events.contains(&Event::Script(EventOutcome::Fired { event: 1, message: true })), "{events:?}");
                assert!(!g.holds_events());
                assert!(ended_unshown, "the scenario ended while its window was still held");
                return;
            }
            if g.script_end().is_some() {
                assert!(g.holds_events(), "over, with no window held or shown");
                ended_unshown = true;
            }
        }
        panic!("no meeting");
    }

    #[test]
    fn an_attack_whose_event_fires_brings_no_battle() {
        // 0x4ade3c: an AI attack runs the events with the attacker first; the battle opens
        // only if none fired. Unlike an army stepped onto, it keeps its talk counter.
        let (g, events) = walk_past_army_with_event(-2);
        assert_eq!(fired(&events), vec![1]);
        assert!(!events.iter().any(|e| matches!(e, Event::Encounter(_))), "{events:?}");
        assert_eq!((g.foe, g.tile(), g.moving()), (None, (3, 3), false));
        assert_ne!(g.world.armies[0].talk, super::super::game::TALKED);
    }

    #[test]
    fn a_lantern_without_a_radius_reveals_nothing() {
        // World.md §3: both lantern loops test radius > 0 (0x4ab762, 0x4b5a32).
        let mut e = ev(EventKind::Global);
        e.results.light_lanterns = [7, 0, 0, 0];
        let mut s = world(vec![e]);
        s.points = vec![point(7, 13, 9, 0)];
        let g = start(&s);
        assert!(g.pending_reveals.is_empty() && g.shown.is_empty());
        assert!(!g.fog.explored((13, 9)));
    }

    #[test]
    fn an_army_moved_to_the_hero_takes_the_cheapest_free_neighbour() {
        // World.md §4.4 (0x4980d8): the hero at (2, 2); score = cost + 50 000 on a building +
        // 100 000 when taken; the first lowest in direction order (NW, N, NE, E, SE, S, SW, W).
        use crate::rules::events::EventWorld as _;
        let mut s = world(vec![]);
        // Road to the south-east (3) is the cheapest; a village on the road east is dearer.
        set(&mut s, 3, 3, crate::dt::dtm::Surface::Road);
        set(&mut s, 3, 2, crate::dt::dtm::Surface::Road);
        s.buildings = vec![building(BuildingType::Village, 3, 2, (1, 1))];
        s.armies = vec![army(2, 12, 10, 1, &[troop(4, 0, 1)]), army(3, 14, 2, 1, &[troop(4, 0, 1)])];
        s.armies[1].inactive = 1;
        let mut g = start(&s);
        g.move_army_to_hero(2);
        let a = &g.world.armies[0];
        assert_eq!((a.tile(&g.world.map), a.post), ((3, 3), (3, 3)), "the road cell, home moved too");
        assert_eq!(a.patrol_centre(), (12, 10), "the patrol box is not recomputed");
        // Taken now: the next one goes to the first grass neighbour, NW, though it waits off
        // the map (it stays there).
        g.move_army_to_hero(3);
        let w = &g.world.inactive[0];
        assert_eq!((w.tile(&g.world.map), w.post), ((1, 1), (1, 1)));
        assert!(g.world.armies.iter().all(|a| a.id != 3), "not brought onto the map");
    }

    // --- Community Update opcodes ---------------------------------------------------------------

    /// A Community opcode event: "no meeting", patrol value `code`, resources (XP, gold, mana).
    fn op(code: i8, x: i16, g: i16, m: i16) -> DtEvent {
        let mut e = ev(EventKind::Global);
        e.message.clear();
        (e.results.no_meeting, e.results.patrol_delta) = (1, code);
        (e.results.experience, e.results.gold, e.results.mana) = (x, g, m);
        e
    }

    #[test]
    fn community_opcodes_change_the_players_army() {
        let mut learn = ev(EventKind::Global);
        learn.results.spells_learned = [4, 6, 0, 0];
        let mut equip = op(6, 0, 1, 0);
        equip.results.artifacts_add = [7, 0, 0, 0];
        let mut lasting = op(11, 0, -1, 0);
        lasting.results.spells_learned = [1, 0, 0, 0];
        let mut forget = op(16, 0, 0, 0);
        forget.results.spells_learned = [4, 0, 0, 0];
        let mut s = world(vec![learn, equip, op(7, 0, 1, 3), op(13, 0, -1, 10), lasting, forget, op(20, 10, 5, 0)]);
        s.named_characters = vec![crate::dt::dtm::NamedCharacter { unit: 5, name: "Aide".into() }];
        s.events.push(op(12, 0, 1, 1));
        let mut g = start(&s);
        read(&mut g);
        assert_eq!(g.squad[1].items[0], Some(ItemId(7)));
        assert_eq!(g.pack, Vec::<ItemId>::new(), "the item is worn, not given");
        assert_eq!((g.squad[1].def, g.squad[1].named), (UnitId(5), 1), "replaced by type 3, then named character 1 of type 5");
        assert_eq!((g.squad[0].xp, g.squad[1].xp), (10, 10));
        assert_eq!(g.spells, vec![6]);
        // Opcode 11's "permanent" spells end 0xEEEEEE hundredths of a minute after the start.
        let end = g.map_start() + crate::rules::magic::OPCODE_SPELL_END;
        assert_eq!(g.active_spells(), vec![SpellSlot { spell: 1, until: end }]);
        assert!(g.squad.iter().all(|u| u.spells == [Some(SpellSlot { spell: 1, until: end }), None, None, None]), "every unit");
        assert!(g.has_spells(Holder::Player, None, &[1]) && !g.has_spells(Holder::Player, None, &[1, 2]));
        assert_eq!(g.tile(), (10, 5));
        assert_eq!(g.gold, 100, "the resources are arguments");
    }

    #[test]
    fn community_opcodes_change_ai_armies_and_buildings() {
        let mut lasting = op(11, 2, -1, 0);
        lasting.results.spells_learned = [3, 0, 0, 0];
        let events = vec![
            op(7, 2, 0, 5),
            op(8, 2, 8, 0),
            op(9, 2, 4, 0),
            op(10, 2, 0, 2),
            lasting,
            op(12, 2, 1, 1),
            op(13, -1, -1, 1000),
            op(17, 2, 12, 0),
            op(19, 2, 14, 10),
            op(9, -1, 1, 0),
        ];
        let mut s = world(events);
        s.named_characters = vec![crate::dt::dtm::NamedCharacter { unit: 3, name: "Aide".into() }];
        // Two troops (unit ids 1–3 in a triple are skipped at load).
        s.armies = vec![army(2, 12, 10, 1, &[troop(4, 0, 2)])];
        let mut fort = building(BuildingType::Fort, 9, 2, (1, 1));
        fort.garrison[0] = troop(4, 0, 1);
        s.buildings = vec![fort];
        let mut g = start(&s);
        read(&mut g);
        let a = g.world.armies.iter().find(|a| a.id == 2).unwrap();
        assert_eq!(a.troops.iter().map(|t| t.unit).collect::<Vec<_>>(), vec![UnitId(5), UnitId(3)]);
        assert_eq!(a.speed, Army::speed_for(-3, 5), "5 − (−3) = 8");
        // Its own attitude becomes 2, but the relation (world::relation) is the player's −2
        // towards the enemy group, as in the original.
        assert_eq!((a.faction, a.attitude), (4, -2), "enemy group: hostile whatever its own attitude");
        let end = g.map_start() + crate::rules::magic::OPCODE_SPELL_END;
        assert!(a.troops.iter().all(|t| t.spells[0] == Some(SpellSlot { spell: 3, until: end })), "every unit of it");
        assert_eq!((a.named, a.figure), (1, 12));
        assert_eq!(a.post, (14, 10));
        assert!(!a.path.is_empty(), "it sets off");
        assert!(g.has_spells(Holder::Army(2), None, &[3]));
        assert!(g.army_at(2, 12, 10) && !g.army_at(2, 14, 10));
        let l = &g.world.locations[0];
        let mut level = 1;
        let mut left = 1000;
        while left >= g.content.xp_to_next(UnitId(4), level) {
            left -= g.content.xp_to_next(UnitId(4), level);
            level += 1;
        }
        assert_eq!(l.garrison[0].level, level);
        assert_eq!(l.garrison[0].xp, left, "an AI troop keeps what is left towards the next level");
        assert!(level > 1);
        assert_eq!((l.faction, l.owner), (1, crate::rules::world::Owner::Player), "the fort joins the player");
    }

    #[test]
    fn event_xp_goes_to_the_hero_as_it_is_and_new_levels_are_reported() {
        let mut gift = ev(EventKind::Global);
        gift.results.experience = 100;
        let mut g = start(&world(vec![gift]));
        let events = g.drain_events();
        // The knight of the test content needs 60, then 84: level 2 with 40 left.
        assert_eq!((g.squad[0].level, g.squad[0].xp), (2, 40));
        assert_eq!(g.squad[1].xp, 0, "only the hero");
        assert!(events.iter().any(|e| matches!(e, Event::LevelUp(0, 2))));
        let mut loss = ev(EventKind::Global);
        loss.results.experience = -50;
        let g = start(&world(vec![loss]));
        assert_eq!((g.squad[0].level, g.squad[0].xp), (1, 0), "negative XP does nothing");
    }

    #[test]
    fn level_conditions_count_from_zero() {
        let mut e = ev(EventKind::Global);
        (e.conditions.stats_check, e.conditions.level) = (1, 1);
        e.results.gold = 5;
        let mut g = start(&world(vec![e]));
        assert_eq!(g.gold, 100, "level 1 is the original's level 0");
        g.squad[0].level = 2;
        g.wait(1);
        assert_eq!(g.gold, 105, "level 2 passes a level-1 condition");
    }

    /// The hand-over (0x4b5b64): the old hero's whole record replaces the new hero (wounds
    /// and class too); gold and mana are set; the old pack and army replace the new map's.
    #[test]
    fn the_next_map_starts_with_what_carries_over() {
        let mut g = start(&world(vec![]));
        g.squad[0].level = 4;
        g.squad[0].xp = 33;
        g.squad[0].hp -= 2;
        g.squad[0].items[0] = Some(ItemId(7));
        g.squad[1].level = 3;
        let mut dead = g.squad[1].clone();
        dead.hp = 0;
        let next = NextMap {
            name: "Road".into(),
            branch: None,
            gold: Some(70),
            mana: Some(9),
            fame: false,
            hero: g.squad[0].clone(),
            spells: Some(vec![2, 5]),
            hero_items: true,
            inventory: Some(vec![ItemId(7)]),
            army: Some(vec![Unit { unpaid: true, last_paid: 0, ..g.squad[1].clone() }, dead]),
            flags: String::new(),
            class: HeroClass::Knight,
            hero_name: None,
            journal: crate::rules::journal::History::default(),
        };
        // The next map's header takes it all.
        let mut all = world(vec![]);
        all.header.carry_over = [1; 7];
        let mut fresh = start(&all);
        fresh.pack = vec![ItemId(3)];
        let hp = g.squad[0].hp;
        fresh.apply_carry_over(&next);
        assert_eq!((fresh.squad[0].level, fresh.squad[0].xp, fresh.squad[0].hp), (4, 33, hp), "his wounds stay");
        assert_eq!(fresh.spells, vec![2, 5], "byte 3 keeps the spell book");
        assert_eq!(fresh.squad[0].items[0], Some(ItemId(7)), "byte 4: the hero's worn items");
        let now = fresh.clock.total_minutes() as u64;
        assert!(!fresh.squad[1].unpaid && fresh.squad[1].last_paid == now, "the army comes paid");
        assert_eq!((fresh.gold, fresh.mana), (70, 9), "set, not added");
        assert_eq!(fresh.squad.len(), 2, "the old army replaces the preset's; its dead are dropped");
        assert_eq!(fresh.squad[1].level, 3, "the army keeps its levels");
        assert_eq!(fresh.pack, vec![ItemId(7)], "the old pack replaces the new one");
        // Without the next map's bytes (РК4 has no pack and no army): level 1 with no XP and
        // this map's book, no worn items, this map's pack and preset army behind the old
        // hero, whatever the old map offered.
        let mut again = start(&world(vec![]));
        again.pack = vec![ItemId(3)];
        let book = again.spells.clone();
        let preset = again.squad.len();
        let gold = again.gold;
        again.apply_carry_over(&next);
        assert_eq!((again.squad[0].level, again.squad[0].xp), (1, 0));
        assert_eq!((again.spells.clone(), again.squad[0].items), (book, [None; 4]));
        assert_eq!((again.gold, again.pack.clone()), (gold, vec![ItemId(3)]));
        assert_eq!((again.squad.len(), again.pack.clone()), (preset, vec![ItemId(3)]));
    }

    /// РК2's mines: a fort's own event asks for the peasants once the fort is the player's.
    /// Beating its garrison takes it but does not enter it (he stays on the cell he fought
    /// from, as the original: world.md §7.2, checked on РК1's ruins), so the event waits
    /// until he walks in.
    #[test]
    fn a_building_taken_from_its_garrison_runs_its_own_events_once_entered() {
        let mut mine = ev(EventKind::Local);
        let c = &mut mine.conditions;
        (c.buildings_check, c.buildings, c.buildings_owner) = (1, [1, 0, 0], [1, 0, 0]);
        mine.results.gold = 9;
        let mut s = world(vec![mine]);
        let mut fort = building(BuildingType::Fort, 5, 2, (1, 1));
        fort.garrison[0] = troop(4, 0, 1);
        (fort.faction, fort.relations) = (4, [-3, 0, 0, 0]);
        fort.event_slots[0] = 1;
        fort.event_count = 1;
        s.buildings = vec![fort];
        let mut g = start(&s);
        read(&mut g);
        let gold = g.gold;
        assert!(g.set_destination((5, 2)));
        walk(&mut g);
        assert!(matches!(g.foe, Some(Foe::Garrison(_))), "the garrison fights");
        let before = g.tile();
        let mut b = g.start_battle();
        b.begin();
        for f in b.fighters.iter_mut().filter(|f| f.team == crate::rules::battle::Team::Enemy) {
            f.hp = 0;
        }
        g.resolve_battle(&b);
        assert!(fired(&g.drain_events()).is_empty(), "not entered: its events wait");
        assert_eq!((g.tile(), g.location), (before, None), "outside, on the cell he fought from");
        assert!(g.world.locations[0].owned(), "taken");
        assert!(g.set_destination((5, 2)), "a click on it walks him in");
        let events = walk(&mut g);
        assert_eq!(g.tile(), (5, 2));
        assert_eq!(fired(&events), vec![1], "its event fires as he enters");
        assert!(g.gold >= gold + 9);
        // Its window opens once the event's is read.
        g.event_window_closed();
        assert_eq!(g.enter_waiting_building(), vec![Event::Arrived(0)]);
    }

    /// РК1 → РК2 → РК3: the next map starts with the carried army and the flags (the
    /// original stashes the flag string with the army, 4b5ef8, and puts it back, 4b5ff8)
    /// before its opening events run. РК2 opens with "the herald died" unless he came along,
    /// and РК3's king rewards the band beaten in РК2 by its flag.
    #[test]
    fn a_campaign_map_starts_with_the_carried_army_and_flags() {
        let mut band = ev(EventKind::Global);
        band.title = "Band%+Band".into();
        band.flags = crate::dt::dtm::FlagScript::from_title(&band.title);
        band.results.units_add = [4, 0, 0, 0];
        band.results.units_add_named = [1, 0, 0, 0];
        band.results.chained_event = 2;
        let mut win = ev(EventKind::Global);
        win.subordinate = 1;
        let mut s = world(vec![band, win]);
        s.header.victory_event = 2;
        s.next_map = "Next.DTm".into();
        s.header.carry_over = [1, 1, 1, 1, 1, 1, 1];
        s.named_characters = vec![crate::dt::dtm::NamedCharacter { unit: 4, name: "Herald".into() }];
        let mut g = Game::from_scenario(Arc::new(content()), &s, HeroClass::Archmage);
        read(&mut g);
        let next = g.next_map().expect("a victory with a next map");
        assert_eq!(next.flags, "Band\u{a0}");
        assert_eq!(next.class, HeroClass::Archmage);

        let mut died = ev(EventKind::Global);
        died.conditions.units_check = 1;
        (died.conditions.units, died.conditions.units_named, died.conditions.units_owner) = ([4, 0, 0], [1, 0, 0], [6, 0, 0]);
        let mut reward = ev(EventKind::Global);
        reward.title = "Reward%=Band".into();
        reward.flags = crate::dt::dtm::FlagScript::from_title(&reward.title);
        reward.results.gold = 5;
        let mut s2 = world(vec![died, reward]);
        s2.header.defeat_event = 1;
        s2.header.carry_over = [1; 7];
        s2.named_characters = s.named_characters.clone();
        let mut g2 = Game::from_campaign(Arc::new(content()), &s2, &next);
        assert_eq!(fired(&g2.drain_events()), vec![2], "the herald came along; the band's flag holds");
        assert_eq!(g2.script_end(), None);
        assert!(g2.script().unwrap().flag("Band"));
        assert_eq!(g2.archetype, 2);
        assert!(g2.squad.iter().any(|u| u.named == 1));

        // A restart (0x4b5ff8) starts the map again from that hand-over, the restart snapshot
        // a save keeps, not from the map's own preset.
        let (gold, units) = (g2.gold, g2.squad.len());
        g2.gold += 1000;
        g2.squad.truncate(1);
        let saved: Game = serde_json::from_value(serde_json::to_value(&g2).unwrap()).unwrap();
        assert_eq!(saved.carried, g2.carried);
        let mut again = saved.restart(Arc::new(content()), &s2);
        assert_eq!((again.gold, again.squad.len(), again.archetype), (gold, units, 2));
        assert!(again.squad.iter().any(|u| u.named == 1));
        assert_eq!(fired(&again.drain_events()), vec![2]);
        assert!(again.script().unwrap().flag("Band"));
        // A map started as a new game restarts from its preset, in the class it began with,
        // and its opening events run again (the herald joins).
        let fresh = g.restart(Arc::new(content()), &s);
        assert_eq!((fresh.carried.is_none(), fresh.archetype, fresh.squad.len()), (true, 2, 2));
    }

    /// The install's РК3 → РК4: РК4's header takes neither the pack nor the army (events.md
    /// §12: the next map's bytes 0x110–0x116), so the hero arrives with РК4's preset troops
    /// and pack, his own worn items still on him.
    #[test]
    fn rk4_takes_neither_the_army_nor_the_pack_of_rk3() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let load = |prefix: &str| dt.maps.iter().find(|m| m.name.starts_with(prefix)).expect("map present").load().unwrap();
        let (rk3, rk4) = (load("РК3"), load("РК4"));
        assert_eq!(rk4.header.carry_over, [1, 1, 1, 1, 1, 0, 0]);
        let content = Arc::new(crate::rules::content::Content::from_dt(&dt));
        let mut g = Game::from_scenario(content.clone(), &rk3, HeroClass::Knight);
        g.pack = vec![ItemId(content.items[0].id)];
        g.squad[0].items[0] = Some(ItemId(content.items[0].id));
        g.gold = 1234;
        // A soldier hired on РК3.
        let soldier = UnitId(content.units[10].id);
        let free = content.formation.new_unit_slot(&g.squad.iter().map(|u| u.slot).collect::<Vec<_>>()).unwrap();
        g.squad.push(Unit::new(&content, soldier, free));
        let next = g.hand_over("РК4".into(), None);
        let preset = Game::from_scenario(content.clone(), &rk4, HeroClass::Knight);
        let g4 = Game::from_campaign(content, &rk4, &next);
        assert_eq!(g4.squad.len(), preset.squad.len(), "РК4's own preset army");
        assert_eq!(g4.pack, preset.pack, "РК4's own pack");
        assert_eq!(g4.gold, 1234, "byte 0: the gold");
        assert_eq!(g4.squad[0].items[0], g.squad[0].items[0], "byte 4: his worn items");
    }

    #[test]
    fn the_tutorial_ends_on_meeting_the_herald_and_hands_over_to_its_second_map() {
        // Обучающий1: victory event 21 «Встреча с королевским гонцом» fires on meeting army 11
        // and has its window; the next map, Обучающий2, is in the install, so the original
        // hands over to it once that window is read (events.md §11).
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let load = |prefix: &str| dt.maps.iter().find(|m| m.name.starts_with(prefix)).expect("map present").load().unwrap();
        let t1 = load("Обучающий1");
        assert_eq!(t1.header.victory_event, 21);
        let win = &t1.events[20];
        assert_eq!((win.conditions.meet_army, win.message.is_empty()), (11, false));
        assert_eq!(t1.next_map.trim(), "Обучающий2.DTm");
        let content = Arc::new(crate::rules::content::Content::from_dt(&dt));
        let mut g = Game::from_scenario(content.clone(), &t1, HeroClass::Knight);
        g.script.as_mut().unwrap().end_for_test(EventOutcome::Victory(21));
        let next = g.next_map().expect("a next map after the victory");
        let stem = next.name.trim().strip_suffix(".DTm").unwrap();
        let entry = dt.maps.iter().find(|m| m.name == stem).expect("Обучающий2 in the install");
        let g2 = Game::from_campaign(content, &entry.load().unwrap(), &next);
        assert_eq!(g2.script_end(), None);
    }

    #[test]
    fn next_map_after_a_campaign_victory() {
        let mut branch = op(15, 3, 2, 0);
        branch.results.chained_event = 2;
        let mut win = ev(EventKind::Global);
        win.subordinate = 1;
        let mut s = world(vec![branch, win]);
        s.header.victory_event = 2;
        s.next_map = "0-0 Road".into();
        s.header.carry_over = [1, 0, 0, 1, 0, 1, 1];
        let mut g = start(&s);
        assert_eq!(g.script_end(), Some(ScriptEnd::Victory(2)));
        // Every unit's spell slots are wiped at the hand-over; the drain is the unit's.
        g.squad[1].spells[0] = Some(SpellSlot { spell: 1, until: u64::MAX });
        (g.squad[1].drain, g.squad[0].drain) = (20, 36);
        let next = g.next_map().unwrap();
        let army = next.army.clone().unwrap();
        assert_eq!((army[0].spells, army[0].drain, next.hero.drain), ([None; SPELL_SLOTS], 20, 36));
        assert_eq!(next.name, "3-2 Road");
        assert_eq!(next.branch, Some((3, 2)));
        // The hand-over offers everything; the next map's bytes choose (events.md §12).
        assert_eq!((next.gold, next.mana), (Some(100), Some(g.mana)));
        assert_eq!((next.hero.level, next.hero.xp), (1, 0));
        assert_eq!(army.len(), 1);
        assert!(next.inventory.is_some() && next.hero_items);
        assert_eq!(next.spells, Some(g.spells.clone()));

        // No branch: the scenario's next map; none before a victory.
        let mut win = ev(EventKind::Global);
        win.start_time = 624_354_300 + 120;
        let mut s = world(vec![win]);
        s.header.victory_event = 1;
        s.next_map = "Road".into();
        let mut g = start(&s);
        assert_eq!(g.next_map(), None);
        g.wait(4);
        assert_eq!(g.next_map().map(|n| n.name), Some("Road".to_string()));
        assert_eq!(branch_name("Road", (4, 1)), "4-1");
        assert_eq!(branch_name("12-3.DTm", (4, 1)), "4-1.DTm");
    }
    /// Every string of the map has its double spaces collapsed at load (0x4b2aa1).
    #[test]
    fn map_strings_lose_their_double_spaces() {
        let mut e = ev(EventKind::Global);
        e.message = "Hail,   friend.".into();
        let mut s = world(vec![e]);
        let mut town = building(BuildingType::Town, 9, 2, (1, 1));
        town.name = "Old  Town".into();
        s.buildings = vec![town];
        let g = start(&s);
        assert_eq!(g.world.locations[0].name, "Old Town");
        assert_eq!(g.script().unwrap().event(1).unwrap().message, "Hail, friend.");
    }

    #[test]
    fn hero_name_escapes() {
        let g = start(&world(vec![]));
        let name = g.hero().name(&g.content).to_string();
        assert_eq!(g.fill_text("Hail, #HERONAME!\r\n"), format!("Hail, {name}!\n"));
        let mut g = g;
        g.set_hero_name("  Ivo ");
        assert_eq!(g.fill_text("#HERONAME the #HEROCLASS"), "Ivo the #HEROCLASS", "no other escape");
        g.set_hero_name(" ");
        assert_eq!(g.hero_name(), name, "an empty name is the class's");
    }
}

#[cfg(test)]
mod real_maps {
    //! РК1 with the player's install; skipped without `RAZDOR_DT_DIR`. Numbers only.
    use super::*;
    use crate::dt::install::DtInstall;
    use crate::rules::content::{Content, HeroClass};
    use std::sync::Arc;

    /// Answers every question Yes; counts the messages.
    fn settle(g: &mut Game, events: Vec<Event>, messages: &mut usize) {
        let mut queue = events;
        for _ in 0..200 {
            if queue.is_empty() {
                break;
            }
            let mut next = Vec::new();
            for e in queue {
                match e {
                    Event::Script(EventOutcome::Fired { message: true, .. }) => *messages += 1,
                    Event::Script(EventOutcome::Question(_)) => {
                        *messages += 1;
                        next.extend(g.answer_question(true));
                    }
                    _ => {}
                }
            }
            next.extend(g.drain_events());
            queue = next;
        }
    }

    #[test]
    fn rk1_opening_and_first_buildings_for_every_class() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let c = Arc::new(Content::from_dt(&dt));
        let s = dt.maps.iter().find(|m| m.name.starts_with("РК1")).unwrap().load().unwrap();
        for class in HeroClass::ALL {
            let mut g = Game::from_scenario(c.clone(), &s, class);
            let mut messages = 0;
            let opening = g.drain_events();
            settle(&mut g, opening, &mut messages);
            let at_start = messages;
            // The two buildings nearest on foot.
            let mut visited = Vec::new();
            for _ in 0..2 {
                let from = g.tile();
                let Some((l, _)) = g.world.nearest_location(from, |l| !l.kind.is_bridge() && !visited.contains(&l.id)) else { break };
                visited.push(g.world.locations[l].id);
                g.set_destination(g.world.locations[l].tile);
                for _ in 0..40_000 {
                    if g.foe.is_some() || g.pending_question().is_some() {
                        break;
                    }
                    if !g.moving() {
                        if g.location == Some(l) {
                            break;
                        }
                        // Stopped to read: go on.
                        if !g.set_destination(g.world.locations[l].tile) {
                            break;
                        }
                    }
                    let events = g.tick(0.05);
                    settle(&mut g, events, &mut messages);
                }
                if g.foe.is_some() {
                    break;
                }
                // Leave the building for the next walk.
                g.location = None;
            }
            let engine = g.script().unwrap();
            println!(
                "РК1 {class:?}: {at_start} messages at the start, {messages} after {} buildings; {} firings, {} quests ({} done), day {}",
                visited.len(),
                engine.total_fired(),
                engine.journal().len() + engine.completed_quests().len(),
                engine.completed_quests().len(),
                g.clock.day_index() - g.world.start.day_index()
            );
            assert!(messages >= 1, "{class:?}: at least one dialog");
        }
    }
}
