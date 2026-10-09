//! The world map: terrain, buildings (locations), armies and the hero's start.
//!
//! Built either from an original scenario ([`World::from_scenario`], `docs/reference/dtm-format.md`)
//! or from the built-in demo kingdom ([`World::standard`], `data/kingdom.txt`), through the
//! same types.

use std::collections::HashMap;

use crate::i18n::{n_, tr};

use crate::dt::dtm::{self, Archetype, BuildingType, EventKind, Scenario};

use super::clock::Clock;
use super::content::{Content, HeroClass, ItemId, Nature, UnitId, WageKind};
use super::formation::{Row, Slot};
use super::ai::{AiMind, AiProfile, Respawn};
use super::magic::ActiveSpell;
use super::map::{is_water, Decoration, Grid, Tile, TileMap, BASE_SPEED};
use super::units::Unit;

const KINGDOM: &str = include_str!("../../data/kingdom.txt");

/// Attitude value at or below which a side attacks the player (relations run −3..3).
pub const HOSTILE_BELOW: i8 = 0;

/// [`Location::relations`] of a save written before buildings kept them.
pub const UNKNOWN_RELATIONS: [i8; 4] = [i8::MIN; 4];

fn unknown_relations() -> [i8; 4] {
    UNKNOWN_RELATIONS
}

/// A unit in an army or a garrison: the original's unit record (ai.md §1) as the AI keeps
/// it, its worn items, death and pay included.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Troop {
    pub unit: UnitId,
    /// 1 as hired.
    pub level: i32,
    pub slot: Slot,
    /// Hit points it lacks: it has its maximum (worn items included) minus this. 0 unhurt.
    #[serde(default)]
    pub hurt: i32,
    /// Experience towards the next level (AI armies gain it in their own battles).
    #[serde(default)]
    pub xp: i32,
    /// Worn items, as a unit's ([`Unit::items`]).
    #[serde(default)]
    pub worn: [Option<ItemId>; crate::rules::items::SLOTS],
    /// Game minute of death: a corpse stays in the record (it may be resurrected, and it
    /// comes back with a respawn) until the AI drops it (ai.md §9).
    #[serde(default)]
    pub died_at: Option<u64>,
    /// Unused: the time of death a unit raised again kept in saves from before Razdor fixed
    /// the original's bug (it cleared it only at a respawn or an activation, so a unit that
    /// fell again counted from its first death; ai.md §9, §10).
    #[serde(default)]
    pub kept_death: Option<u64>,
    /// Missed a payday: it stays out of the battles its army starts (economy.md §1).
    #[serde(default)]
    pub unpaid: bool,
    /// Game minute it was last paid; it may leave on a short noon long after.
    #[serde(default)]
    pub last_paid: u64,
    /// Hiring kind for the wage formula: AI leaders are [`WageKind::Leader`].
    #[serde(default = "recruit")]
    pub kind: WageKind,
    /// Lasting world spells on it, its drain and HP carry, as a unit's ([`Unit::spells`]).
    #[serde(default)]
    pub spells: [Option<crate::rules::units::SpellSlot>; crate::rules::units::SPELL_SLOTS],
    #[serde(default)]
    pub drain: i32,
    #[serde(default)]
    pub carry: crate::rules::units::HpCarry,
    /// The named character it is (unit +0x14, 1-based); 0 for an ordinary unit.
    #[serde(default)]
    pub named: u8,
    /// Its first `personal` worn slots hold its own items, not to be taken off while it lives
    /// (unit +0x18, see [`crate::rules::units::Unit::personal`]).
    #[serde(default)]
    pub personal: u8,
}

fn recruit() -> WageKind {
    WageKind::Recruit
}

impl Troop {
    /// A troop at full health, a recruit.
    pub fn new(unit: UnitId, level: i32, slot: Slot) -> Troop {
        Troop {
            unit,
            level,
            slot,
            hurt: 0,
            xp: 0,
            worn: [None; crate::rules::items::SLOTS],
            died_at: None,
            kept_death: None,
            unpaid: false,
            last_paid: 0,
            kind: WageKind::Recruit,
            spells: [None; crate::rules::units::SPELL_SLOTS],
            drain: 0,
            carry: crate::rules::units::HpCarry(0.0),
            named: 0,
            personal: 0,
        }
    }

    pub fn alive(&self) -> bool {
        self.died_at.is_none()
    }
}

/// Places `(unit, level, count)` entries into free formation cells, each unit in its preferred
/// row, around the already `occupied` cells. Unknown unit ids and units beyond the formation's
/// capacity are dropped; returns the troops and how many units were dropped.
///
/// The `.DTm` troop triple is (unit, extra level, count): the middle byte is 0 for almost
/// every troop and the last byte is 1–9, so the middle one is levels above the first *(L)*.
pub fn place_troops(content: &Content, occupied: &[Slot], entries: &[(u32, i32, i32)]) -> (Vec<Troop>, usize) {
    let mut taken = occupied.to_vec();
    let mut out = Vec::new();
    let mut dropped = 0;
    for &(unit, level, count) in entries {
        let id = UnitId(unit);
        if content.try_unit(id).is_none() {
            dropped += count.max(0) as usize;
            continue;
        }
        // Each unit takes the first free cell from the reserve forward (495ce0).
        for _ in 0..count.max(0) {
            match content.formation.new_unit_slot(&taken) {
                Some(slot) if taken.len() < content.formation.capacity() => {
                    taken.push(slot);
                    out.push(Troop::new(id, level.max(1), slot));
                }
                _ => dropped += 1,
            }
        }
    }
    (out, dropped)
}

/// The troop triples of a preset, an army or a garrison. Unit ids 1–3 (the hero types) are
/// skipped, as the map loader adds only a unit above 3 (0x4b2504); an army's leader is not
/// a triple and may be one.
fn dt_entries(troops: &[dtm::Troop]) -> Vec<(u32, i32, i32)> {
    troops.iter().filter(|t| t.unit > 3 && t.count > 0).map(|t| (t.unit as u32, t.level as i32 + 1, t.count as i32)).collect()
}

/// The 16 building types of the original plus the demo's bandit camp.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LocationKind {
    Palace,
    Town,
    Village,
    Castle,
    Fort,
    Tavern,
    Market,
    Church,
    Smithy,
    Shipyard,
    Altar,
    Entrance,
    Ruins,
    StoneBridge,
    WoodenBridge,
    Obelisk,
    /// Demo only: a bandit camp with a garrison and a reward.
    Camp,
}

impl LocationKind {
    pub fn from_building(t: BuildingType) -> LocationKind {
        use LocationKind as K;
        match t {
            BuildingType::Palace => K::Palace,
            BuildingType::Town => K::Town,
            BuildingType::Village => K::Village,
            BuildingType::Castle => K::Castle,
            BuildingType::Fort => K::Fort,
            BuildingType::Tavern => K::Tavern,
            BuildingType::Market => K::Market,
            BuildingType::Church => K::Church,
            BuildingType::Smithy => K::Smithy,
            BuildingType::Shipyard => K::Shipyard,
            BuildingType::Altar => K::Altar,
            BuildingType::DungeonEntrance => K::Entrance,
            BuildingType::Ruins => K::Ruins,
            BuildingType::StoneBridge => K::StoneBridge,
            BuildingType::WoodenBridge => K::WoodenBridge,
            BuildingType::Obelisk => K::Obelisk,
        }
    }

    pub fn is_bridge(self) -> bool {
        matches!(self, LocationKind::StoneBridge | LocationKind::WoodenBridge)
    }

    /// Castles and forts are taken by beating their garrison.
    pub fn capturable(self) -> bool {
        matches!(self, LocationKind::Castle | LocationKind::Fort)
    }

    /// Kinds whose garrison fights a hostile visitor (ruins guard their treasure).
    pub fn defends(self) -> bool {
        matches!(self, LocationKind::Castle | LocationKind::Fort | LocationKind::Ruins | LocationKind::Camp)
    }

    pub fn label(self) -> &'static str {
        use LocationKind::*;
        match self {
            Palace => tr("Palace"),
            Town => tr("Town"),
            Village => tr("Village"),
            Castle => tr("Castle"),
            Fort => tr("Fort"),
            Tavern => tr("Tavern"),
            Market => tr("Market"),
            Church => tr("Church"),
            Smithy => tr("Smithy"),
            Shipyard => tr("Shipyard"),
            Altar => tr("Altar"),
            Entrance => tr("Dungeon entrance"),
            Ruins => tr("Ruins"),
            StoneBridge => tr("Stone bridge"),
            WoodenBridge => tr("Wooden bridge"),
            Obelisk => tr("Obelisk"),
            Camp => tr("Bandit camp"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Owner {
    Player,
    /// An army of the scenario, by its 1-based id.
    Army(u8),
    /// The building's own (neutral) owner.
    Neutral,
}

/// A unit type a barracks offers. `stock: None` = unlimited (the demo).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Recruit {
    pub unit: UnitId,
    pub stock: Option<i32>,
    /// The editor's maximum (at least the start count).
    pub max: i32,
    /// Unused (the regrowth is a daily roll, `rules::economy::regrow`); kept for saves.
    pub progress: i32,
    /// Its slot in the building's six barracks slots (0–5): the AI's hiring scans the slots,
    /// empty ones included (ai.md §9.5). Scenario data, restored on load.
    #[serde(skip)]
    pub slot: u8,
}

impl Recruit {
    pub fn new(unit: UnitId, start: i32, max: i32) -> Recruit {
        Recruit { unit, stock: Some(start.max(0)), max: max.max(start).max(0), progress: 0, slot: 0 }
    }
}

/// A player's unit left in a garrison (its whole record: kind, paid mark, last pay).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Stationed {
    pub unit: Unit,
}

/// A scenario event id (1-based, file order), as buildings list them.
pub type EventId = u16;

/// What the building screens show of a scenario event (the event engine is separate).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventInfo {
    pub kind: Option<EventKind>,
    /// The title without its flag script.
    pub title: String,
}

/// A lantern or event point of the scenario (`docs/reference/dtm-format.md` §8). Events
/// refer to it by `id`; the hero standing on `tile` is "at the point".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapPoint {
    pub id: u8,
    pub tile: Tile,
    /// Radius a lit lantern reveals.
    pub radius: i32,
    /// Lit from the start: active (byte 40) with a radius; the model byte (8 lantern, 9 event
    /// point) is not looked at (0x4b5a37).
    pub lit: bool,
}

/// The places of a market (goods words 1..12 of a building).
pub const MARKET_PLACES: usize = 12;

/// A good in a market place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Good {
    pub item: ItemId,
    /// One of the map's goods (a negative id in the original): kept until bought, never
    /// redrawn, and left alone by the AI.
    pub fixed: bool,
}

/// Items for sale (economy.md §2): the 12 places in order, with the map's fixed goods where
/// the map put them and random goods drawn into the empty places at each restock.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(from = "ShopSave")]
pub struct Shop {
    /// [`MARKET_PLACES`] places; `None` is an empty one.
    pub places: Vec<Option<Good>>,
    /// Random goods drawn at each restock (byte 295).
    pub random: usize,
    /// The price window of the random goods (words 333 and 335, as the map load set them).
    pub price: (i32, i32),
    /// The restock timer (minutes): due when it is above 0 and not after now; a restock
    /// with random goods sets it 12 hours on (0x4be178).
    #[serde(default)]
    pub timer: u64,
}

/// A saved shop: older saves kept the goods as a list and the fixed ones apart.
#[derive(serde::Deserialize)]
struct ShopSave {
    #[serde(default)]
    places: Option<Vec<Option<Good>>>,
    random: usize,
    price: (i32, i32),
    #[serde(default)]
    timer: Option<u64>,
    #[serde(default)]
    fixed: Vec<ItemId>,
    #[serde(default)]
    stock: Vec<ItemId>,
}

impl From<ShopSave> for Shop {
    fn from(s: ShopSave) -> Shop {
        let places = s.places.unwrap_or_else(|| {
            let mut fixed = s.fixed.clone();
            let mut places: Vec<Option<Good>> = s
                .stock
                .iter()
                .map(|&item| {
                    let k = fixed.iter().position(|&x| x == item);
                    Some(Good { item, fixed: k.map(|k| fixed.remove(k)).is_some() })
                })
                .collect();
            places.resize(MARKET_PLACES.max(places.len()), None);
            places
        });
        Shop { places, random: s.random, price: s.price, timer: s.timer.unwrap_or(1) }
    }
}

impl Shop {
    /// A market as the map load sets it up (0x4b5200 area): the map's goods fixed in their
    /// places, the timer set; with random goods the top of the price window is capped at
    /// the dearest item's Cost (0 means that cap) and the bottom set to 0 unless below it.
    pub fn from_map(places: Vec<Option<ItemId>>, random: usize, (mut lo, mut hi): (i32, i32), dearest: i32) -> Shop {
        let mut places: Vec<Option<Good>> = places.into_iter().map(|i| i.map(|item| Good { item, fixed: true })).collect();
        places.resize(MARKET_PLACES, None);
        if random > 0 {
            hi = hi.min(dearest);
            if hi <= lo {
                lo = 0;
            }
            if hi == 0 {
                hi = dearest;
            }
        }
        Shop { places, random, price: (lo, hi), timer: 1 }
    }

    /// The goods on sale, in place order with the empty places left out (as the window
    /// lists them).
    pub fn goods(&self) -> Vec<ItemId> {
        self.places.iter().flatten().map(|g| g.item).collect()
    }

    /// The place of the `k`-th good on sale.
    fn place_of(&self, k: usize) -> Option<usize> {
        self.places.iter().enumerate().filter(|(_, p)| p.is_some()).nth(k).map(|(i, _)| i)
    }

    /// Whether the `k`-th good on sale is one of the map's.
    pub fn is_fixed(&self, k: usize) -> bool {
        self.place_of(k).and_then(|i| self.places[i]).is_some_and(|g| g.fixed)
    }

    /// Takes the `k`-th good on sale: its place is emptied.
    pub fn take(&mut self, k: usize) -> Option<ItemId> {
        let i = self.place_of(k)?;
        self.places[i].take().map(|g| g.item)
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Location {
    /// 1-based building id in the scenario (events refer to it); 0 in the demo.
    pub id: u16,
    /// Texts of the scenario: not saved, restored from it on load.
    #[serde(skip)]
    pub name: String,
    /// The neutral owner's name (a village headman, a lord …).
    #[serde(skip)]
    pub owner_name: String,
    #[serde(skip)]
    pub description: String,
    pub kind: LocationKind,
    /// Centre of the footprint `(x0 + sx/2, y0 + sy/2)`, where AI armies stand (world.md §7).
    /// There is no entry cell: stepping onto any footprint cell enters the building.
    pub tile: Tile,
    /// Bottom-right cell of the footprint.
    pub anchor: Tile,
    /// Footprint size in cells (x, y), at least 1×1.
    pub size: (i32, i32),
    /// Sprite: `Objects.ugs` section-B (picture type, variant). The demo borrows pictures of
    /// the same kinds.
    pub picture: (u8, u8),
    pub owner: Owner,
    /// 1 player, 2 ally, 3 neighbour, 4 enemy.
    pub faction: u8,
    /// Attitude towards the player, −3..3.
    pub attitude: i8,
    /// Attitudes towards the four factions (byte 338); the one towards the player's faction
    /// is [`Location::attitude`] ([`Location::attitude_to`]). The AI reads the one towards
    /// its own faction (ai.md §3). Saves from before them read [`UNKNOWN_RELATIONS`] and take
    /// the scenario's on load.
    #[serde(default = "unknown_relations")]
    pub relations: [i8; 4],
    /// Byte 294, the services flag: AI armies heal, resurrect and hire here (ai.md §9.4).
    /// Scenario data, restored on load.
    #[serde(skip)]
    pub services: bool,
    /// Daily gold and mana for the owner (castles, forts, towns) or, for villages, the
    /// tribute that accumulates up to the maximum.
    pub gold_income: i32,
    pub gold_max: i32,
    pub mana_income: i32,
    pub mana_max: i32,
    /// Village tribute waiting to be collected.
    pub tribute_gold: i32,
    pub tribute_mana: i32,
    /// The scenario's garrison (the owner's troops).
    pub garrison: Vec<Troop>,
    pub garrison_defence: i32,
    /// Its garrison's unit strengths (+0x1ae) are still the ones of the map load's recount,
    /// counted before the ruins' items were given to its units (0x4b53dd, 0x4b55aa): its
    /// battles count them without those items until the garrison's next recount
    /// (0x4a16d4: after a real battle, a garrison purchase or a reshuffle).
    #[serde(default)]
    pub strengths_bare: bool,
    /// The player's units left here (his castles and forts).
    pub stationed: Vec<Stationed>,
    pub recruits: Vec<Recruit>,
    /// Recruiting any listed type is allowed (the "all types" flag).
    pub recruit_all_types: bool,
    pub shop: Option<Shop>,
    /// Ruins: treasure gold and items; the demo's camps: reward and loot rolls.
    pub treasure_gold: i32,
    /// Ruins: the garrison's pack (its worn items are on its troops).
    pub treasure: Vec<ItemId>,
    /// Ruins: the goods words of the building record, which the original keeps as the map
    /// has them (only the diff test's state reads them).
    #[serde(skip)]
    pub map_goods: Vec<i32>,
    pub loot_rolls: u32,
    /// Spells taught here (1-based spell index).
    pub spells: Vec<u8>,
    /// Local events (1-based event ids).
    pub events: Vec<u16>,
    /// Linked building (a village's castle), index into `locations`.
    pub linked: Option<usize>,
    /// Garrison beaten / treasure taken.
    pub cleared: bool,
}

impl Location {
    fn new(kind: LocationKind, name: &str, tile: Tile) -> Location {
        Location {
            id: 0,
            name: name.to_string(),
            owner_name: String::new(),
            description: String::new(),
            kind,
            tile,
            anchor: tile,
            size: (1, 1),
            picture: (0, 0),
            owner: Owner::Neutral,
            faction: 3,
            attitude: 1,
            relations: [1, 0, 0, 0],
            services: false,
            gold_income: 0,
            gold_max: 0,
            mana_income: 0,
            mana_max: 0,
            tribute_gold: 0,
            tribute_mana: 0,
            garrison: Vec::new(),
            garrison_defence: 0,
            strengths_bare: false,
            stationed: Vec::new(),
            recruits: Vec::new(),
            recruit_all_types: false,
            shop: None,
            treasure_gold: 0,
            treasure: Vec::new(),
            map_goods: Vec::new(),
            loot_rolls: 0,
            spells: Vec::new(),
            events: Vec::new(),
            linked: None,
            cleared: false,
        }
    }

    /// Footprint cells (world.md §7): `size` cells up and to the left of the anchor, plus
    /// one extra row above when the building is wider than tall.
    pub fn cells(&self) -> impl Iterator<Item = Tile> + '_ {
        let (ax, ay) = self.anchor;
        let rows = self.size.1 + i32::from(self.size.0 > self.size.1);
        (0..rows).flat_map(move |j| (0..self.size.0).map(move |i| (ax - i, ay - j)))
    }

    /// Centre of the footprint `(x0 + sx/2, y0 + sy/2)` (world.md §7).
    pub fn centre(&self) -> Tile {
        let (x0, y0) = (self.anchor.0 - self.size.0 + 1, self.anchor.1 - self.size.1 + 1);
        (x0 + self.size.0 / 2, y0 + self.size.1 / 2)
    }

    /// Closes the hero's route (world.md §1.3, 0x4cc148): a castle or fort whose attitude to
    /// him is at most 0, and ruins that are not his (the demo's camps as forts until burnt).
    /// The route goes around it unless it is the building clicked or the one he stands in;
    /// every other building, ill-disposed or not, is walked through.
    pub fn bars_hero(&self) -> bool {
        match self.kind {
            LocationKind::Castle | LocationKind::Fort => self.attitude <= 0,
            LocationKind::Ruins => !self.owned(),
            LocationKind::Camp => !self.cleared && self.attitude <= 0,
            _ => false,
        }
    }

    pub fn owned(&self) -> bool {
        self.owner == Owner::Player
    }

    /// Attitude towards faction `f` (1–4): the original reads its byte `+0x151 + f`; another
    /// value reads as 0.
    pub fn attitude_to(&self, f: u8) -> i8 {
        match f {
            1 => self.attitude,
            2..=4 => self.relations[f as usize - 1],
            _ => 0,
        }
    }

    /// Takes the faction and the four attitudes of a new owner.
    pub fn take_sides(&mut self, faction: u8, relations: [i8; 4]) {
        self.faction = faction;
        self.attitude = relations[0];
        self.relations = relations;
    }

    /// Not the player's and ill-disposed towards him.
    pub fn hostile(&self) -> bool {
        !self.owned() && self.attitude < HOSTILE_BELOW
    }

    /// A garrison that fights the player when he steps in.
    pub fn defended(&self) -> bool {
        self.kind.defends() && self.hostile() && !self.cleared && !self.garrison.is_empty()
    }

    /// The player may leave troops here: his own castles and forts.
    pub fn takes_garrison(&self) -> bool {
        self.owned() && self.kind.capturable()
    }

    /// The player's hire tab (0x4bbc84, economy.md §7): some barracks slot holds a unit,
    /// and every slot's unit is of ordinary Nature or the "all types" byte is set. No
    /// attitude or type test, beyond the types with no building window (villages and
    /// shipyards have their own; bridges and the obelisk none).
    pub fn hires(&self, c: &Content) -> bool {
        use LocationKind::*;
        let ordinary = |r: &Recruit| c.try_unit(r.unit).is_some_and(|d| matches!(d.nature, Nature::Normal | Nature::People));
        !matches!(self.kind, Village | Shipyard | StoneBridge | WoodenBridge | Obelisk | Camp)
            && !self.recruits.is_empty()
            && (self.recruit_all_types || self.recruits.iter().all(ordinary))
    }

    /// Income the owner receives each day (villages pay tribute instead).
    pub fn pays_income(&self) -> bool {
        !matches!(self.kind, LocationKind::Village)
    }
}

/// The steps an army took while the hero made one step or wait tick (drawing only). The
/// original plays them within that step's real time (world.md §2, 0x4a39d0): a step it could
/// follow with another lasts its share `minutes / banked` of the window, the last one fills
/// the rest, so every army walks at the same time as the hero.
#[derive(Clone, Debug, Default)]
pub struct Walk {
    /// Where it stood when the stretch began, then each cell it reached.
    pub points: Vec<(f32, f32)>,
    /// Minutes each step cost.
    pub minutes: Vec<f32>,
    /// Minutes banked for the stretch (its budget plus the stretch's time, capped).
    pub banked: f32,
}

impl Walk {
    /// Where the army is `k` (0..1) of the way through the window, if it walked.
    pub fn at(&self, k: f32) -> Option<(f32, f32)> {
        let n = self.minutes.len();
        if n == 0 || self.points.len() != n + 1 {
            return None;
        }
        let k = k.clamp(0.0, 1.0);
        let mut start = 0.0;
        for i in 0..n {
            let share = if i + 1 == n { 1.0 - start } else { self.minutes[i] / self.banked.max(1e-3) };
            let end = (start + share).min(1.0);
            if k < end || i + 1 == n {
                let t = if end > start { ((k - start) / (end - start)).clamp(0.0, 1.0) } else { 1.0 };
                let (a, b) = (self.points[i], self.points[i + 1]);
                return Some((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
            start = end;
        }
        None
    }
}

/// An army on the map (an AI lord, a gang, peasants) or, in the demo, a bandit gang.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Army {
    /// 1-based army id in the scenario; 0 for the demo's gangs.
    pub id: u8,
    /// Unique among the world's armies for the whole game (a spell's target is followed by it).
    pub uid: u32,
    /// Texts of the scenario: not saved, restored from it on load.
    #[serde(skip)]
    pub name: String,
    #[serde(skip)]
    pub leader_name: String,
    #[serde(skip)]
    pub description: String,
    /// Map model (`.DTm` army byte 5): 4 feudal, 5 bandits, 6 peasants, … The editor's
    /// picture only: the game's loader never reads it; the figure on the map is [`Army::figure`].
    pub model: u8,
    /// The figure on the map (army +0x169d): an index into the 13 sprite names at 0x4ed238
    /// (`ui::world_view::FIGURES`), from [`Army::figure_for`] at load, set by event opcode 17.
    /// [`NO_FIGURE`] in saves from before it: [`World::restore_statics`] takes the scenario's.
    #[serde(default = "no_figure")]
    pub figure: u8,
    /// Position in world units (see `map::center`).
    pub pos: (f32, f32),
    /// Home building, index into `locations`.
    pub home: Option<usize>,
    /// Its home cell, the centre of its patrol.
    pub post: Tile,
    /// The centre of its patrol box when that is no longer its post: an event moved it to
    /// the hero, and the original does not recompute the box (world.md §4.4, 0x4980d8).
    #[serde(default)]
    pub box_centre: Option<Tile>,
    pub patrols: bool,
    pub patrol_radius: i32,
    pub troops: Vec<Troop>,
    /// 1 player, 2 ally, 3 neighbour, 4 enemy.
    pub faction: u8,
    /// Attitude towards the player, −3..3.
    pub attitude: i8,
    /// Gold carried (loot basis).
    pub gold: i32,
    pub items: Vec<ItemId>,
    /// Minutes per cost unit of a step (the hero's knight is 5; [`Army::speed_for`]).
    /// 0 in saves from before speeds: [`World::restore_statics`] takes the scenario's.
    #[serde(default)]
    pub speed: u32,
    /// Game minutes banked towards its next step (world.md §2: the hero's steps and waits
    /// feed it, up to [`AI_BUDGET_CAP`]).
    #[serde(default)]
    pub budget: f32,
    /// The steps it took in the last stretch of time, for drawing ([`Walk`]).
    #[serde(skip)]
    pub walk: Walk,
    pub path: Vec<Tile>,
    pub chasing: bool,
    /// Game minute until which it leaves the player alone (after a stalemate).
    pub ignore_until: f64,
    /// Its talk counter towards the hero (world.md §4.3): it greets him when above 0, then
    /// it is set to −500; it grows as the army steps.
    #[serde(default)]
    pub talk: i32,
    /// It took (or tried) a step in the last stretch of time: only then does it attack or
    /// greet the hero (world.md §4.3).
    #[serde(skip)]
    pub arrived: bool,
    /// Game minute until which it stands still between patrol legs.
    pub rest_until: f64,
    /// The named character leading it in saves from before each troop kept its own
    /// ([`Troop::named`]); a load moves it onto the leader.
    #[serde(default, skip_serializing)]
    pub named: u8,
    /// Army-wide world spells of saves before format 7; a load moves them into the units'
    /// slots (`rules::save`).
    #[serde(default, rename = "effects", skip_serializing)]
    pub old_effects: Vec<ActiveSpell>,
    /// Ship type (`.DTm` army byte 72): 0 a land army, else it sails (`rules::ships::kind`).
    #[serde(default)]
    pub ship: u8,
    /// What the scenario says about its behaviour (`rules::ai`); `enabled` is false for the
    /// demo's gangs, which keep the simple chase-and-patrol rules.
    #[serde(default)]
    pub ai: AiProfile,
    /// What the AI keeps of it between its steps: scores, talk counters, plans (`rules::ai`).
    #[serde(default)]
    pub mind: AiMind,
}

fn shown_army_name(name: &str) -> &str {
    name.split('#').next().unwrap_or_default().trim_end_matches(' ')
}

impl Army {
    /// The name the player sees (0x4973a0): the scenario's name up to its first `#` (the
    /// editor's numbering, `Призрак#2`), trailing spaces dropped.
    pub fn shown_name(&self) -> &str {
        shown_army_name(&self.name)
    }

    pub fn tile(&self, map: &TileMap) -> Tile {
        map.tile_at(self.pos)
    }

    /// The centre of its patrol box: its post, unless an event moved the post away from it.
    pub fn patrol_centre(&self) -> Tile {
        self.box_centre.unwrap_or(self.post)
    }

    /// Attacks the player on contact and chases him (its `attitude` is the relation of
    /// [`relation`], worked out at load and when events change its faction or attitudes).
    pub fn hostile(&self) -> bool {
        self.attitude < HOSTILE_BELOW
    }

    /// The troop in the middle of the front row, else the first one.
    pub fn leader(&self) -> Option<UnitId> {
        self.troops.first().map(|t| t.unit)
    }

    /// A ship: it moves on open water only (`rules::ships`).
    pub fn sails(&self) -> bool {
        self.ship != 0
    }

    /// Speed of an army from the editor's speed correction (world.md §2): `max(1, 5 − c)`,
    /// then one less when its leader is the Archmage unit (GlobalIndex 2; the original's
    /// loader does so, probably meaning the Ranger), at least 1. One point is 20% of the time.
    pub fn speed_for(correction: i8, leader: u32) -> u32 {
        let s = (BASE_SPEED as i32 - correction as i32).max(1);
        let s = if leader == ARCHMAGE_UNIT { s - 1 } else { s };
        s.max(1) as u32
    }

    /// The figure the loader gives a map army on land (0x4b4824, the code at 0x4b4a10):
    /// 5, then by its style (byte 59) 0 → 6 (Knight), 1 → 4 (Rogue), 2 → 5 (Peasant). Then
    /// its leader's type t (unit 1, 0-based: GlobalIndex − 1; 0 with no leader, the record
    /// being cleared) overrides it: an Undead leader is 8 (Zombie) for t 42–44 and 46, 9
    /// (Ghost) for t 50–52, else 7 (Necromant); any other leader whose bit is set in the
    /// 88-bit table at 0x4b5afc (the priests, mages and witches) is 10 (Mage). Byte 5 (the
    /// editor's picture) is never read. A ship army's figure is set after this
    /// ([`Army::ship_figure`]).
    pub fn figure_for(content: &Content, a: &dtm::Army) -> u8 {
        let mut figure = match a.behaviour {
            0 => 6,
            1 => 4,
            _ => 5,
        };
        let t = u32::from(a.leader_unit.saturating_sub(1));
        let nature = content.try_unit(UnitId(t + 1)).map_or(Nature::Normal, |u| u.nature);
        if nature == Nature::Undead {
            figure = match t {
                42..=44 | 46 => 8,
                50..=52 => 9,
                _ => 7,
            };
        } else if t < 88 && MAGE_FIGURE_BITS[(t >> 3) as usize] >> (t & 7) & 1 != 0 {
            figure = 10;
        }
        figure
    }

    /// The figure of an army loaded afloat, by its ship byte 72 (0x4b4a90): 0 and 1 the
    /// hero's galley (3), 2 the pirate (12), 3 the merchant (11); another value keeps its
    /// land figure.
    pub fn ship_figure(byte_72: u8, land: u8) -> u8 {
        match byte_72 {
            0 | 1 => 3,
            2 => 12,
            3 => 11,
            _ => land,
        }
    }
}

/// The leader types (0-based) that get the Mage figure: the bit table at 0x4b5afc.
const MAGE_FIGURE_BITS: [u8; 11] = [0x02, 0x00, 0x00, 0xfe, 0x73, 0x00, 0x00, 0x00, 0x40, 0x08, 0x0c];

/// [`Army::figure`] not known yet (a save from before it).
pub const NO_FIGURE: u8 = 0xFF;

fn no_figure() -> u8 {
    NO_FIGURE
}

/// The end minute of a map army's own spell (army byte 84): 0x3dcc5000, "for good".
pub const MAP_ARMY_SPELL_END: u64 = 1_036_800_000;

/// The Archmage's unit (GlobalIndex 2), whose armies the original's loader makes faster.
const ARCHMAGE_UNIT: u32 = 2;
/// Most game minutes an army banks towards its steps (world.md §2).
pub const AI_BUDGET_CAP: f32 = 200.0;

/// Where and with what the hero starts.
#[derive(Clone, Debug, PartialEq)]
pub struct HeroStart {
    pub class: HeroClass,
    pub tile: Tile,
    pub gold: i32,
    pub mana: i32,
    /// Troops besides the hero; the hero stands in `hero_slot`.
    pub hero_slot: Slot,
    pub troops: Vec<Troop>,
    pub items: Vec<ItemId>,
    pub spells: Vec<u8>,
    /// Location the hero starts in, if any.
    pub location: Option<usize>,
    /// Buildings given to him at the start: the preset's start building and those flagged
    /// for his class.
    pub owned: Vec<usize>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct World {
    /// Statics (the map, texts, indexes) are not saved: they are rebuilt from the scenario
    /// when a game is loaded ([`World::restore_statics`]).
    #[serde(skip)]
    pub title: String,
    #[serde(skip)]
    pub map: TileMap,
    pub locations: Vec<Location>,
    /// Armies on the map.
    pub armies: Vec<Army>,
    /// Armies not on the map yet (inactive at start, events bring them in; and ships).
    pub inactive: Vec<Army>,
    /// Troops of a newly spawned demo gang.
    pub gang: Vec<Troop>,
    /// When the game starts.
    pub start: Clock,
    /// Relations between the four factions (player, ally, neighbour, enemy), −3..3.
    pub relations: [[i8; 4]; 4],
    /// The built-in demo (its own flavour rules: gangs, item tribute, weekly markets).
    pub demo: bool,
    /// Units of the scenario that the content does not know or that did not fit.
    pub dropped_units: usize,
    /// The scenario's events by id − 1 (titles and kinds only).
    #[serde(skip)]
    pub events: Vec<EventInfo>,
    /// Lanterns and event points.
    #[serde(skip)]
    pub points: Vec<MapPoint>,
    /// Names of the scenario's named characters, by id − 1.
    #[serde(skip)]
    pub named_characters: Vec<String>,
    /// Last [`Army::uid`] handed to a spawned demo gang.
    pub next_uid: u32,
    /// Beaten armies waiting to come back: lords recovering in a building, and armies with
    /// a respawn time (`rules::ai`).
    #[serde(default)]
    pub respawns: Vec<Respawn>,
    /// Connected region of every passable cell on foot (`TileMap::regions`), to skip goals
    /// an AI army cannot walk to. Rebuilt from the map.
    #[serde(skip)]
    pub(crate) regions: Vec<u32>,
    #[serde(skip)]
    footprints: HashMap<Tile, usize>,
    /// Water the hero sails on, a `w*h` mask (`rules::ships`): shallows and coastal water
    /// that is not a building's footprint.
    #[serde(skip)]
    pub(crate) sea: Vec<bool>,
}

/// How far an army is moved to find a cell of its kind (land, or water for a ship).
pub const PLACE_RADIUS: i32 = 8;

pub const GANG_REWARD: i32 = 30;
/// What the demo calls its roaming gangs.
const GANG_NAME: &str = n_("Bandit gang");
/// Uids above the scenario's army ids (1..=255) go to the demo's gangs.
const FIRST_GANG_UID: u32 = 256;
/// Demo gangs carry this much gold; the victor takes `VictoryGoldDiv` of it.
const GANG_GOLD: i32 = 2 * GANG_REWARD;
/// A little slower than the hero.
const GANG_SPEED: u32 = 6;
const GANG_PATROL: i32 = 8;

/// A demo unit type by its `Key=`. Panics if the built-in data lacks it.
pub fn demo_unit(content: &Content, key: &str) -> UnitId {
    content.unit_by_key(key).unwrap_or_else(|| panic!("demo unit '{key}' missing"))
}

/// A roaming gang: two bandits in front, an archer behind.
pub fn gang(content: &Content) -> Vec<Troop> {
    let (bandit, archer) = (demo_unit(content, "bandit"), demo_unit(content, "bandit_archer"));
    let t = |unit, row, col| Troop::new(unit, 1, Slot::new(row, col));
    vec![t(bandit, Row::Front, 2), t(bandit, Row::Front, 3), t(archer, Row::Back, 2)]
}

fn artifact_ids(content: &Content, ids: impl Iterator<Item = u32>) -> Vec<ItemId> {
    ids.map(ItemId).filter(|&i| content.try_item(i).is_some()).collect()
}

/// The relation between two sides as the original computes it (0x4a0868, world.md §4): `a`
/// is one side's attitude to the other's faction, `b` the other's to the first. Both ≥ 0 →
/// their mean (rounded down), `a` < 0 → `a`, else −1. Below 0 means hostile.
pub fn relation(a: i8, b: i8) -> i8 {
    if a >= 0 && b >= 0 {
        ((a as i16 + b as i16) / 2) as i8
    } else if a < 0 {
        a
    } else {
        -1
    }
}

/// The player's attitude to `faction` (1–4) from the scenario's relation matrix; an unset
/// faction counts as neutral.
pub fn player_attitude_to(relations: &[[i8; 4]; 4], faction: u8) -> i8 {
    (faction as usize).checked_sub(1).and_then(|f| relations[0].get(f).copied()).unwrap_or(0)
}

impl World {
    fn empty(title: &str, map: TileMap, start: Clock) -> World {
        World {
            title: title.to_string(),
            map,
            locations: Vec::new(),
            armies: Vec::new(),
            inactive: Vec::new(),
            gang: Vec::new(),
            start,
            relations: [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]],
            demo: false,
            dropped_units: 0,
            events: Vec::new(),
            points: Vec::new(),
            named_characters: Vec::new(),
            next_uid: FIRST_GANG_UID,
            respawns: Vec::new(),
            regions: Vec::new(),
            footprints: HashMap::new(),
            sea: Vec::new(),
        }
    }

    /// Lays the buildings over the map (world.md §1 step 6, §7): every footprint cell,
    /// bridges included, becomes road on the foot and the ship map; each location stands at
    /// its footprint's centre. Indexes the footprints and the hero's sea.
    fn place_buildings(&mut self) {
        self.footprints.clear();
        for (i, l) in self.locations.iter_mut().enumerate() {
            let c = l.centre();
            l.tile = (c.0.clamp(0, (self.map.w - 1).max(0)), c.1.clamp(0, (self.map.h - 1).max(0)));
            let cells: Vec<Tile> = l.cells().filter(|&t| self.map.in_bounds(t)).collect();
            for t in cells {
                // Where footprints overlap, the later building's wins.
                self.footprints.insert(t, i);
                self.map.pave(t);
            }
        }
        let map = &self.map;
        self.sea = (0..map.w * map.h)
            .map(|i| {
                let t = (i % map.w, i / map.w);
                is_water(map.surface(t)) && map.water_cost(t).is_some() && !self.footprints.contains_key(&t)
            })
            .collect();
    }

    /// The world of an original scenario.
    ///
    /// - Terrain and objects set the cell costs (`map` module).
    /// - Every building becomes a location of its type with its footprint (bottom-right
    ///   anchor, `size_x × size_y`), walked at road speed and entered from any of its cells.
    /// - Active land armies are placed on the map, the others (and ships) wait in `inactive`.
    ///   Hostility is the army's own attitude towards the player (< 0 attacks). Army word 17
    ///   is its starting gold, byte 80 × 10 its daily income (world.md §5).
    pub fn from_scenario(s: &Scenario, content: &Content) -> World {
        let (w, h) = (s.width() as i32, s.height() as i32);
        let objects = s
            .objects
            .iter()
            .map(|o| Decoration { tile: (o.x as i32, o.y as i32), class: o.class, sprite: o.sprite })
            .collect();
        let map = TileMap::from_codes(Grid::Square8, w, h, &s.terrain, objects);
        // The original's clock reads the start minute plus 1 (world.md §6.3, 0x4b42d8), a
        // header time of 0 included: such a map starts at minute 1 of year 0.
        let start = Clock::at_minutes(s.header.start_time as u64 + 1);
        let mut world = World::empty(&s.title, map, start);
        world.relations = s.header.relations;
        world.events = s.events.iter().map(|e| EventInfo { kind: e.kind(), title: e.display_title().trim().to_string() }).collect();
        world.points = s
            .points
            .iter()
            .map(|p| MapPoint { id: p.id, tile: (p.x as i32, p.y as i32), radius: p.radius as i32, lit: p.active != 0 && p.radius != 0 })
            .collect();
        // The building and army strings have their double spaces collapsed at load
        // (0x4b2aa1); the named characters' names are kept as they are.
        let text = crate::dt::dtm::collapse_spaces;
        world.named_characters = s.named_characters.iter().map(|n| n.name.clone()).collect();

        for (i, b) in s.buildings.iter().enumerate() {
            let kind = b.building_type().map_or(LocationKind::Smithy, LocationKind::from_building);
            let size = (b.size_x.max(1) as i32, b.size_y.max(1) as i32);
            let anchor = (b.x as i32, b.y as i32);
            let mut l = Location::new(kind, &text(&b.name), anchor);
            l.anchor = anchor;
            l.size = size;
            l.id = i as u16 + 1;
            l.owner_name = text(&b.owner_name);
            l.description = text(&b.description);
            l.picture = (b.picture_type, b.picture_variant);
            // The owner byte as it is (0x4b2504): 0 is the player, 0xFF nobody, k army k. A
            // building of the player's faction is not his for that; his start buildings are
            // given by the hero's preset ([`World::start_buildings`]).
            l.owner = match b.owner() {
                None => Owner::Neutral,
                Some(0) => Owner::Player,
                Some(k) => Owner::Army(k),
            };
            l.faction = b.faction;
            l.attitude = b.relations[0];
            l.relations = b.relations;
            l.services = b.has_barracks != 0;
            l.gold_income = b.gold_per_day as i32;
            l.gold_max = b.gold_max as i32;
            l.mana_income = b.mana_per_day as i32;
            l.mana_max = b.mana_max as i32;
            // A village starts with one day's income in both stocks; every other building
            // at 0 (0x4b55f0).
            l.tribute_gold = if kind == LocationKind::Village { l.gold_income } else { 0 };
            l.tribute_mana = if kind == LocationKind::Village { l.mana_income } else { 0 };
            // Only towns, castles, forts and ruins get a garrison record (0x4b5600 area); the
            // triples of any other type are not read.
            let garrisoned = matches!(kind, LocationKind::Town | LocationKind::Castle | LocationKind::Fort | LocationKind::Ruins);
            let triples = if garrisoned { dt_entries(&b.garrison) } else { Vec::new() };
            let (mut garrison, dropped) = place_troops(content, &[], &triples);
            world.dropped_units += dropped;
            garrison.iter_mut().for_each(|t| t.last_paid = start.total_minutes() as u64);
            l.garrison = garrison;
            l.garrison_defence = b.garrison_extra_defence as i32;
            if b.has_barracks != 0 || b.barracks.iter().any(|r| r.unit != 0) {
                l.recruits = b
                    .barracks
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.unit != 0 && content.try_unit(UnitId(r.unit as u32)).is_some())
                    .map(|(k, r)| Recruit { slot: k as u8, ..Recruit::new(UnitId(r.unit as u32), r.start_count as i32, r.max_count as i32) })
                    .collect();
            }
            l.recruit_all_types = b.recruit_all_types != 0;
            // A market has 12 places; ruins keep their first 5 goods as treasure; the map load
            // wipes the goods of every other type, altars included (0x4b5600 area).
            let goods = |n: usize| artifact_ids(content, b.artifact_slots[..n].iter().filter(|&&x| x != 0).map(|&x| u32::from(x)));
            let market = matches!(kind, LocationKind::Town | LocationKind::Market | LocationKind::Church);
            if kind == LocationKind::Ruins {
                // The garrison's gold, and its items: with units, each of the first 5 goods
                // goes to the unit it helps most, else into the garrison's pack (0x4a273c);
                // with none, all into the pack (0x4b554e). The building's goods words stay as
                // the map has them.
                l.treasure_gold = b.price_max as i32;
                l.map_goods = b.artifact_slots[..MARKET_PLACES].iter().filter(|&&x| x != 0).map(|&x| (x as i16).unsigned_abs() as i32).collect();
                if l.garrison.is_empty() {
                    l.treasure = goods(5);
                } else {
                    // Given after the garrison's recount: its strengths stay without them.
                    l.strengths_bare = !goods(5).is_empty();
                    for item in goods(5) {
                        super::ai::give_item_to(content, &mut l.garrison, &mut l.treasure, item);
                    }
                }
            } else if market && (!goods(MARKET_PLACES).is_empty() || b.random_artifacts_for_sale > 0) {
                let places = b.artifact_slots[..MARKET_PLACES].iter().map(|&x| Some(ItemId(u32::from(x))).filter(|&i| x != 0 && content.try_item(i).is_some())).collect();
                l.shop = Some(Shop::from_map(places, b.random_artifacts_for_sale as usize, (b.price_min as i32, b.price_max as i32), content.dearest_item()));
            }
            l.spells = b.spells_for_sale.iter().copied().filter(|&x| x != 0).collect();
            l.events = b.events().collect();
            l.linked = (b.linked_building as usize).checked_sub(1).filter(|&j| j < s.buildings.len());
            world.locations.push(l);
        }
        world.place_buildings();

        for a in &s.armies {
            let mut entries = Vec::new();
            if a.leader_unit != 0 {
                entries.push((a.leader_unit as u32, a.leader_level as i32 + 1, 1));
            }
            entries.extend(dt_entries(&a.troops));
            let (mut troops, dropped) = place_troops(content, &[], &entries);
            world.dropped_units += dropped;
            // An army record without units is kept as it is (0x4b2504 fills every slot): an
            // event may give it units and activate it (Проклятое озеро's army 44).
            if a.leader_unit != 0 {
                // The leader draws no wage (economy.md §1, kind 0).
                if let Some(leader) = troops.first_mut() {
                    leader.kind = WageKind::Leader;
                }
            }
            // Everyone counts as paid at the start.
            let paid = start.total_minutes() as u64;
            troops.iter_mut().for_each(|t| t.last_paid = paid);
            let at = (a.x as i32, a.y as i32);
            // An army placed on water (terrain codes 0–2), not on a bridge, is a ship army
            // for good, inside any other building's footprint too; byte 72 only picks its
            // picture (ai.md §13, 0x4b4a90).
            let on_bridge = world.location_covering(at).is_some_and(|l| world.locations[l].kind.is_bridge());
            let afloat = world.map.in_bounds(at) && is_water(world.map.surface(at)) && !on_bridge;
            let ship = if afloat { a.ship.max(super::ships::kind::HERO) } else { 0 };
            let figure = Army::figure_for(content, a);
            let figure = if afloat { Army::ship_figure(a.ship, figure) } else { figure };
            // Exactly the file's cell (0x4b2504): no search for a free or passable one.
            let tile = at;
            // Merchant ships trade and never attack (guess; one shipped merchant is marked
            // ill-disposed in its file).
            // The original's relation (0x4a0868): the player's attitude to the army's faction
            // and the army's to the player. An army whose own attitudes were left at 0 (РК1's
            // mage) is hostile through its faction.
            let towards = player_attitude_to(&world.relations, a.faction);
            let attitude = relation(towards, a.relations[0]);
            let attitude = if ship == super::ships::kind::MERCHANT { attitude.max(0) } else { attitude };
            let home = (a.home_building as usize).checked_sub(1).filter(|&j| j < world.locations.len());
            let army = Army {
                id: a.id,
                uid: a.id as u32,
                name: text(&a.name),
                leader_name: text(&a.leader_name),
                description: text(&a.description),
                model: a.model,
                figure,
                pos: world.map.center(tile),
                home,
                post: tile,
                box_centre: None,
                patrols: a.patrols != 0,
                patrol_radius: a.patrol_radius as i32,
                troops,
                faction: a.faction,
                attitude,
                // Word 17, signed (ai.md §1).
                gold: a.gold_income as i16 as i32,
                items: Vec::new(),
                speed: Army::speed_for(a.speed_correction, a.leader_unit as u32),
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
                ship,
                ai: AiProfile::from_dt(a),
                mind: AiMind::default(),
            };
            let mut army = army;
            if let Some(leader) = army.troops.first_mut().filter(|_| a.leader_unit != 0) {
                leader.named = a.named_character;
            }
            // Its items go to the unit each one helps most, else into its pack (0x4a273c).
            for item in artifact_ids(content, a.artifacts.iter().filter(|&&x| x != 0).map(|&x| x as u32)) {
                super::ai::give_item(content, &mut army, item);
            }
            // Then its first unit's worn items become his own (0x4b2504 sets unit 1's +0x18 to
            // the number of his filled worn slots): Йошка's pitchfork stays on while he lives.
            if let Some(first) = army.troops.first_mut() {
                first.personal = first.worn.iter().filter(|w| w.is_some()).count() as u8;
            }
            // Byte 84: every unit holds that spell in its first slot for good (0x4b2504).
            if a.spell != 0 {
                for t in &mut army.troops {
                    t.spells[0] = Some(crate::rules::units::SpellSlot { spell: u32::from(a.spell), until: MAP_ARMY_SPELL_END });
                }
            }
            if a.is_active() {
                world.armies.push(army);
            } else {
                world.inactive.push(army);
            }
        }
        world.regions = world.map.regions().0;
        world
    }

    /// The buildings given to the hero of `class` at the start (world.md §7): the preset's
    /// start building (byte 16, 1-based) and every building flagged for the class (building
    /// byte 353 + class), bridges too (0x4b2504 does not exclude them).
    pub fn start_buildings(&self, s: &Scenario, class: HeroClass) -> Vec<usize> {
        let k = match class {
            HeroClass::Knight => 0,
            HeroClass::Archmage => 1,
            HeroClass::Ranger => 2,
        };
        let archetype = [Archetype::Knight, Archetype::Archmage, Archetype::Ranger][k];
        let p = s.header.hero(archetype);
        let ok = |i: usize| i < self.locations.len();
        let preset = (p.start_building as usize).checked_sub(1);
        let flagged = s.buildings.iter().enumerate().filter(|(_, b)| b.start_for[k] != 0).map(|(i, _)| i);
        let mut out: Vec<usize> = preset.into_iter().chain(flagged).filter(|&i| ok(i)).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Where and with what the hero of `class` starts in scenario `s` (its header preset):
    /// exactly on the preset's cell (world.md §7, no relocation); his start buildings
    /// ([`World::start_buildings`]) become his without moving him. A class the map does not
    /// offer starts from the map's first offered preset ([`start_preset`]).
    pub fn hero_start(&self, s: &Scenario, content: &Content, class: HeroClass) -> HeroStart {
        let from = start_preset(s, class);
        let archetype = match from {
            HeroClass::Knight => Archetype::Knight,
            HeroClass::Archmage => Archetype::Archmage,
            HeroClass::Ranger => Archetype::Ranger,
        };
        let p = s.header.hero(archetype);
        let tile = (p.x as i32, p.y as i32);
        let hero_slot = content.formation.new_unit_slot(&[]).expect("empty formation");
        let (troops, _) = place_troops(content, &[hero_slot], &dt_entries(&p.troops));
        HeroStart {
            class,
            tile,
            gold: p.gold as u16 as i16 as i32,
            mana: p.mana as u16 as i16 as i32,
            hero_slot,
            troops,
            items: artifact_ids(content, p.artifacts.iter().filter(|&&x| x != 0).map(|&x| x as u32)),
            spells: p.spells.iter().copied().filter(|&x| x != 0).collect(),
            location: self.location_at(tile),
            owned: self.start_buildings(s, from),
        }
    }

    /// Building `l` becomes the player's (a start building, a capture): owner, his faction
    /// and his four attitudes (the hero's row of the relation matrix, economy.md §3,
    /// saves-data.md §10.4), his attitude to his own faction included, 3 or not.
    pub fn give_to_player(&mut self, l: usize) {
        let mine = self.relations[0];
        let loc = &mut self.locations[l];
        loc.owner = Owner::Player;
        loc.take_sides(1, mine);
    }

    /// The demo kingdom of `data/kingdom.txt`, populated with the built-in demo units.
    pub fn standard(content: &Content) -> Self {
        let u = |key| demo_unit(content, key);
        let (spearman, archer, swordsman, healer) = (u("spearman"), u("archer"), u("swordsman"), u("healer"));
        let (bandit, bandit_archer, bandit_chief) = (u("bandit"), u("bandit_archer"), u("bandit_chief"));
        let map = TileMap::parse(KINGDOM);
        let tile = |c: char| {
            map.markers.iter().find(|(m, _)| *m == c).map(|&(_, t)| t).unwrap_or_else(|| panic!("map has no '{c}'"))
        };
        let t = |unit, row, col| Troop::new(unit, 1, Slot::new(row, col));
        let (f, b) = (Row::Front, Row::Back);
        let recruits = |units: Vec<UnitId>| units.into_iter().enumerate().map(|(k, unit)| Recruit { unit, stock: None, max: 0, progress: 0, slot: k as u8 }).collect();
        let dearest = content.dearest_item();
        let shop = || Some(Shop::from_map(Vec::new(), 6, (0, 0), dearest));

        let mut oakford = Location::new(LocationKind::Castle, tr("Oakford"), tile('C'));
        oakford.picture = (3, 0);
        oakford.owner = Owner::Player;
        oakford.faction = 1;
        oakford.attitude = 3;
        oakford.gold_income = 20;
        // A castle pays its stock, which grows only up to a maximum (economy.md §3).
        oakford.gold_max = 20;
        oakford.recruits = recruits(vec![spearman, archer, healer]);
        oakford.shop = shop();
        let mut greywall = Location::new(LocationKind::Castle, tr("Greywall"), tile('G'));
        greywall.picture = (3, 2);
        greywall.recruits = recruits(vec![swordsman, archer, healer]);
        greywall.shop = shop();
        greywall.spells = vec![3, 5];
        let village = |name, c| {
            let mut v = Location::new(LocationKind::Village, name, tile(c));
            v.gold_income = 10;
            v.gold_max = 10;
            v.tribute_gold = 10;
            // The peasants pray for the hero: mana for the demo's spells.
            v.mana_income = 10;
            v.mana_max = 10;
            v.tribute_mana = 10;
            v.picture = (2, 6);
            v
        };
        let camp = |name, c, garrison, reward, loot| {
            let mut l = Location::new(LocationKind::Camp, name, tile(c));
            l.faction = 4;
            l.attitude = -3;
            l.garrison = garrison;
            l.treasure_gold = reward;
            l.loot_rolls = loot;
            l.picture = (12, 5);
            l
        };
        let locations = vec![
            oakford,
            village(tr("Millbrook"), 'M'),
            village(tr("Ashford"), 'A'),
            village(tr("Saltmarsh"), 'S'),
            Location { picture: (7, 4), spells: vec![1, 2, 4], ..Location::new(LocationKind::Church, tr("St. Beor's church"), tile('+')) },
            greywall,
            camp(
                tr("Bandit camp"),
                'B',
                vec![t(bandit, f, 1), t(bandit, f, 2), t(bandit, f, 3), t(bandit_archer, b, 2), t(bandit_archer, b, 3)],
                100,
                1,
            ),
            camp(
                tr("Bandit lair"),
                'L',
                vec![t(bandit_chief, f, 2), t(bandit, f, 1), t(bandit, f, 3), t(bandit_archer, b, 1), t(bandit_archer, b, 3)],
                150,
                2,
            ),
        ];
        let mut w = World::empty(tr("Demo kingdom"), map, Clock::demo_start());
        w.locations = locations;
        w.demo = true;
        w.gang = gang(content);
        w.place_buildings();
        // Two gangs already on the roads, one from each camp.
        let camp = w.index_of(tr("Bandit camp"));
        let lair = w.index_of(tr("Bandit lair"));
        w.spawn_gang(camp, (39, 16));
        w.spawn_gang(lair, (14, 24));
        w.regions = w.map.regions().0;
        w
    }

    /// Puts back what a save leaves out (the `#[serde(skip)]` fields: the map, the texts and
    /// the indexes) from `fresh`, the same world rebuilt from its scenario or the demo. Fails
    /// when the saved world does not fit it (another map).
    pub fn restore_statics(&mut self, fresh: World) -> Result<(), String> {
        if self.locations.len() != fresh.locations.len() {
            return Err(format!("{} buildings saved, the map has {}", self.locations.len(), fresh.locations.len()));
        }
        for (l, f) in self.locations.iter_mut().zip(&fresh.locations) {
            if l.id != f.id || l.kind != f.kind || l.anchor != f.anchor {
                return Err(format!("building {} does not match the map", l.id));
            }
            l.name.clone_from(&f.name);
            l.owner_name.clone_from(&f.owner_name);
            l.description.clone_from(&f.description);
            l.services = f.services;
            l.map_goods.clone_from(&f.map_goods);
            for (r, fr) in l.recruits.iter_mut().zip(&f.recruits) {
                r.slot = fr.slot;
            }
            if l.relations == UNKNOWN_RELATIONS {
                l.relations = [l.attitude, f.relations[1], f.relations[2], f.relations[3]];
            }
        }
        let texts: HashMap<u8, &Army> = fresh.armies.iter().chain(fresh.inactive.iter()).filter(|a| a.id != 0).map(|a| (a.id, a)).collect();
        let respawning = self.respawns.iter_mut().map(|r| &mut r.army);
        for a in self.armies.iter_mut().chain(self.inactive.iter_mut()).chain(respawning) {
            match texts.get(&a.id) {
                _ if a.id == 0 => {
                    a.name = tr(GANG_NAME).to_string();
                    if a.figure == NO_FIGURE {
                        a.figure = 4;
                    }
                    if a.speed == 0 {
                        a.speed = GANG_SPEED;
                    }
                }
                Some(f) => {
                    if a.speed == 0 {
                        a.speed = f.speed;
                    }
                    if a.figure == NO_FIGURE {
                        a.figure = f.figure;
                    }
                    a.name.clone_from(&f.name);
                    a.leader_name.clone_from(&f.leader_name);
                    a.description.clone_from(&f.description);
                    // Saves from before the AI kept no profile: the scenario's.
                    if !a.ai.enabled {
                        a.ai = f.ai.clone();
                    }
                    if a.ai.garrison_level < 0 {
                        a.ai.garrison_level = f.ai.garrison_level;
                    }
                }
                None => return Err(format!("army {} is not on the map", a.id)),
            }
        }
        self.title = fresh.title;
        self.map = fresh.map;
        self.events = fresh.events;
        self.points = fresh.points;
        self.named_characters = fresh.named_characters;
        self.footprints = fresh.footprints;
        self.sea = fresh.sea;
        self.regions = fresh.regions;
        Ok(())
    }

    pub fn index_of(&self, name: &str) -> usize {
        self.locations.iter().position(|l| l.name == name).unwrap_or_else(|| panic!("no location {name}"))
    }

    /// Quests and rumours offered in the main hall of location `l`: its event slots that
    /// hold quest or rumour events. The event engine (`rules::events`) decides which of them
    /// are available; this is only the building's list.
    pub fn local_events(&self, l: usize) -> Vec<EventId> {
        self.locations[l]
            .events
            .iter()
            .copied()
            .filter(|&id| {
                let e = (id as usize).checked_sub(1).and_then(|i| self.events.get(i));
                e.is_some_and(|e| matches!(e.kind, Some(EventKind::Quest | EventKind::Rumour)))
            })
            .collect()
    }

    /// An event's title, if the scenario has it.
    pub fn event_title(&self, id: EventId) -> Option<&str> {
        (id as usize).checked_sub(1).and_then(|i| self.events.get(i)).map(|e| e.title.as_str()).filter(|t| !t.is_empty())
    }

    /// The building (not a bridge) whose footprint covers `t`: standing there is being in it.
    pub fn location_at(&self, t: Tile) -> Option<usize> {
        self.location_covering(t).filter(|&l| !self.locations[l].kind.is_bridge())
    }

    /// Location whose footprint covers `t` (bridges included).
    pub fn location_covering(&self, t: Tile) -> Option<usize> {
        self.footprints.get(&t).copied()
    }

    /// A demo gang from camp `home` at `at`.
    pub fn spawn_gang(&mut self, home: usize, at: Tile) {
        self.next_uid = self.next_uid.max(FIRST_GANG_UID) + 1;
        self.armies.push(Army {
            id: 0,
            uid: self.next_uid,
            name: tr(GANG_NAME).to_string(),
            leader_name: String::new(),
            description: String::new(),
            model: 5,
            // The Rogue figure (0x4ed238).
            figure: 4,
            pos: self.map.center(at),
            home: Some(home),
            post: self.locations[home].tile,
            box_centre: None,
            patrols: true,
            patrol_radius: GANG_PATROL,
            troops: self.gang.clone(),
            faction: 4,
            attitude: -3,
            gold: GANG_GOLD,
            items: Vec::new(),
            speed: GANG_SPEED,
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
            ship: 0,
            // A gang pays the player's XP in full (a map's army gives its byte 71).
            ai: AiProfile { exp_correction: 100, ..AiProfile::default() },
            mind: AiMind::default(),
        });
    }

    /// Both cells lie in the same region on foot (true when regions are not known).
    pub fn same_region(&self, a: Tile, b: Tile) -> bool {
        match (self.map.mask_index(a), self.map.mask_index(b)) {
            (Some(i), Some(j)) if !self.regions.is_empty() => self.regions[i] == self.regions[j],
            (Some(_), Some(_)) => true,
            _ => false,
        }
    }

    pub fn camps(&self) -> impl Iterator<Item = (usize, &Location)> {
        self.locations.iter().enumerate().filter(|(_, l)| l.kind == LocationKind::Camp)
    }

    /// The demo is won when every camp is cleared; worlds without camps are not won this way.
    pub fn all_camps_cleared(&self) -> bool {
        self.camps().next().is_some() && self.camps().all(|(_, l)| l.cleared)
    }

    /// Location closest to `t` on foot among those `pick` accepts, with the path to it.
    pub fn nearest_location(&self, t: Tile, pick: impl Fn(&Location) -> bool) -> Option<(usize, Vec<Tile>)> {
        let mut candidates: Vec<usize> = (0..self.locations.len()).filter(|&i| pick(&self.locations[i])).collect();
        candidates.sort_by_key(|&i| self.map.distance(t, self.locations[i].tile));
        candidates
            .into_iter()
            .take(8)
            .filter_map(|i| {
                let p = self.map.path(t, self.locations[i].tile);
                (!p.is_empty() || self.locations[i].tile == t).then_some((i, p))
            })
            .min_by_key(|(_, p)| self.map.path_minutes(t, p))
    }
}

#[cfg(test)]
pub(crate) mod testkit {
    //! Small synthetic scenarios, built in code.
    use super::*;
    use crate::dt::dtm::{Army as DtArmy, Building, HeroPreset, MapObject, Surface, Troop as DtTroop};

    /// Units 1–3 are the hero classes, 4 a warrior, 5 a shooter; items 7 (ring) and 9 (potion).
    pub fn content() -> Content {
        use crate::rules::content::testkit as ck;
        use crate::rules::content::{ArtefactType, MagicDirection, MagicSchool};
        let units = vec![
            ck::warrior(1, 20, 5),
            ck::mage(2, 10, MagicSchool::Elemental, MagicDirection::ToEnemy),
            ck::shooter(3, 15),
            ck::warrior(4, 10, 2),
            ck::shooter(5, 8),
        ];
        ck::content(units, vec![ck::item(7, ArtefactType::Ring), ck::item(9, ArtefactType::Potion)])
    }

    pub fn troop(unit: u8, level: u8, count: u8) -> DtTroop {
        DtTroop { unit, level, count }
    }

    /// A `w × h` grass scenario starting 1204-05-19 09:00.
    pub fn scenario(w: u32, h: u32) -> Scenario {
        let mut s = Scenario::default();
        s.header.width = w;
        s.header.height = h;
        s.header.start_time = 624_354_300;
        s.header.relations = [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]];
        s.terrain = vec![Surface::GrassPlain as u8; (w * h) as usize];
        s.title = "Test".into();
        s
    }

    pub fn set(s: &mut Scenario, x: u32, y: u32, surface: Surface) {
        let w = s.width();
        s.terrain[(y * w + x) as usize] = surface as u8;
    }

    pub fn object(s: &mut Scenario, x: u16, y: u16, class: u8, sprite: u8) {
        s.objects.push(MapObject { x, y, sprite, class });
    }

    pub fn building(kind: BuildingType, x: u16, y: u16, size: (u8, u8)) -> Building {
        Building { kind: kind as u8, picture_type: kind as u8, x, y, size_x: size.0, size_y: size.1, name: format!("{kind:?}"), ..Building::default() }
    }

    pub fn army(id: u8, x: u16, y: u16, attitude: i8, troops: &[DtTroop]) -> DtArmy {
        let mut t = [DtTroop::default(); 6];
        t[..troops.len()].copy_from_slice(troops);
        DtArmy { id, x, y, model: 4, faction: if attitude < 0 { 4 } else { 3 }, relations: [attitude, 0, 0, 0], troops: t, name: format!("Army {id}"), ..DtArmy::default() }
    }

    pub fn hero(x: u16, y: u16, gold: u32, troops: &[DtTroop]) -> HeroPreset {
        let mut t = [DtTroop::default(); 6];
        t[..troops.len()].copy_from_slice(troops);
        HeroPreset { x, y, gold, troops: t, ..HeroPreset::default() }
    }
}

/// The class whose header preset starts the hero of `class` on scenario `s`: his own when the
/// map offers it ([`crate::dt::dtm::HeroPreset::offered`]: a start cell), else the map's first
/// offered class (knight, archmage, ranger). Razdor fixes the original's bug: a campaign's
/// next map keeps the class it was started with without looking at whether that map offers
/// it (0x4b5b64), so a hero of a class the map leaves out started on cell (0, 0) of the
/// empty preset. Here he keeps his class and record, and starts where, and with what, the
/// map's first offered hero would. A map that offers no class keeps his own preset.
pub fn start_preset(s: &Scenario, class: HeroClass) -> HeroClass {
    let k = HeroClass::ALL.iter().position(|&c| c == class).unwrap_or(0);
    let offered = s.header.offered_classes();
    match s.header.first_offered_class() {
        Some(first) if !offered[k] => HeroClass::ALL[first],
        _ => class,
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;
    use crate::dt::dtm::Surface;

    #[test]
    fn an_army_s_name_is_shown_up_to_its_hash() {
        assert_eq!(shown_army_name("Призрак#2"), "Призрак");
        assert_eq!(shown_army_name("Призрак #2"), "Призрак");
        assert_eq!(shown_army_name("Банда Хромого"), "Банда Хромого");
        assert_eq!(shown_army_name("#3"), "");
    }
    use crate::rules::content::Content;
    use crate::rules::game::Game;
    use crate::rules::map::object_class;


    #[test]
    fn an_older_saves_market_loads_into_its_places() {
        let old = r#"{"fixed":[24],"random":3,"price":[50,400],"stock":[20,24]}"#;
        let shop: Shop = serde_json::from_str(old).unwrap();
        assert_eq!(shop.places.len(), MARKET_PLACES);
        assert_eq!(shop.places[..2], [Some(Good { item: ItemId(20), fixed: false }), Some(Good { item: ItemId(24), fixed: true })]);
        assert_eq!((shop.goods(), shop.timer), (vec![ItemId(20), ItemId(24)], 1));
        let again: Shop = serde_json::from_str(&serde_json::to_string(&shop).unwrap()).unwrap();
        assert_eq!(again, shop);
    }

    #[test]
    fn an_armys_hostility_follows_the_originals_relation() {
        // 0x4a0868 (world.md §4): a = the player's attitude to the army's faction (header
        // matrix), b = the army's to the player; both ≥ 0 → (a + b) / 2, a < 0 → a, else −1.
        assert_eq!([relation(-2, 0), relation(2, 0), relation(1, -1), relation(3, 1)], [-2, 1, -1, 2]);
        let mut s = scenario(20, 6);
        // РК1's "Маг Эктор": faction 4 (enemy), its own attitudes left at 0.
        let mut mage = army(1, 5, 2, 0, &[troop(4, 0, 1)]);
        mage.faction = 4;
        mage.relations = [0, 0, 0, 0];
        let mut friend = army(2, 12, 2, 0, &[troop(4, 0, 1)]);
        friend.faction = 2;
        friend.relations = [0, 0, 0, 0];
        s.armies = vec![mage, friend];
        let w = World::from_scenario(&s, &content());
        assert!(w.armies[0].hostile(), "the player's −2 towards the enemy faction makes it hostile");
        assert!(!w.armies[1].hostile(), "an ally with no attitudes of its own stays friendly");
    }

    #[test]
    fn standard_world_places_every_location_on_a_passable_tile() {
        let w = World::standard(&Content::builtin());
        assert_eq!(w.locations.len(), 8);
        for l in &w.locations {
            assert!(w.map.passable(l.tile), "{} is on impassable ground", l.name);
        }
        assert_eq!(w.location_at(w.locations[0].tile), Some(0));
        assert!(w.locations[0].owned() && !w.locations[0].hostile());
        assert!(w.camps().all(|(_, l)| l.defended()));
    }

    #[test]
    fn every_location_is_reachable_from_home() {
        let w = World::standard(&Content::builtin());
        let home = w.locations[0].tile;
        for l in &w.locations[1..] {
            assert!(!w.map.path(home, l.tile).is_empty(), "{} unreachable", l.name);
        }
    }

    #[test]
    fn two_gangs_start_on_the_map() {
        let w = World::standard(&Content::builtin());
        assert_eq!(w.armies.len(), 2);
        assert!(w.armies.iter().all(|p| w.map.passable(p.tile(&w.map)) && p.hostile()));
    }

    #[test]
    fn scenario_terrain_and_objects_set_passability() {
        let mut s = scenario(8, 6);
        set(&mut s, 1, 0, Surface::DeepSea);
        set(&mut s, 2, 0, Surface::ImpassableSwamp);
        set(&mut s, 3, 0, Surface::ImpassableSnowdrifts);
        set(&mut s, 4, 0, Surface::Road);
        set(&mut s, 5, 0, Surface::Marsh);
        object(&mut s, 0, 2, object_class::MOUNTAINS, 11);
        object(&mut s, 1, 2, object_class::THICKET, 3);
        object(&mut s, 2, 2, object_class::HILLS, 12);
        object(&mut s, 3, 2, object_class::TREES, 3);
        object(&mut s, 4, 2, object_class::DEAD_TREES, 110);
        object(&mut s, 5, 2, object_class::ROCKS, 12);
        let w = World::from_scenario(&s, &content());
        let m = &w.map;
        assert_eq!((m.w, m.h), (8, 6));
        assert!(m.passable((0, 0)));
        assert!(!m.passable((1, 0)) && !m.passable((2, 0)) && !m.passable((3, 0)));
        assert!(m.minutes((4, 0)) < m.minutes((0, 0)), "road is fastest");
        assert!(m.minutes((5, 0)) > m.minutes((0, 0)), "marsh is slow");
        assert!(!m.passable((0, 2)) && !m.passable((1, 2)) && !m.passable((5, 2)));
        for x in 2..5 {
            assert!(m.minutes((x, 2)) > m.minutes((0, 0)), "hills and trees slow ({x})");
        }
        assert_eq!(w.start.label(), "1204, month 5, day 19, 9 h");
    }

    #[test]
    fn building_footprints_are_walked_at_road_speed() {
        let mut s = scenario(10, 10);
        for x in 0..10 {
            set(&mut s, x, 7, Surface::DeepSea);
        }
        let mut castle = building(BuildingType::Castle, 5, 4, (4, 3));
        castle.gold_per_day = 55;
        castle.faction = 4;
        castle.relations = [-2, 0, 0, 0];
        castle.owner_army = 2;
        castle.garrison[0] = troop(4, 0, 2);
        castle.garrison[1] = troop(5, 1, 1);
        castle.garrison_extra_defence = 11;
        castle.barracks[0] = crate::dt::dtm::RecruitSlot { unit: 4, start_count: 3, max_count: 9 };
        castle.has_barracks = 1;
        castle.artifact_slots[0] = 7;
        castle.random_artifacts_for_sale = 2;
        let mut ruins = building(BuildingType::Ruins, 8, 2, (1, 1));
        ruins.artifact_slots[0] = 9;
        ruins.artifact_slots[5] = 10; // only the first 5 goods are treasure
        ruins.price_max = 300;
        ruins.relations = [-3, 0, 0, 0];
        ruins.garrison[0] = troop(4, 0, 1);
        let bridge = building(BuildingType::StoneBridge, 3, 7, (1, 1));
        let mut village = building(BuildingType::Village, 1, 9, (2, 2));
        village.gold_per_day = 20;
        village.gold_max = 50;
        village.linked_building = 1;
        s.buildings = vec![castle, ruins, bridge, village];
        let w = World::from_scenario(&s, &content());

        let c = &w.locations[0];
        assert_eq!((c.kind, c.id, c.anchor, c.size), (LocationKind::Castle, 1, (5, 4), (4, 3)));
        let cells: Vec<Tile> = c.cells().collect();
        // 4 × 3 up and left of the anchor, plus a row above: wider than tall.
        assert_eq!(cells.len(), 16);
        assert!(cells.contains(&(2, 2)) && cells.contains(&(5, 4)) && cells.contains(&(2, 1)) && !cells.contains(&(1, 4)) && !cells.contains(&(2, 0)));
        assert_eq!(c.tile, (4, 3), "the footprint's centre (x0 + sx/2, y0 + sy/2)");
        assert!(cells.iter().all(|&t| w.map.cost(t) == Some(crate::rules::map::ROAD)), "every cell is road");
        assert!(cells.iter().all(|&t| w.location_at(t) == Some(0)), "any cell is the building");
        assert_eq!(w.location_at((1, 3)), None);
        assert_eq!((c.owner, c.faction, c.gold_income, c.garrison_defence), (Owner::Army(2), 4, 55, 11));
        assert!(c.hostile() && c.defended() && c.bars_hero());
        assert_eq!(c.garrison.iter().map(|t| (t.unit.0, t.level)).collect::<Vec<_>>(), [(4, 1), (4, 1), (5, 2)]);
        assert_eq!(c.garrison[2].slot.row, Row::Back, "the shooter stands behind");
        assert_eq!(c.recruits, vec![Recruit { unit: UnitId(4), stock: Some(3), max: 9, progress: 0, slot: 0 }]);
        assert!(c.shop.is_none(), "a castle's goods are wiped at load: only towns, markets and churches sell");

        let r = &w.locations[1];
        assert_eq!((r.kind, r.tile, r.treasure.clone(), r.treasure_gold), (LocationKind::Ruins, (8, 2), vec![ItemId(9)], 300));
        assert!(r.shop.is_none() && r.defended() && r.bars_hero());

        let b = &w.locations[2];
        assert!(b.kind.is_bridge() && !b.bars_hero());
        assert!(w.map.passable((3, 7)) && !w.map.passable((2, 7)), "the bridge crosses the water");
        assert_eq!(w.location_at((3, 7)), None, "bridges are not entered");
        assert_eq!(w.location_covering((3, 7)), Some(2));

        let v = &w.locations[3];
        assert_eq!((v.kind, v.tile, v.linked, v.tribute_gold), (LocationKind::Village, (1, 9), Some(0), 20));
        assert!(!v.hostile() && !v.defended() && !v.bars_hero(), "villages are walked through");
        // Every building type maps to its kind.
        for t in BuildingType::ALL {
            assert_eq!(format!("{t:?}").replace("DungeonEntrance", "Entrance"), format!("{:?}", LocationKind::from_building(t)));
        }
    }

    #[test]
    fn ill_disposed_castles_forts_and_unowned_ruins_bar_the_way() {
        // World.md §1.3 (0x4cc148): castles and forts by their attitude alone (a building he
        // takes gets attitude 3), ruins unless his; nothing else.
        let mut l = Location::new(LocationKind::Fort, "Fort", (0, 0));
        for (attitude, bars) in [(-2, true), (0, true), (1, false)] {
            l.attitude = attitude;
            assert_eq!(l.bars_hero(), bars, "attitude {attitude}");
        }
        let mut r = Location::new(LocationKind::Ruins, "Ruins", (0, 0));
        r.attitude = 3;
        assert!(r.bars_hero(), "ruins not his, whatever their attitude");
        r.owner = Owner::Player;
        assert!(!r.bars_hero());
        for k in [LocationKind::Town, LocationKind::Village, LocationKind::Church, LocationKind::Tavern, LocationKind::Palace, LocationKind::StoneBridge] {
            let mut t = Location::new(k, "", (0, 0));
            t.attitude = -3;
            assert!(!t.bars_hero(), "an enemy {k:?} is walked through");
        }
    }

    #[test]
    fn a_fort_at_the_foot_of_a_bridge_is_walked_through() {
        // Water on rows 4 and 5, a bridge across at x = 5; the fort (4..=5, 6..=7) closes
        // the gap between impassable bogs on the south bank.
        let mut s = scenario(10, 10);
        for x in 0..10 {
            set(&mut s, x, 4, Surface::DeepSea);
            set(&mut s, x, 5, Surface::DeepSea);
        }
        for x in [0, 1, 2, 3, 6, 7, 8, 9] {
            set(&mut s, x, 6, Surface::ImpassableSwamp);
            set(&mut s, x, 7, Surface::ImpassableSwamp);
        }
        let fort = building(BuildingType::Fort, 5, 7, (2, 2));
        let bridges = [building(BuildingType::StoneBridge, 5, 4, (1, 1)), building(BuildingType::StoneBridge, 5, 5, (1, 1))];
        s.buildings = vec![fort, building(BuildingType::Village, 2, 9, (1, 1)), bridges[0].clone(), bridges[1].clone()];
        let w = World::from_scenario(&s, &content());
        let path = w.map.path((2, 9), (2, 1));
        assert!(!path.is_empty(), "north over the bridge, through the fort's footprint");
        assert!(path.iter().any(|&t| w.location_at(t) == Some(0)), "{path:?}");
    }

    #[test]
    fn main_halls_list_their_quests_and_rumours() {
        use crate::dt::dtm::Event as DtEvent;
        let mut s = scenario(6, 6);
        let ev = |kind, title: &str| DtEvent { kind, title: title.into(), ..DtEvent::default() };
        s.events = vec![ev(3, "A quest%+flag"), ev(2, "Local"), ev(4, "Rumour"), ev(1, "Global")];
        let mut t = building(BuildingType::Town, 2, 2, (1, 1));
        t.event_slots[..4].copy_from_slice(&[1, 2, 3, 9]);
        t.event_count = 4;
        s.buildings = vec![t];
        let w = World::from_scenario(&s, &content());
        assert_eq!(w.local_events(0), vec![1, 3], "quests and rumours; local events fire by themselves");
        assert_eq!(w.event_title(1), Some("A quest"), "without the flag script");
        assert_eq!(w.event_title(9), None);
    }

    #[test]
    fn an_armys_figure_is_its_style_or_its_leaders_never_byte_5() {
        use crate::rules::content::{testkit as ck, UnitDef};
        // GlobalIndex = type + 1: 2 the Archmage (bit 1 of 0x4b5afc), 43 and 47 zombies, 46
        // a death knight, 51 a shade, 64 a rogue, 65 a cursed undead of no group.
        let undead = |id| UnitDef { nature: Nature::Undead, ..ck::warrior(id, 5, 5) };
        let rogue = UnitDef { nature: Nature::Rogue, ..ck::warrior(64, 5, 5) };
        let units = vec![ck::warrior(1, 5, 5), ck::warrior(2, 5, 5), undead(43), undead(46), undead(47), undead(51), rogue, undead(65)];
        let c = ck::content(units, Vec::new());
        let figure = |behaviour, leader_unit, model| Army::figure_for(&c, &dtm::Army { behaviour, leader_unit, model, ..dtm::Army::default() });
        // Byte 5 is never read: model 7 ("inactive") and 4 alike.
        assert_eq!([figure(0, 1, 7), figure(1, 64, 7), figure(2, 64, 4), figure(9, 1, 4)], [6, 4, 5, 5], "Knight, Rogue, Peasant, else 5");
        assert_eq!(figure(1, 0, 5), 4, "no leader: type 0, an ordinary unit");
        assert_eq!(figure(0, 2, 4), 10, "a mage leader whatever the style");
        assert_eq!([figure(1, 43, 7), figure(2, 47, 6), figure(1, 46, 7), figure(1, 51, 5), figure(0, 65, 4)], [8, 8, 7, 9, 7]);
        assert_eq!([Army::ship_figure(0, 6), Army::ship_figure(1, 6), Army::ship_figure(2, 6), Army::ship_figure(3, 6), Army::ship_figure(4, 6)], [3, 3, 12, 11, 6]);
    }

    #[test]
    fn a_save_without_figures_takes_the_scenarios() {
        let mut s = scenario(8, 8);
        let mut rogue = army(1, 2, 2, -2, &[troop(4, 0, 1)]);
        (rogue.behaviour, rogue.model) = (1, 7);
        s.armies = vec![rogue];
        let mut w = World::from_scenario(&s, &content());
        assert_eq!(w.armies[0].figure, 4, "the Rogue figure, not byte 5's 7");
        w.armies[0].figure = NO_FIGURE;
        w.restore_statics(World::from_scenario(&s, &content())).unwrap();
        assert_eq!(w.armies[0].figure, 4);
    }

    #[test]
    fn only_active_armies_start_on_the_map_and_hostility_follows_attitude() {
        let mut s = scenario(12, 12);
        let mut foe = army(1, 2, 2, -2, &[troop(4, 0, 3), troop(5, 2, 2)]);
        foe.leader_unit = 1;
        foe.patrols = 1;
        foe.patrol_radius = 6;
        foe.gold_income = 80;
        foe.unknown_80 = 3;
        foe.artifacts = [7, 0, 0];
        let friend = army(2, 8, 8, 1, &[troop(4, 0, 1)]);
        let mut sleeper = army(3, 5, 5, -2, &[troop(4, 0, 1)]);
        sleeper.model = 7;
        sleeper.inactive = 1;
        let mut unknown = army(4, 6, 6, -1, &[troop(99, 0, 2)]);
        unknown.leader_unit = 0;
        s.armies = vec![foe, friend, sleeper, unknown];
        let w = World::from_scenario(&s, &content());
        // The army whose only units are of an unknown type keeps its record, empty.
        assert_eq!(w.armies.iter().map(|a| (a.id, a.troops.len())).collect::<Vec<_>>(), [(1, 6), (2, 1), (4, 0)]);
        assert_eq!(w.inactive.iter().map(|a| a.id).collect::<Vec<_>>(), [3]);
        assert_eq!(w.dropped_units, 2, "the unknown unit type is dropped");
        let a = &w.armies[0];
        assert!(a.hostile() && !w.armies[1].hostile());
        assert_eq!(a.tile(&w.map), (2, 2));
        assert_eq!(a.troops.len(), 6, "leader + 3 + 2");
        assert_eq!(a.leader(), Some(UnitId(1)));
        assert_eq!(a.troops.iter().filter(|t| t.unit == UnitId(5)).map(|t| t.level).collect::<Vec<_>>(), [3, 3]);
        // Placed as the original adds units (495ce0): the reserve, then the back row, whatever
        // their roles.
        let mut taken = Vec::new();
        for t in &a.troops {
            let s = content().formation.new_unit_slot(&taken).unwrap();
            assert_eq!(t.slot, s);
            taken.push(s);
        }
        assert!(a.troops.iter().all(|t| t.slot.row != Row::Front));
        assert_eq!((a.patrols, a.patrol_radius, a.gold, a.items.clone()), (true, 6, 80, vec![ItemId(7)]), "word 17 is its starting gold");
        assert_eq!(a.ai.extra_income, 30, "byte 80 × 10 is its daily income");
        assert_eq!(a.speed, 5);
    }

    #[test]
    fn army_speed_is_five_minus_the_correction() {
        assert_eq!(Army::speed_for(0, 4), 5);
        assert_eq!(Army::speed_for(1, 4), 4, "one point is 20% of the time");
        assert_eq!(Army::speed_for(-3, 4), 8);
        assert_eq!(Army::speed_for(7, 4), 1, "at least 1");
        assert_eq!(Army::speed_for(0, 2), 4, "led by the Archmage unit: one less");
        assert_eq!(Army::speed_for(4, 2), 1);
    }

    #[test]
    fn hero_starts_exactly_on_the_preset_cell() {
        let mut s = scenario(12, 12);
        let mut fort = building(BuildingType::Fort, 6, 6, (2, 2));
        fort.faction = 3;
        fort.relations = [-1, 0, 0, 0];
        s.buildings = vec![fort];
        s.header.heroes[0] = hero(3, 3, 150, &[troop(4, 0, 2), troop(5, 0, 1)]);
        s.header.heroes[0].artifacts = [7, 0, 0];
        s.header.heroes[0].mana = 100;
        // The archmage starts on a cell of the fort, his start building: he stays there and
        // the fort is his.
        s.header.heroes[1] = hero(6, 5, 500, &[troop(4, 0, 1)]);
        s.header.heroes[1].start_building = 1;
        let c = content();
        let w = World::from_scenario(&s, &c);
        let k = w.hero_start(&s, &c, HeroClass::Knight);
        assert_eq!((k.tile, k.gold, k.mana, k.items.clone(), k.location, k.owned.clone()), ((3, 3), 150, 100, vec![ItemId(7)], None, vec![]));
        assert_eq!(k.troops.len(), 3);
        assert!(k.troops.iter().all(|t| t.slot != k.hero_slot));
        let m = w.hero_start(&s, &c, HeroClass::Archmage);
        assert_eq!((m.tile, m.gold, m.location, m.owned.clone()), ((6, 5), 500, Some(0), vec![0]));
        let g = Game::from_scenario(std::sync::Arc::new(c), &s, HeroClass::Archmage);
        let f = &g.world.locations[0];
        assert_eq!((g.tile(), g.location, f.owner, f.faction, f.attitude), ((6, 5), Some(0), Owner::Player, 1, 3));
    }

    /// Razdor fixes the original's bug: a class the map does not offer (a campaign's next
    /// map can carry one) starts from the first offered preset, not on cell (0, 0).
    #[test]
    fn a_class_the_map_leaves_out_starts_from_the_first_offered_preset() {
        let mut s = scenario(12, 12);
        s.buildings = vec![building(BuildingType::Fort, 6, 6, (2, 2))];
        s.header.heroes[1] = hero(6, 5, 500, &[troop(4, 0, 1)]);
        s.header.heroes[1].start_building = 1;
        // The knight and the ranger have no start cell: the archmage is the first offered.
        assert_eq!(start_preset(&s, HeroClass::Ranger), HeroClass::Archmage);
        assert_eq!(start_preset(&s, HeroClass::Knight), HeroClass::Archmage);
        assert_eq!(start_preset(&s, HeroClass::Archmage), HeroClass::Archmage);
        let c = content();
        let w = World::from_scenario(&s, &c);
        let r = w.hero_start(&s, &c, HeroClass::Ranger);
        assert_eq!((r.class, r.tile, r.gold, r.location, r.owned.clone()), (HeroClass::Ranger, (6, 5), 500, Some(0), vec![0]));
        // With the knight offered too, he is the first.
        s.header.heroes[0] = hero(3, 3, 150, &[]);
        assert_eq!(start_preset(&s, HeroClass::Ranger), HeroClass::Knight);
        assert_eq!(w.hero_start(&s, &c, HeroClass::Ranger).tile, (3, 3));
        // A map offering nobody keeps the class's own (empty) preset.
        s.header.heroes = Default::default();
        assert_eq!(start_preset(&s, HeroClass::Ranger), HeroClass::Ranger);
    }

    #[test]
    fn the_map_loads_as_the_original_loader() {
        // saves-data.md §10 (0x4b2504).
        let mut s = scenario(20, 10);
        set(&mut s, 5, 5, Surface::ImpassableSwamp);
        // Unit ids 1–3 in a triple are skipped (not a leader); the army stands exactly on its
        // file cell, impassable or not.
        let mut a = army(1, 5, 5, -2, &[troop(4, 0, 1), troop(2, 0, 3), troop(5, 0, 1)]);
        a.leader_unit = 3;
        // Model 7 ("inactive") is not read: only byte 63 keeps an army off the map.
        let mut b = army(2, 9, 2, 1, &[troop(4, 0, 1)]);
        b.model = 7;
        let mut c = army(3, 12, 2, 1, &[troop(4, 0, 1)]);
        c.inactive = 1;
        s.armies = vec![a, b, c];
        // Owner byte 0 is the player; a building of his faction owned by nobody is not his.
        let mut mine = building(BuildingType::Village, 2, 8, (1, 1));
        mine.owner_army = 0;
        let mut theirs = building(BuildingType::Village, 4, 8, (1, 1));
        theirs.faction = 1;
        let mut fort = building(BuildingType::Fort, 8, 8, (1, 1));
        fort.garrison[0] = troop(1, 0, 2);
        fort.garrison[1] = troop(4, 0, 2);
        fort.name = "Old  fort".into();
        // A bridge flagged for the knight is his too.
        let mut bridge = building(BuildingType::StoneBridge, 15, 8, (1, 1));
        bridge.start_for = [1, 0, 0];
        s.buildings = vec![mine, theirs, fort, bridge];
        s.header.heroes[0] = hero(1, 1, 100, &[troop(1, 0, 1), troop(4, 0, 1)]);
        s.named_characters = vec![crate::dt::dtm::NamedCharacter { unit: 4, name: "Ivo  the  Bold".into() }];
        let c = content();
        let w = World::from_scenario(&s, &c);
        let a = &w.armies[0];
        assert_eq!(a.tile(&w.map), (5, 5));
        assert!(!w.map.passable((5, 5)));
        assert_eq!(a.troops.iter().map(|t| t.unit.0).collect::<Vec<_>>(), [3, 4, 5], "the leader, then 4 and 5; no unit 2");
        assert_eq!(w.armies.iter().map(|a| a.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(w.inactive.iter().map(|a| a.id).collect::<Vec<_>>(), [3]);
        let l = &w.locations;
        assert_eq!((l[0].owner, l[1].owner), (Owner::Player, Owner::Neutral));
        assert_eq!(l[2].garrison.iter().map(|t| t.unit.0).collect::<Vec<_>>(), [4, 4]);
        assert_eq!(l[2].name, "Old fort", "building strings are collapsed");
        assert_eq!(w.named_characters, ["Ivo  the  Bold"], "named characters are not");
        let k = w.hero_start(&s, &c, HeroClass::Knight);
        assert_eq!(k.troops.iter().map(|t| t.unit.0).collect::<Vec<_>>(), [4]);
        assert_eq!(k.owned, [3]);
    }

    #[test]
    fn the_loader_keeps_empty_armies_and_gives_army_spells_as_the_original() {
        // saves-data.md §10.1, §10.5, §10.6 (0x4b2504).
        let mut s = scenario(20, 10);
        // A header time of 0 starts at minute 1, not at some other date.
        s.header.start_time = 0;
        // An army record with no units is kept, off the map here (an event may fill it).
        let mut empty = army(1, 3, 3, 1, &[]);
        empty.inactive = 1;
        // Byte 84: every unit, the leader included, holds the spell in its first slot.
        let mut blessed = army(2, 6, 3, -1, &[troop(4, 0, 2)]);
        (blessed.leader_unit, blessed.spell) = (5, 3);
        s.armies = vec![empty, blessed];
        // Only towns, castles, forts and ruins read their garrison triples.
        let mut village = building(BuildingType::Village, 2, 8, (1, 1));
        village.garrison[0] = troop(4, 0, 2);
        let mut fort = building(BuildingType::Fort, 8, 8, (1, 1));
        fort.garrison[0] = troop(4, 0, 2);
        s.buildings = vec![village, fort];
        s.header.relations[0] = [1, -2, -3, -3];
        let c = content();
        let mut w = World::from_scenario(&s, &c);
        assert_eq!(w.start, Clock::at_minutes(1));
        assert_eq!(w.inactive.iter().map(|a| (a.id, a.troops.len())).collect::<Vec<_>>(), [(1, 0)]);
        let a = &w.armies[0];
        assert_eq!(a.troops.len(), 3);
        for t in &a.troops {
            assert_eq!(t.spells[0], Some(crate::rules::units::SpellSlot { spell: 3, until: MAP_ARMY_SPELL_END }));
            assert!(t.spells[1..].iter().all(Option::is_none));
        }
        assert!(w.locations[0].garrison.is_empty(), "a village has no garrison record");
        assert_eq!(w.locations[1].garrison.len(), 2);
        // A building given to the player takes the hero's four attitudes as they are.
        w.give_to_player(0);
        let v = &w.locations[0];
        assert_eq!((v.owner, v.faction, v.attitude, v.relations), (Owner::Player, 1, 1, [1, -2, -3, -3]));
    }
}

#[cfg(test)]
mod real_maps {
    //! Checks against the player's install; skipped without `RAZDOR_DT_DIR`.
    use super::*;
    use crate::dt::install::DtInstall;
    use crate::rules::game::{Event, Game};
    use std::sync::Arc;

    fn install() -> Option<(DtInstall, Arc<Content>)> {
        let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
        let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let c = Arc::new(Content::from_dt(&dt));
        Some((dt, c))
    }


    /// Non-bridge buildings whose entry the hero of `class` can walk to from his start.
    fn reachable_entries(w: &World, start: Tile) -> (usize, usize) {
        let reach = w.map.reachable(start);
        let entries: Vec<&Location> = w.locations.iter().filter(|l| !l.kind.is_bridge()).collect();
        let ok = entries.iter().filter(|l| w.map.mask_index(l.tile).is_some_and(|i| reach[i])).count();
        (ok, entries.len())
    }

    #[test]
    fn every_shipped_map_loads_into_a_world() {
        let Some((dt, c)) = install() else { return };
        let mut totals = (0, 0);
        let mut stuck = 0;
        for m in &dt.maps {
            let s = m.load().unwrap();
            let w = World::from_scenario(&s, &c);
            assert_eq!((w.map.w, w.map.h), (s.width() as i32, s.height() as i32), "{}", m.name);
            assert_eq!(w.locations.len(), s.buildings.len(), "{}", m.name);
            // Every army record, one without units too (Проклятое озеро's army 44).
            assert_eq!(w.armies.len() + w.inactive.len(), s.armies.len(), "{}: every army", m.name);
            // Every unit type is known; one РК6 garrison lists 13 units, one more than a formation holds.
            assert!(w.dropped_units <= 1, "{}: {} units dropped", m.name, w.dropped_units);
            // Exactly on the file's cell (0x4b2504), even where it cannot walk.
            for a in w.armies.iter().chain(&w.inactive) {
                let d = s.armies.iter().find(|d| d.id == a.id).expect("its record");
                assert_eq!(a.tile(&w.map), (d.x as i32, d.y as i32), "{}", m.name);
                assert!(a.figure <= 12, "{}: army {} figure {}", m.name, a.id, a.figure);
            }
            // Другой берег's "Разбойники у дороги" (army 40, editor picture 7, style 1, a
            // rogue leader) walks as the Rogue, not the Knight.
            if m.name.contains("Другой берег") {
                let a = w.armies.iter().chain(&w.inactive).find(|a| a.id == 40).unwrap();
                assert_eq!((a.model, a.figure), (7, 4), "{}", a.name);
            }
            stuck += w.armies.iter().filter(|a| if a.sails() { !w.is_sea(a.tile(&w.map)) } else { !w.map.passable(a.tile(&w.map)) }).count();
            assert!(w.locations.iter().all(|l| w.map.passable(l.tile)), "{}", m.name);
            for class in HeroClass::ALL {
                let h = w.hero_start(&s, &c, class);
                // Exactly the preset: on land, or at sea ("Тихая пристань" starts on the shallows).
                assert!(w.map.passable(h.tile) || w.is_sea(h.tile), "{} {class:?}", m.name);
                let (ok, n) = reachable_entries(&w, h.tile);
                totals.0 += ok;
                totals.1 += n;
                let g = Game::from_scenario(c.clone(), &s, class);
                // Opening events may change the preset (a companion joins, gold is given).
                if g.script().is_some_and(|e| e.total_fired() == 0) {
                    assert_eq!(g.squad.len(), 1 + h.troops.len());
                    assert_eq!(g.gold, h.gold);
                }
            }
        }
        // On foot alone; with ships every building is reachable (see below).
        assert!(totals.0 * 100 / totals.1 >= 80, "{totals:?}");
        // ДС1 and ДС2 each put one land army on a cell it cannot walk; the original leaves
        // them there too.
        assert_eq!(stuck, 2);
    }

    /// Buildings (not bridges) whose entry the hero can reach by land and sea from `start`
    /// ([`World::reachable_with_ships`]): (reached, all, ids of the others).
    fn reachable_by_ship(w: &World, start: Tile) -> (usize, usize, Vec<u16>) {
        let reach = w.reachable_with_ships(start);
        let entries: Vec<&Location> = w.locations.iter().filter(|l| !l.kind.is_bridge()).collect();
        let missed: Vec<u16> = entries.iter().filter(|l| !w.map.mask_index(l.tile).is_some_and(|i| reach[i])).map(|l| l.id).collect();
        (entries.len() - missed.len(), entries.len(), missed)
    }

    #[test]
    fn every_building_is_reachable_by_land_and_sea() {
        let Some((dt, c)) = install() else { return };
        let mut report = Vec::new();
        for m in &dt.maps {
            let s = m.load().unwrap();
            let w = World::from_scenario(&s, &c);
            for class in HeroClass::ALL {
                let h = w.hero_start(&s, &c, class);
                let (ok, n, missed) = reachable_by_ship(&w, h.tile);
                report.push((m.name.clone(), class, ok, n, missed));
            }
        }
        for (name, class, ok, n, missed) in &report {
            eprintln!("{name} {class:?}: {ok}/{n} buildings; unreached ids {missed:?}");
        }
        // Every map, except (with the original's movement rules, world.md §1):
        // - the second tutorial's church (building 15), in a ring of dense thickets and bog;
        // - the first tutorial's three villages, across a river crossed by fords: shallows
        //   are water, sailed but not walked, and the tutorial has no shipyard;
        // - РК3/РК5's altar (100/99), shut in by massifs (a square of `sprite div 10` cells
        //   each) and a lake with no shipyard;
        // - РК7's eastern island (21, 24–31), ringed by deep sea, which ships cannot sail.
        // Scripted events (teleports) are not considered.
        for (name, class, ok, n, missed) in &report {
            let expected: &[u16] = if name.starts_with("Обучающий2") {
                &[15]
            } else if name.starts_with("Обучающий1") {
                &[5, 6, 7]
            } else if name.starts_with("РК3") {
                &[100]
            } else if name.starts_with("РК5") {
                &[99]
            } else if name.starts_with("РК7") {
                &[21, 24, 25, 26, 27, 28, 29, 30, 31]
            } else {
                &[]
            };
            assert_eq!(missed.as_slice(), expected, "{name} {class:?}: {ok}/{n}");
        }
    }

    #[test]
    fn rk1_and_rk3_are_walkable_from_every_start() {
        let Some((dt, c)) = install() else { return };
        for prefix in ["РК1", "РК3"] {
            let s = dt.maps.iter().find(|m| m.name.starts_with(prefix)).unwrap().load().unwrap();
            let w = World::from_scenario(&s, &c);
            for class in HeroClass::ALL {
                let h = w.hero_start(&s, &c, class);
                assert!(w.map.passable(h.tile), "{prefix} {class:?}");
                let (ok, n) = reachable_entries(&w, h.tile);
                // РК3's altar is shut in (see above).
                let shut = usize::from(prefix == "РК3");
                assert_eq!(ok, n - shut, "{prefix} {class:?}: every building is reachable on foot or over bridges");
            }
        }
    }

    #[test]
    fn auto_walk_to_the_nearest_village_terminates() {
        let Some((dt, c)) = install() else { return };
        for prefix in ["РК1", "РК3"] {
            let s = dt.maps.iter().find(|m| m.name.starts_with(prefix)).unwrap().load().unwrap();
            let mut g = Game::from_scenario(c.clone(), &s, HeroClass::Knight);
            let (village, _) = g.world.nearest_location(g.tile(), |l| l.kind == LocationKind::Village).expect("a village");
            let target = g.world.locations[village].tile;
            let start = g.clock.total_minutes();
            let mut events = g.walk_through_fog(target);
            // An event's window at the village (РК1's) is read: then it is entered.
            events.extend(g.enter_waiting_building());
            assert!(!g.moving() || g.foe.is_some(), "{prefix}: the walk ends");
            assert!(g.clock.total_minutes() > start);
            let arrived = events.contains(&Event::Arrived(village)) && g.location == Some(village);
            let met = events.iter().any(|e| matches!(e, Event::Encounter(_) | Event::Met(_)));
            assert!(arrived || met, "{prefix}: arrives, or is stopped by an army: {events:?}");
        }
    }
}



