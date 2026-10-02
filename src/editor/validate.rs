//! Checks before saving: everything the file format, the original game or Razdor would
//! trip over, as readable messages.
//!
//! Errors block saving; warnings are shown and saved anyway. Checks against unit, artefact
//! and spell ids need the install's [`Names`]; checks of object and building pictures need
//! a palette read from the install ([`Palette::from_install`]).

use std::fmt;

use crate::dt::dtm::{Scenario, BUILDING_SIZE, EVENT_SIZE};
use crate::dt::text;
use crate::i18n::{n_, tr};
use crate::trf;

use super::geometry::Footprint;
use super::palette::{building_type_label, Names, Palette};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// What an issue is about (ids are 1-based, as the map stores them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Map,
    Settings,
    Hero(usize),
    Object(usize),
    Building(u16),
    Army(u8),
    Point(u16),
    Event(u16),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    pub place: Place,
    pub message: String,
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Place::Map => f.write_str(tr("Map")),
            Place::Settings => f.write_str(tr("Scenario settings")),
            Place::Hero(k) => f.write_str(&trf!("{class} start", class = tr(super::palette::HERO_CLASSES.get(*k).unwrap_or(&n_("Hero"))))),
            Place::Object(i) => f.write_str(&trf!("Object {n}", n = i + 1)),
            Place::Building(id) => f.write_str(&trf!("Building {id}", id)),
            Place::Army(id) => f.write_str(&trf!("Army {id}", id)),
            Place::Point(id) => f.write_str(&trf!("Point {id}", id)),
            Place::Event(id) => f.write_str(&trf!("Event {id}", id)),
        }
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self.severity {
            Severity::Error => tr("error"),
            Severity::Warning => tr("warning"),
        };
        write!(f, "{} ({s}): {}", self.place, self.message)
    }
}

/// Largest map side the editor accepts (the original's generator offers up to 800).
pub const MAX_SIDE: u32 = 800;
/// References to buildings, armies and points are single bytes.
pub const MAX_RECORDS: usize = 255;

struct Checker<'a> {
    s: &'a Scenario,
    names: Option<&'a Names>,
    palette: Option<&'a Palette>,
    out: Vec<Issue>,
}

impl Checker<'_> {
    fn error(&mut self, place: Place, message: String) {
        self.out.push(Issue { severity: Severity::Error, place, message });
    }

    fn warn(&mut self, place: Place, message: String) {
        self.out.push(Issue { severity: Severity::Warning, place, message });
    }

    fn inside(&self, x: u16, y: u16) -> bool {
        (x as u32) < self.s.width() && (y as u32) < self.s.height()
    }

    fn string(&mut self, place: Place, what: &str, s: &str) {
        if s.contains('\0') {
            self.error(place, trf!("the {what} contains a NUL character", what));
        } else if text::decode(&text::encode(s)) != s {
            let bad: String = s.chars().filter(|c| text::decode(&text::encode(&c.to_string())) != c.to_string()).take(5).collect();
            self.warn(place, trf!("the {what} has characters Windows-1251 cannot store ({bad}); they are saved as '?'", what, bad));
        }
    }

    fn building_ref(&mut self, place: Place, what: &str, id: u32) {
        if id as usize > self.s.buildings.len() {
            self.error(place, trf!("{what} refers to building {id}, which does not exist", what, id));
        }
    }

    fn army_ref(&mut self, place: Place, what: &str, id: u32) {
        if id as usize > self.s.armies.len() {
            self.error(place, trf!("{what} refers to army {id}, which does not exist", what, id));
        }
    }

    /// A building reference the original editor's building delete leaves as it is (army
    /// home, building link, preset start building, records.md §12): past the end it is the
    /// original's own output, which the game ignores (no such building), so a warning.
    fn stale_building_ref(&mut self, place: Place, what: &str, id: u32) {
        if id as usize > self.s.buildings.len() {
            self.warn(place, trf!("{what} refers to building {id}, past the end (a building delete leaves it, as in the original editor); the game ignores it", what, id));
        }
    }

    /// An army reference the original editor's army delete leaves as it is (the patrol
    /// change, the army at home, the battle army, records.md §12): a warning, as
    /// [`Checker::stale_building_ref`].
    fn stale_army_ref(&mut self, place: Place, what: &str, id: u32) {
        if id as usize > self.s.armies.len() {
            self.warn(place, trf!("{what} refers to army {id}, past the end (an army delete leaves it, as in the original editor); the game ignores it", what, id));
        }
    }

    /// A named character the original editor's character delete leaves as it is (army byte
    /// 58 and the events' bytes, records.md §7): a warning, as [`Checker::stale_building_ref`].
    fn stale_named(&mut self, place: Place, what: &str, n: u8) {
        if n as usize > self.s.named_characters.len() {
            self.warn(place, trf!("{what}: named character {n} is past the end (a character delete leaves it, as in the original editor); the game finds no one", what, n));
        }
    }

    fn event_ref(&mut self, place: Place, what: &str, id: u32) {
        if id as usize > self.s.events.len() {
            self.error(place, trf!("{what} refers to event {id}, which does not exist", what, id));
        }
    }

    fn unit(&mut self, place: Place, what: &str, id: u8) {
        if let Some(n) = self.names {
            if id != 0 && !n.has_unit(id as u32) {
                self.error(place, trf!("{what}: unit {id} is not in the game's unit list", what, id));
            }
        }
    }

    fn artefact(&mut self, place: Place, what: &str, id: u32) {
        if let Some(n) = self.names {
            if id != 0 && !n.has_artefact(id) {
                self.error(place, trf!("{what}: artefact {id} is not in the game's artefact list", what, id));
            }
        }
    }

    fn spell(&mut self, place: Place, what: &str, id: u8) {
        if let Some(n) = self.names {
            if id != 0 && !n.has_spell(id as u32) {
                self.error(place, trf!("{what}: spell {id} is not in the game's spell list", what, id));
            }
        }
    }

    fn relations(&mut self, place: Place, what: &str, r: &[i8]) {
        if r.iter().any(|v| !(-3..=3).contains(v)) {
            self.error(place, trf!("{what} must lie between -3 and 3", what));
        }
    }

    fn faction(&mut self, place: Place, f: u8) {
        if !(1..=4).contains(&f) {
            self.error(place, trf!("faction {f} is not one of 1-4 (player, ally, neighbour, enemy)", f));
        }
    }

    fn map(&mut self) {
        let s = self.s;
        let (w, h) = (s.width(), s.height());
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
            self.error(Place::Map, trf!("map size {w}x{h}: each side must be 1-{max}", w, h, max = MAX_SIDE));
        }
        if s.terrain.len() as u64 != w as u64 * h as u64 {
            self.error(Place::Map, trf!("the terrain has {cells} cells, the size needs {need}", cells = s.terrain.len(), need = w as u64 * h as u64));
        }
        if w != h {
            self.warn(Place::Map, tr("the map is not square; every shipped map is").into());
        }
        if let Some(c) = s.terrain.iter().find(|c| **c > 15) {
            self.error(Place::Map, trf!("terrain code {c} is not one of the 16 surfaces", c));
        }
        let from_install = self.palette.filter(|p| p.from_install);
        for (i, o) in s.objects.iter().enumerate() {
            if !self.inside(o.x, o.y) {
                self.error(Place::Object(i), trf!("object at ({x}, {y}) is outside the map", x = o.x, y = o.y));
            }
            if let Some(p) = from_install {
                if !p.has_object(o.class, o.sprite) {
                    self.error(Place::Object(i), trf!("the game has no picture for object class {class} sprite {sprite}", class = o.class, sprite = o.sprite));
                }
            }
        }
    }

    fn settings(&mut self) {
        let s = self.s;
        let h = &s.header;
        for (what, v) in [(tr("title"), &s.title), (tr("description"), &s.description), (tr("campaign name"), &s.campaign_name), (tr("next map"), &s.next_map)] {
            self.string(Place::Settings, what, v);
        }
        if !s.next_map.is_empty() && !s.next_map.to_ascii_lowercase().ends_with(".dtm") {
            self.warn(Place::Settings, trf!("the next map \"{file}\" should be a .DTm file name", file = s.next_map));
        }
        self.event_ref(Place::Settings, tr("the victory event"), h.victory_event as u32);
        self.event_ref(Place::Settings, tr("the defeat event"), h.defeat_event as u32);
        for row in &h.relations {
            self.relations(Place::Settings, tr("the faction relations"), row);
        }
        if h.scenario_kind > 2 {
            self.error(Place::Settings, trf!("scenario kind {kind} is not one of 0-2", kind = h.scenario_kind));
        }
        if s.named_characters.len() > 32 {
            self.error(Place::Settings, trf!("{n} named characters; at most 32 fit", n = s.named_characters.len()));
        }
        for (k, n) in s.named_characters.iter().enumerate() {
            self.unit(Place::Settings, &trf!("named character {n}", n = k + 1), n.unit);
            self.string(Place::Settings, &trf!("name of named character {n}", n = k + 1), &n.name);
        }
        for (k, p) in h.heroes.iter().enumerate() {
            let place = Place::Hero(k);
            if !self.inside(p.x, p.y) {
                self.error(place, trf!("the start ({x}, {y}) is outside the map", x = p.x, y = p.y));
            }
            self.stale_building_ref(place, tr("the start building"), p.start_building as u32);
            for t in p.troops.iter().filter(|t| t.unit != 0) {
                self.unit(place, tr("starting troops"), t.unit);
            }
            for a in p.artifacts {
                self.artefact(place, tr("starting artefacts"), a as u32);
            }
            for sp in p.spells {
                self.spell(place, tr("starting spells"), sp);
            }
            if p.gold > i16::MAX as u32 || p.mana > i16::MAX as u32 {
                self.error(place, trf!("gold and mana must be at most {max} (the game reads 16 bits)", max = i16::MAX));
            }
        }
    }

    fn buildings(&mut self) {
        let s = self.s;
        if s.buildings.len() > MAX_RECORDS {
            self.error(Place::Map, trf!("{n} buildings; at most {max} can be referred to", n = s.buildings.len(), max = MAX_RECORDS));
        }
        let footprints: Vec<Footprint> = s.buildings.iter().map(|b| Footprint::of(b.x as i32, b.y as i32, b.size_x, b.size_y)).collect();
        let from_install = self.palette.filter(|p| p.from_install);
        for (i, b) in s.buildings.iter().enumerate() {
            let id = i as u16 + 1;
            let place = Place::Building(id);
            if b.kind > 15 {
                self.error(place, trf!("building type {kind} is not one of 0-15", kind = b.kind));
            }
            if b.size_x == 0 || b.size_y == 0 {
                self.error(place, tr("the footprint size must be at least 1x1").into());
            }
            if !footprints[i].inside(s.width(), s.height()) {
                self.error(place, trf!("the footprint of the {kind} at ({x}, {y}) reaches outside the map", kind = tr(building_type_label(b.kind)), x = b.x, y = b.y));
            }
            let is_bridge = matches!(b.kind, 13 | 14);
            if !is_bridge {
                let bounds = footprints[i].bounds();
                if let Some(j) = (0..i).find(|&j| !matches!(s.buildings[j].kind, 13 | 14) && footprints[j].bounds().overlaps(&bounds)) {
                    self.warn(place, trf!("overlaps building {n}", n = j + 1));
                }
            }
            if let Some(p) = from_install {
                match p.picture(b.picture_type, b.picture_variant) {
                    None => self.error(place, trf!("the game has no picture {variant} of type {kind}", variant = b.picture_variant, kind = b.picture_type)),
                    Some(pic) if pic.size != (b.size_x, b.size_y) => self.warn(
                        place,
                        trf!("footprint {w}x{h} differs from its picture's {pw}x{ph}", w = b.size_x, h = b.size_y, pw = pic.size.0, ph = pic.size.1),
                    ),
                    Some(_) => {}
                }
            }
            if b.event_count as usize > b.event_slots.len() {
                self.error(place, trf!("{n} local events; at most {max} fit", n = b.event_count, max = b.event_slots.len()));
            }
            for e in b.events() {
                self.event_ref(place, tr("a local event"), e as u32);
            }
            if b.owner_army != 0 && b.owner_army != 0xFF {
                self.army_ref(place, tr("the owner"), b.owner_army as u32);
            }
            self.stale_building_ref(place, tr("the linked building"), b.linked_building as u32);
            if b.linked_building as u16 == id {
                self.warn(place, tr("is linked to itself").into());
            }
            self.faction(place, b.faction);
            self.relations(place, tr("attitudes"), &b.relations);
            for t in b.garrison.iter().filter(|t| t.unit != 0) {
                self.unit(place, tr("garrison"), t.unit);
            }
            for r in b.barracks.iter().filter(|r| r.unit != 0) {
                self.unit(place, tr("barracks"), r.unit);
                if r.start_count > r.max_count {
                    self.warn(place, trf!("barracks start with {start} units but hold at most {max}", start = r.start_count, max = r.max_count));
                }
            }
            for a in b.artifacts() {
                self.artefact(place, tr("goods"), a as u32);
            }
            for sp in b.spells_for_sale {
                self.spell(place, tr("spells for sale"), sp);
            }
            if b.kind != 12 && b.random_artifacts_for_sale > 0 && b.price_min > b.price_max {
                self.warn(place, trf!("the lowest price {low} is above the highest {high}", low = b.price_min, high = b.price_max));
            }
            for (what, v) in [(tr("name"), &b.name), (tr("owner name"), &b.owner_name), (tr("description"), &b.description)] {
                self.string(place, what, v);
            }
        }
    }

    fn armies(&mut self) {
        let s = self.s;
        if s.armies.len() > MAX_RECORDS {
            self.error(Place::Map, trf!("{n} armies; at most {max} fit", n = s.armies.len(), max = MAX_RECORDS));
        }
        for (i, a) in s.armies.iter().enumerate() {
            let place = Place::Army(i.min(254) as u8 + 1);
            if a.id as usize != i + 1 {
                self.error(place, trf!("stores id {id}; armies must be numbered in order", id = a.id));
            }
            if !self.inside(a.x, a.y) {
                self.error(place, trf!("({x}, {y}) is outside the map", x = a.x, y = a.y));
            }
            if !(1..=12).contains(&a.model) {
                self.error(place, trf!("map model {model} is not one of 1-12", model = a.model));
            }
            self.stale_building_ref(place, tr("the home building"), a.home_building as u32);
            self.stale_named(place, tr("the leader"), a.named_character);
            if a.leader_unit == super::records::SPECIAL_LEADER {
                // The original's extra leader entry (records.md §3.3); what the game does
                // with it is unknown.
                self.warn(place, tr("the leader is the original editor's extra empty entry (255)").into());
            } else {
                self.unit(place, tr("the leader"), a.leader_unit);
            }
            for t in a.troops.iter().filter(|t| t.unit != 0) {
                self.unit(place, tr("troops"), t.unit);
            }
            if a.leader_unit == 0 && a.troops().next().is_none() {
                self.warn(place, tr("has neither a leader nor troops").into());
            }
            for x in a.artifacts {
                self.artefact(place, tr("carried artefacts"), x as u32);
            }
            self.spell(place, tr("the spell on the army"), a.spell);
            self.faction(place, a.faction);
            self.relations(place, tr("attitudes"), &a.relations);
            if a.target_model > 4 {
                self.error(place, trf!("target model {model} is not one of 0-4", model = a.target_model));
            }
            for (what, v) in [(tr("name"), &a.name), (tr("leader name"), &a.leader_name), (tr("description"), &a.description)] {
                self.string(place, what, v);
            }
        }
    }

    fn points(&mut self) {
        let s = self.s;
        if s.points.len() > super::doc::MAX_POINTS {
            self.error(Place::Map, trf!("{n} points; at most {max} fit", n = s.points.len(), max = super::doc::MAX_POINTS));
        }
        for (i, p) in s.points.iter().enumerate() {
            let place = Place::Point(i.min(super::doc::MAX_POINTS - 1) as u16 + 1);
            // The original's 256th point stores id 0 and its model plus 1
            // ([`super::defaults::new_point`]), and a point delete then makes it id 255 with
            // that model (its renumbering keeps the high byte): the original editor's own
            // output, which the game reads (it finds points by the stored id and does not look
            // at the model), so warnings.
            if i + 1 == super::doc::MAX_POINTS && p.id == 0 {
                self.warn(place, tr("stores id 0: the original editor's 256th point overflows its id; no event can light it").into());
            } else if p.id as usize != i + 1 {
                self.error(place, trf!("stores id {id}; points must be numbered in order", id = p.id));
            }
            if !self.inside(p.x, p.y) {
                self.error(place, trf!("({x}, {y}) is outside the map", x = p.x, y = p.y));
            }
            if p.model == 11 {
                self.warn(place, tr("model 11: an AI target point the original editor placed as the 256th point; the game reads it as a point").into());
            } else if !matches!(p.model, 8..=10) {
                self.error(place, trf!("model {model} is none of 8 (lantern), 9 (event point) and 10 (AI target)", model = p.model));
            }
            if p.event_count as usize > p.event_slots.len() {
                self.error(place, trf!("{n} events; at most {max} fit", n = p.event_count, max = p.event_slots.len()));
            }
            for e in p.events() {
                self.event_ref(place, tr("an attached event"), e as u32);
            }
            if p.radius > 24 {
                self.warn(place, trf!("radius {r} is above the original's 24", r = p.radius));
            }
        }
    }

    fn events(&mut self) {
        use super::events::{self as ev, Arg};
        let s = self.s;
        if s.events.len() > ev::MAX_EVENTS {
            self.error(Place::Map, trf!("{n} events; the original editor holds at most {max}", n = s.events.len(), max = ev::MAX_EVENTS));
        }
        let named = s.named_characters.len();
        for (i, e) in s.events.iter().enumerate() {
            let id = i.min(u16::MAX as usize - 1) as u16 + 1;
            let place = Place::Event(id);
            let (c, r) = (&e.conditions, &e.results);
            let opcode = ev::opcode(e);
            if !(1..=4).contains(&e.kind) {
                self.error(place, trf!("type {kind} is not one of 1-4 (global, local, quest, rumour)", kind = e.kind));
            }
            if e.archetype > 3 {
                self.error(place, trf!("hero archetype {n} is not one of 0-3", n = e.archetype));
            }
            for b in c.buildings {
                self.building_ref(place, tr("a building condition"), b as u32);
            }
            for a in c.defeated_armies.into_iter().chain(c.beaten_armies).chain([c.meet_army, c.army_active, c.army_inactive]) {
                self.army_ref(place, tr("a condition"), a as u32);
            }
            self.stale_army_ref(place, tr("the army-at-home condition"), c.army_at_home as u32);
            for a in r.activate_armies.into_iter().chain([r.deactivate_army, r.show_army, r.removed_units_to_army, r.units_from_army]) {
                self.army_ref(place, tr("a result"), a as u32);
            }
            self.stale_army_ref(place, tr("the battle"), r.start_battle_with as u32);
            if opcode.is_none() {
                self.stale_army_ref(place, tr("the patrol change"), r.patrol_army as u32);
            }
            for x in c.happened_yes.into_iter().chain(c.happened_no).chain(c.not_happened).chain([r.relative_event, r.completes_quest, r.chained_event]) {
                self.event_ref(place, tr("a condition or result"), x as u32);
            }
            if r.completes_quest != 0 && (r.completes_quest as usize) <= s.events.len() && !ev::is_quest(s, r.completes_quest) {
                self.error(place, trf!("completes event {id}, which is not a quest", id = r.completes_quest));
            }
            for l in r.light_lanterns {
                if l as usize > s.points.len() {
                    self.error(place, trf!("lights point {l}, which does not exist", l));
                }
            }
            for (what, list) in [(tr("named squads"), &c.units_named[..]), (tr("units added"), &r.units_add_named[..]), (tr("units removed"), &r.units_remove_named[..])] {
                for &n in list {
                    self.stale_named(place, what, n);
                }
            }
            for o in c.buildings_owner.into_iter().chain(c.units_owner).chain(c.artifacts_owner) {
                if o > 6 {
                    self.error(place, trf!("owner code {o} is not one of 0-6", o));
                }
            }
            for u in c.units.into_iter().chain(r.units_add).chain(r.units_remove.into_iter().filter(|u| *u < ev::REMOVE_ADDED_UNIT)).chain([r.new_hero_class]) {
                self.unit(place, tr("the event"), u);
            }
            if !matches!(r.picture, 0 | ev::PICTURE_DEFEAT | ev::PICTURE_VICTORY) {
                self.unit(place, tr("the picture"), r.picture);
            }
            for a in c.artifacts.into_iter().chain(r.artifacts_add).chain(r.artifacts_remove) {
                self.artefact(place, tr("the event"), a as u32);
            }
            for sp in r.spells_learned.into_iter().chain([r.cast_spell]) {
                self.spell(place, tr("the event"), sp);
            }
            for m in ev::flag_problems(&e.title) {
                self.error(place, trf!("flag script: {m}", m));
            }
            if c.confirm_question != 0 && e.question.trim().is_empty() && e.message.trim().is_empty() {
                self.warn(place, tr("asks a question but has neither a question nor a message text").into());
            }
            if let Some(op) = opcode.and_then(ev::opcode_info) {
                let args = ev::opcode_args(e);
                for (k, arg) in op.args.iter().enumerate() {
                    let Some((label, kind)) = arg else { continue };
                    let v = args[k];
                    let bad = match kind {
                        Arg::Holder => (v > 0 && v as usize > s.armies.len()) || (v < 0 && v.unsigned_abs() as usize > s.buildings.len()),
                        Arg::Army => v < 0 || v as usize > s.armies.len(),
                        Arg::EventShift => !(1..=s.events.len() as i64).contains(&(id as i64 + v as i64)),
                        Arg::EventField => !ev::EVENT_FIELDS.iter().any(|f| f.0 as i16 == v),
                        Arg::Named => v < 1 || v as usize > named,
                        Arg::Unit => v < 1 || self.names.is_some_and(|n| !n.has_unit(v as u32)),
                        Arg::Number => false,
                    };
                    if bad {
                        self.warn(place, trf!("opcode {code} ({name}): {label} = {v} names nothing on this map", code = op.code, name = tr(op.name), label = tr(label), v));
                    }
                }
                if let Some(x) = ev::second_edit(e) {
                    if !(1..=s.events.len() as i64).contains(&(id as i64 + x.shift as i64)) {
                        self.warn(place, trf!("opcode {code}: the second setting's target event ({shift}) does not exist", code = op.code, shift = format!("{:+}", x.shift)));
                    }
                    if !ev::EVENT_FIELDS.iter().any(|f| f.0 as i16 == x.field) {
                        self.warn(place, trf!("opcode {code}: the second setting's field {field} is not a field", code = op.code, field = x.field));
                    }
                }
            }
            if let Some(p) = &e.custom_picture {
                if p.len() > u16::MAX as usize {
                    self.error(place, tr("the event picture is larger than 65535 bytes").into());
                } else if ev::picture_size(p).is_none() {
                    self.warn(place, tr("the event picture's size does not match its data").into());
                }
            }
            for (what, v) in [(tr("title"), &e.title), (tr("question"), &e.question), (tr("message"), &e.message)] {
                self.string(place, what, v);
            }
        }
    }
}

/// All issues of a scenario, errors first.
pub fn validate(s: &Scenario, names: Option<&Names>, palette: Option<&Palette>) -> Vec<Issue> {
    let mut c = Checker { s, names, palette, out: Vec::new() };
    c.map();
    c.settings();
    c.buildings();
    c.armies();
    c.points();
    c.events();
    let mut out = c.out;
    // The writer must produce a payload that reads back to the same bytes. Only checked when
    // nothing structural is wrong (the writer assumes a consistent terrain size).
    if !out.iter().any(|i| i.severity == Severity::Error) {
        if let Err(message) = self_check(s) {
            out.push(Issue { severity: Severity::Error, place: Place::Map, message });
        }
    }
    out.sort_by_key(|i| std::cmp::Reverse(i.severity));
    out
}

/// The payload of `s` reads back and serialises to the same bytes: section sizes, record
/// counts and the strings (one per field, in record order) all line up.
pub fn self_check(s: &Scenario) -> Result<Vec<u8>, String> {
    let bytes = s.to_payload();
    let back = Scenario::parse_payload(&bytes).map_err(|e| trf!("the written map does not read back: {e}", e))?;
    let sizes = [back.buildings.len() * BUILDING_SIZE, back.events.len() * EVENT_SIZE];
    if sizes != [s.buildings.len() * BUILDING_SIZE, s.events.len() * EVENT_SIZE] || back.armies.len() != s.armies.len() || back.points.len() != s.points.len() {
        return Err(tr("the written map has a different number of records").into());
    }
    // Every record's strings are where the reader expects them: 4 of the scenario, 3 per
    // building, army and event, one per named character.
    let expected = 4 + 3 * (s.buildings.len() + s.armies.len() + s.events.len()) + s.named_characters.len();
    if count_strings(&bytes) != Some(expected) {
        return Err(tr("the written map has a different number of texts than its records need").into());
    }
    if back.to_payload() != bytes {
        return Err(tr("the written map does not serialise back to the same bytes").into());
    }
    Ok(bytes)
}

/// The number of NUL-terminated strings between the text marker and the pictures.
fn count_strings(payload: &[u8]) -> Option<usize> {
    let u32_at = |o: usize| payload.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")) as usize);
    let text = u32_at(0x18)?;
    let pictures = u32_at(0x11C)?;
    let events = Scenario::parse_payload(payload).ok()?.events;
    let event_pictures: usize = events.iter().filter_map(|e| e.custom_picture.as_ref()).map(Vec::len).sum();
    let end = payload.len().checked_sub(pictures + event_pictures)?;
    Some(payload.get(text..end)?.iter().filter(|b| **b == 0).count())
}

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::Error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Army, Building, Event, NamedCharacter, Point};
    use crate::editor::palette::{BuildingPicture, ObjectKey};

    fn map(w: u32, h: u32) -> Scenario {
        let mut s = Scenario::default();
        s.header.width = w;
        s.header.height = h;
        s.terrain = vec![6; (w * h) as usize];
        s
    }

    fn errors(s: &Scenario) -> Vec<String> {
        validate(s, None, None).into_iter().filter(|i| i.severity == Severity::Error).map(|i| i.to_string()).collect()
    }

    fn building(x: u16, y: u16, size: (u8, u8)) -> Building {
        Building { x, y, kind: 3, picture_type: 3, size_x: size.0, size_y: size.1, faction: 3, ..Building::default() }
    }

    #[test]
    fn an_empty_map_is_valid() {
        assert!(errors(&map(10, 10)).is_empty());
    }

    #[test]
    fn size_and_terrain() {
        let mut s = map(4, 4);
        s.terrain.pop();
        assert!(errors(&s)[0].contains("terrain has 15 cells"));
        let mut s = map(4, 4);
        s.terrain[3] = 16;
        assert!(errors(&s)[0].contains("terrain code 16"));
        let s = map(0, 0);
        assert!(errors(&s)[0].contains("each side"));
        let w = validate(&map(4, 3), None, None);
        assert!(w.iter().any(|i| i.severity == Severity::Warning && i.message.contains("not square")));
    }

    #[test]
    fn footprints_must_be_inside() {
        let mut s = map(10, 10);
        s.buildings = vec![building(9, 9, (4, 4)), building(2, 2, (4, 4)), building(5, 2, (4, 3))];
        let e = errors(&s);
        assert_eq!(e.len(), 2, "{e:?}");
        assert!(e[0].starts_with("Building 2") && e[0].contains("outside"));
        // 4x3 needs an extra row above: rows -1..=2 for y = 2.
        assert!(e[1].starts_with("Building 3"));
        s.buildings[2].y = 3;
        s.buildings[1].x = 3;
        s.buildings[1].y = 3;
        assert!(errors(&s).is_empty());
    }

    #[test]
    fn overlapping_buildings_warn() {
        let mut s = map(10, 10);
        s.buildings = vec![building(5, 5, (2, 2)), building(6, 6, (2, 2))];
        let all = validate(&s, None, None);
        assert!(all.iter().any(|i| i.place == Place::Building(2) && i.message.contains("overlaps building 1")));
    }

    #[test]
    fn dangling_references_are_errors() {
        let mut s = map(10, 10);
        let mut b = building(5, 5, (2, 2));
        b.owner_army = 3;
        b.linked_building = 7;
        b.event_count = 1;
        b.event_slots[0] = 2;
        s.buildings = vec![b];
        s.header.victory_event = 1;
        s.header.heroes[1].start_building = 4;
        s.armies = vec![Army { id: 1, x: 1, y: 1, model: 4, faction: 4, home_building: 9, leader_unit: 1, ..Army::default() }];
        s.points = vec![Point { id: 1, model: 9, event_count: 1, event_slots: [5, 0, 0, 0, 0, 0, 0, 0, 0, 0], ..Point::default() }];
        let e = errors(&s).join("\n");
        for needle in ["victory event refers to event 1", "local event refers to event 2", "owner refers to army 3", "Point 1 (error): an attached event refers to event 5"] {
            assert!(e.contains(needle), "{needle} missing in\n{e}");
        }
        // What the original's deletes leave past the end is a warning, not an error.
        let w: Vec<String> = validate(&s, None, None).iter().filter(|i| i.severity == Severity::Warning).map(|i| i.to_string()).collect();
        let w = w.join("\n");
        for needle in ["Archmage start (warning): the start building refers to building 4, past the end", "linked building refers to building 7, past the end", "home building refers to building 9, past the end"] {
            assert!(w.contains(needle), "{needle} missing in\n{w}");
        }
    }

    /// The references the original editor's deletes leave as they are (records.md §7, §12)
    /// do not stop the save: a map whose last building, army or named character was deleted
    /// still saves, with warnings. The references the original renumbers stay errors.
    #[test]
    fn what_the_original_deletes_leave_saves() {
        use crate::editor::refs;
        let mut s = map(10, 10);
        s.buildings = vec![building(2, 2, (1, 1)), building(5, 5, (1, 1))];
        s.buildings[0].kind = 2;
        s.buildings[0].linked_building = 2;
        s.armies = vec![Army { id: 1, x: 1, y: 1, model: 4, faction: 4, leader_unit: 1, home_building: 2, named_character: 1, ..Army::default() }, Army { id: 2, x: 3, y: 3, model: 4, faction: 4, leader_unit: 1, ..Army::default() }];
        s.header.heroes[2].start_building = 2;
        s.named_characters = vec![NamedCharacter { unit: 1, name: "N".into() }];
        let mut e = Event { kind: 1, ..Event::default() };
        e.conditions.army_at_home = 2;
        e.conditions.units_named[0] = 1;
        e.results.start_battle_with = 2;
        e.results.patrol_army = 2;
        e.results.units_add_named[0] = 1;
        e.results.units_remove_named[0] = 1;
        s.events = vec![e];
        assert!(errors(&s).is_empty(), "{:?}", errors(&s));
        assert!(refs::remove_building(&mut s, 2));
        assert!(refs::remove_army(&mut s, 2));
        assert!(refs::remove_named_character(&mut s, 1));
        assert!(errors(&s).is_empty(), "{:?}", errors(&s));
        let w = validate(&s, None, None).iter().filter(|i| i.message.contains("past the end")).count();
        // Link, home, start building; army at home, battle, patrol; army 1's character and
        // the event's three.
        assert_eq!(w, 10);
        assert!(self_check(&s).is_ok());
        // A renumbered reference past the end is still an error.
        s.events[0].conditions.meet_army = 5;
        assert!(errors(&s).iter().any(|m| m.contains("refers to army 5, which does not exist")));
    }

    /// The original's 256th point stores id 0 (and a target point model 11; a later point
    /// delete makes it id 255 with that model): warnings, as the game reads such a point.
    #[test]
    fn the_original_256th_point_saves() {
        use crate::editor::defaults::{new_point, LANTERN, TARGET_POINT};
        let mut s = map(10, 10);
        s.points = (1..=256).map(|k| new_point(k, 1, 1, if k == 256 { TARGET_POINT } else { LANTERN })).collect();
        assert_eq!((s.points[255].id, s.points[255].model), (0, 11));
        assert!(errors(&s).is_empty(), "{:?}", errors(&s));
        let w: Vec<String> = validate(&s, None, None).iter().map(|i| i.to_string()).collect();
        assert!(w.iter().any(|m| m.starts_with("Point 256 (warning): stores id 0")), "{w:?}");
        assert!(w.iter().any(|m| m.starts_with("Point 256 (warning): model 11")), "{w:?}");
        assert!(self_check(&s).is_ok());
        crate::editor::refs::remove_point(&mut s, 1);
        assert_eq!((s.points[254].id, s.points[254].model), (255, 11));
        assert!(errors(&s).is_empty(), "{:?}", errors(&s));
        // Any other point with a wrong id, a 257th point or another model is an error.
        s.points[3].id = 0;
        s.points.push(new_point(256, 1, 1, LANTERN));
        s.points.push(new_point(257, 1, 1, LANTERN));
        s.points[0].model = 12;
        let e = errors(&s).join("\n");
        for needle in ["257 points; at most 256 fit", "Point 4 (error): stores id 0", "Point 1 (error): model 12"] {
            assert!(e.contains(needle), "{needle} missing in\n{e}");
        }
    }

    #[test]
    fn ids_must_follow_record_order() {
        let mut s = map(10, 10);
        s.armies = vec![Army { id: 2, model: 4, faction: 4, leader_unit: 1, ..Army::default() }];
        s.points = vec![Point { id: 3, model: 8, ..Point::default() }];
        let e = errors(&s);
        assert!(e.iter().any(|m| m.contains("stores id 2")));
        assert!(e.iter().any(|m| m.contains("stores id 3")));
    }

    #[test]
    fn content_ids_need_names() {
        let mut s = map(10, 10);
        s.header.heroes[0].troops[0] = crate::dt::dtm::Troop { unit: 250, level: 0, count: 1 };
        s.header.heroes[0].spells[0] = 99;
        assert!(errors(&s).is_empty(), "without names nothing is checked");
        let names = Names::from_content(&crate::rules::content::Content::builtin());
        let e: Vec<String> = validate(&s, Some(&names), None).iter().map(|i| i.to_string()).collect();
        assert!(e.iter().any(|m| m.contains("unit 250 is not in the game's unit list")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("spell 99")));
    }

    #[test]
    fn install_palette_checks_pictures() {
        let mut s = map(10, 10);
        s.objects = vec![crate::dt::dtm::MapObject { x: 1, y: 1, class: 9, sprite: 7 }];
        s.buildings = vec![building(5, 5, (2, 2))];
        let p = Palette { objects: vec![ObjectKey { class: 9, sprite: 1 }], buildings: vec![BuildingPicture { picture_type: 3, variant: 0, size: (4, 4), brush: 4 }], from_install: true };
        let all = validate(&s, None, Some(&p));
        assert!(all.iter().any(|i| i.severity == Severity::Error && i.message.contains("object class 9 sprite 7")));
        assert!(all.iter().any(|i| i.severity == Severity::Warning && i.message.contains("differs from its picture's 4x4")));
        let fallback = Palette { from_install: false, ..p };
        assert!(validate(&s, None, Some(&fallback)).iter().all(|i| i.severity != Severity::Error));
    }

    #[test]
    fn strings_nul_and_encoding() {
        let mut s = map(10, 10);
        s.title = "Замок\0".into();
        s.description = "snow ☃".into();
        let all = validate(&s, None, None);
        assert!(all.iter().any(|i| i.severity == Severity::Error && i.message.contains("title contains a NUL")));
        assert!(all.iter().any(|i| i.severity == Severity::Warning && i.message.contains("(☃)")));
        s.title = "Замок".into();
        assert!(!has_errors(&validate(&s, None, None)));
    }

    #[test]
    fn too_many_named_characters() {
        let mut s = map(10, 10);
        s.named_characters = (0..33).map(|_| NamedCharacter { unit: 1, name: "x".into() }).collect();
        assert!(errors(&s).iter().any(|m| m.contains("33 named characters")));
    }

    #[test]
    fn event_references() {
        let mut s = map(10, 10);
        let mut e = Event::default();
        e.conditions.meet_army = 2;
        e.results.chained_event = 5;
        e.results.light_lanterns[0] = 1;
        s.events = vec![e];
        let e = errors(&s).join("\n");
        assert!(e.contains("refers to army 2") && e.contains("refers to event 5") && e.contains("lights point 1"), "{e}");
    }

    #[test]
    fn event_rules() {
        use crate::editor::events::{set_opcode, set_opcode_args};
        let mut s = map(10, 10);
        let quest = Event { kind: 3, ..Event::default() };
        let mut e = Event { kind: 1, ..Event::default() };
        e.results.completes_quest = 3;
        e.results.units_add_named[0] = 1;
        e.conditions.buildings_owner[0] = 7;
        e.title = "Bad%Foo".into();
        let other = Event { kind: 9, archetype: 4, ..Event::default() };
        s.events = vec![e, quest, other];
        let all: Vec<String> = validate(&s, None, None).iter().map(|i| i.to_string()).collect();
        assert!(all.iter().any(|m| m.contains("(warning): units added: named character 1 is past the end")), "{all:?}");
        let e = errors(&s).join("\n");
        for needle in [
            "Event 1 (error): completes event 3, which is not a quest",
            "owner code 7",
            "flag script: the flag action \"Foo\" must start with +",
            "Event 3 (error): type 9 is not one of 1-4",
            "hero archetype 4",
        ] {
            assert!(e.contains(needle), "{needle} missing in\n{e}");
        }
        s.events[0].results.completes_quest = 2;
        s.events[0].title = "Good%+Foo=/Bar".into();
        s.events[0].results.units_add_named[0] = 0;
        s.events[0].conditions.buildings_owner[0] = 6;
        s.events[2] = Event { kind: 4, ..Event::default() };
        assert!(errors(&s).is_empty(), "{:?}", errors(&s));
        // Content ids of events are checked against the install's names.
        let names = Names::from_content(&crate::rules::content::Content::builtin());
        s.events[0].results.units_remove = [0xFE, 0xFF, 250, 0];
        s.events[0].results.spells_learned[0] = 99;
        s.events[0].results.picture = 201;
        let e: Vec<String> = validate(&s, Some(&names), None).iter().map(|i| i.to_string()).collect();
        assert!(e.iter().any(|m| m.contains("unit 250")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("spell 99")));
        assert_eq!(e.iter().filter(|m| m.contains("unit ")).count(), 1, "0xFE, 0xFF and the victory picture are not units: {e:?}");
        // Opcode arguments that name nothing are warnings; the patrol byte is not an army.
        s.events[0].results.units_remove = [0; 4];
        s.events[0].results.spells_learned[0] = 0;
        set_opcode(&mut s.events[1], Some(13));
        set_opcode_args(&mut s.events[1], [4, -1, 100]);
        s.events[1].results.patrol_army = 99;
        let all = validate(&s, None, None);
        assert!(!has_errors(&all), "{all:?}");
        assert!(all.iter().any(|i| i.severity == Severity::Warning && i.message.contains("opcode 13") && i.message.contains("= 4")));
        set_opcode(&mut s.events[1], Some(1));
        set_opcode_args(&mut s.events[1], [-5, 84, 1]);
        let w: Vec<String> = validate(&s, None, None).iter().map(|i| i.message.clone()).collect();
        assert!(w.iter().any(|m| m.contains("Target event") && m.contains("-5")) && w.iter().any(|m| m.contains("Field") && m.contains("84")), "{w:?}");
        set_opcode(&mut s.events[1], None);
        s.events[1].results.no_meeting = 0;
        // The patrol army is not renumbered by the original's army delete: a warning.
        assert!(errors(&s).is_empty());
        assert!(validate(&s, None, None).iter().any(|i| i.severity == Severity::Warning && i.message.contains("patrol change refers to army 99, past the end")));
    }

    #[test]
    fn too_many_events() {
        let mut s = map(4, 4);
        s.events = vec![Event { kind: 1, ..Event::default() }; 5001];
        assert!(errors(&s).iter().any(|m| m.contains("5001 events")));
    }

    #[test]
    fn string_counts_match_the_records() {
        let mut s = map(6, 6);
        s.events = vec![Event { kind: 1, title: "A".into(), custom_picture: Some(vec![1, 0, 1, 0, 0, 0]), ..Event::default() }];
        s.named_characters = vec![NamedCharacter { unit: 1, name: "N".into() }];
        let bytes = self_check(&s).unwrap();
        assert_eq!(count_strings(&bytes), Some(4 + 3 + 1));
    }

    #[test]
    fn self_check_accepts_a_valid_map() {
        let mut s = map(6, 6);
        s.buildings = vec![building(3, 3, (2, 2))];
        s.title = "Test".into();
        let bytes = self_check(&s).unwrap();
        assert_eq!(Scenario::parse_payload(&bytes).unwrap().title, "Test");
    }
}
