//! The original editor's world generator (docs/reference/editor/worldgen.md, TMakeWorld):
//! three steps the user runs one at a time, each on the open map — buildings and roads
//! ([`buildings_and_roads`]), the economy ([`economy`]) and armies and garrisons
//! ([`armies`]) — and the random army builder they use ([`builder`]).
//!
//! The same generator state, options and game data give the same world as the original's,
//! quirks included (each is named where it happens). Where the original stops a step with a
//! range error, a division by zero or an endless loop, the step stops cleanly here and says
//! why ([`Stop`]); what it wrote up to there stays, as in the original.

mod armies;
pub mod builder;
mod economy;
mod infra;
#[cfg(test)]
mod tests;

use crate::dt::dtm::Scenario;
use crate::rules::rng::Rng;

use super::cells::CellLayer;
use super::naming::NamePools;
use super::palette::{BuildingPicture, Palette};

pub use builder::Units;

/// Entries of a chance drop-down; a new window starts on the third (60 %).
pub const CHANCE_ENTRIES: u8 = 6;
pub const DEFAULT_CHANCE: u8 = 2;

/// The chance of drop-down entry `index`: (5 − index) × 20 %.
pub fn chance(index: u8) -> i32 {
    (5 - index as i32) * 20
}

/// The ten chance drop-downs: five on the first tab, five on the third.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chance {
    Towns = 0,
    Castles,
    Villages,
    Ruins,
    Other,
    TownArmies,
    CastleArmies,
    VillageArmies,
    RuinArmies,
    OtherArmies,
}

/// The sixteen budget spin boxes (§2), low and high of each range.
pub mod budget {
    pub const T1: usize = 0;
    pub const T2: usize = 1;
    pub const C1: usize = 2;
    pub const C2: usize = 3;
    pub const V1: usize = 4;
    pub const V2: usize = 5;
    pub const R1: usize = 6;
    pub const R2: usize = 7;
    pub const O1: usize = 8;
    pub const O2: usize = 9;
    pub const TG1: usize = 10;
    pub const TG2: usize = 11;
    pub const CG1: usize = 12;
    pub const CG2: usize = 13;
    pub const RG1: usize = 14;
    pub const RG2: usize = 15;
    /// The spin boxes' range and step.
    pub const MAX: i32 = 50_000;
    pub const STEP: i32 = 100;
}

/// What the window holds when it runs a step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The drop-downs' entries, by [`Chance`].
    pub chances: [u8; 10],
    /// The income grid: town gold, castle gold, village gold, village mana (cell text).
    pub income: [String; 4],
    /// The trade grid: town, market, church rows of (minimum price, maximum price, goods).
    pub trade: [[String; 3]; 3],
    /// The library grid: town, church rows of (minimum price, maximum price, spells).
    pub library: [[String; 3]; 2],
    /// The spin boxes, by [`budget`].
    pub budgets: [i32; 16],
    /// The minimum-point keypad: 0 none, else the numeric keypad's key (7 top-left, 3
    /// bottom-right).
    pub min_point: u8,
    /// Only buildings whose owner is "none" get armies; existing armies stay.
    pub unowned_only: bool,
    /// Town and castle armies' buildings become enemies.
    pub enemies_only: bool,
}

impl Options {
    /// A new window's values (FormCreate 0x56e858) on a map `w` cells wide: the window is
    /// made anew at every opening, so the budgets always follow the open map.
    pub fn new(w: u32) -> Options {
        let w = w as i32;
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let row = |v: [&str; 3]| -> [String; 3] { s(&v).try_into().expect("three") };
        let budgets = [
            2 * w + 500,
            3 * w + 500,
            w + 200,
            2 * w + 200,
            200,
            w / 2 + 200,
            2 * w + 500,
            4 * w + 1000,
            w + 500,
            4 * w + 500,
            2 * w + 1500,
            3 * w + 1500,
            2 * w + 250,
            3 * w + 500,
            3 * w + 500,
            6 * w + 1000,
        ]
        .map(|v| v.clamp(0, budget::MAX));
        Options {
            chances: [DEFAULT_CHANCE; 10],
            income: s(&["100", "50", "75", "75"]).try_into().expect("four"),
            trade: [row(["25", "3000", "10"]), row(["125", "2000", "8"]), row(["25", "250", "6"])],
            library: [row(["250", "5000", "5"]), row(["100", "2000", "3"])],
            budgets,
            min_point: 0,
            unowned_only: false,
            enemies_only: false,
        }
    }

    pub fn chance(&self, c: Chance) -> i32 {
        chance(self.chances[c as usize])
    }

    /// The minimum point of the keypad on a `w` × `h` map (0x574752).
    pub fn point(&self, w: u32, h: u32) -> Option<(i32, i32)> {
        let (w, h) = (w as i32, h as i32);
        let (px, py) = match self.min_point {
            1 => (0, h),
            2 => (w / 2, h),
            3 => (w, h),
            4 => (0, h / 2),
            5 => (w / 2, h / 2),
            6 => (w, h / 2),
            7 => (0, 0),
            8 => (w / 2, 0),
            9 => (w, 0),
            _ => return None,
        };
        Some((px, py))
    }
}

/// Delphi's `StrToIntDef` of a grid cell: an optional sign and decimal digits, or `$` and
/// hexadecimal ones, after leading blanks; anything else, or a value outside 32 bits, gives
/// `default`.
pub fn grid_int(text: &str, default: i32) -> i32 {
    let t = text.trim_start();
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let v = match t.strip_prefix('$') {
        Some(hex) if !hex.is_empty() => i64::from_str_radix(hex, 16).ok(),
        _ if !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) => t.parse::<i64>().ok(),
        _ => None,
    };
    v.map(|v| if neg { -v } else { v }).and_then(|v| i32::try_from(v).ok()).unwrap_or(default)
}

/// The building pictures as the original numbers them: every picture of the install in
/// picture-type then variant order, a type's pictures one after another (the draws assume
/// that, §3.11).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pictures {
    list: Vec<BuildingPicture>,
}

impl Pictures {
    pub fn from_palette(p: &Palette) -> Pictures {
        let mut list = p.buildings.clone();
        list.sort_by_key(|b| (b.picture_type, b.variant));
        Pictures { list }
    }

    /// The first picture of type `t` (a type without pictures has none).
    pub fn first(&self, t: u8) -> Option<usize> {
        self.list.iter().position(|b| b.picture_type == t)
    }

    /// How many pictures type `t` has.
    pub fn count(&self, t: u8) -> i32 {
        self.list.iter().filter(|b| b.picture_type == t).count() as i32
    }

    /// The generator's picture draw for type `t`: `R(count) + first`.
    pub fn draw(&self, rng: &mut Rng, t: u8) -> Option<usize> {
        let r = rng.random(self.count(t)) as usize;
        self.first(t).map(|f| f + r)
    }

    pub fn get(&self, serial: Option<usize>) -> Option<BuildingPicture> {
        serial.and_then(|s| self.list.get(s)).copied()
    }

    /// The largest side of a footprint of type `t` (0 without pictures).
    pub fn largest(&self, t: u8) -> u8 {
        self.list.iter().filter(|b| b.picture_type == t).map(|b| b.size.0.max(b.size.1)).max().unwrap_or(0)
    }

    /// The footprint of picture `(t, variant)`.
    pub fn size(&self, t: u8, variant: u8) -> Option<(u8, u8)> {
        self.list.iter().find(|b| (b.picture_type, b.variant) == (t, variant)).map(|b| b.size)
    }
}

/// What the steps read besides the map and the options.
#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub pictures: &'a Pictures,
    /// The building names of the editor's language ini (empty names without it).
    pub names: Option<&'a NamePools>,
    /// The units, themes and wages of the army builder.
    pub units: &'a Units,
    /// `CostGold` of each spell, by 1-based index − 1.
    pub spell_prices: &'a [i32],
    /// The main window's brush size, which the original's placement checks (0x595390).
    pub brush: u32,
    /// The processor clock `Randomize` reads (the first step's seed).
    pub clock: u32,
}

/// The ten counters the window shows (the original's labels).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub towns: u32,
    pub villages: u32,
    pub castles: u32,
    pub forts: u32,
    pub shipyards: u32,
    pub bridges: u32,
    pub taverns: u32,
    pub churches: u32,
    pub markets: u32,
    pub ruins: u32,
}

/// The counts the window shows when it opens (0x56e3f8), by record type. Its two bridge
/// cases test the type they were chosen by against itself the other way round, so bridges
/// never count (the original's slip, kept).
pub fn existing(s: &Scenario) -> Counters {
    let mut c = Counters::default();
    for b in &s.buildings {
        match b.kind {
            1 => c.towns += 1,
            2 => c.villages += 1,
            3 => c.castles += 1,
            4 => c.forts += 1,
            5 => c.taverns += 1,
            6 => c.markets += 1,
            7 => c.churches += 1,
            9 => c.shipyards += 1,
            12 => c.ruins += 1,
            _ => {}
        }
    }
    c
}

/// Why a step stopped before its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// Building `building`'s footprint reaches past the map's left or top edge: the
    /// original's clearing reads the cell there with a range error (§3.1).
    FootprintOffMap { building: u16 },
    /// The map is narrower than 50 cells: the sector count is 0 and the original divides by
    /// it (§3.2).
    NarrowMap,
    /// A town of the sector loop's extra visit on a map 800 or more cells wide: its row is
    /// outside the town table (§3.2).
    TownTable,
    /// A town's search succeeded but its placement was refused while no building stood yet:
    /// its table entry names record 0, which the road pass reads with a range error (§3.6).
    TownRecord,
    /// A junction building at `(x, y)` whose mask box (or neighbour read) reaches column or
    /// row −1 (§3.9).
    JunctionAtEdge { x: i32, y: i32 },
    /// Building `building`'s economy value does not fit its field (§4).
    EconomyValue { building: u16 },
    /// The daily income of building `building`'s army, `value` tens, does not fit its byte
    /// (§5.4).
    ArmyIncome { building: u16, value: i32 },
    /// The gold of building `building`'s army, `value`, is outside the army's signed 16-bit
    /// field (a maximum gold or a ruin budget above 32767, §5.4).
    ArmyGold { building: u16, value: i32 },
    /// A theme lists a unit id above 255, which an army's byte cannot hold.
    UnitId { building: u16 },
    /// The builder's window for building `building` can never take a unit of the theme: the
    /// original loops for ever (§6.3).
    Hang { building: u16, slot: u8, lo: i32, hi: i32 },
}

/// What a step did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub counters: Counters,
    pub stop: Option<Stop>,
}

/// Step 1 (0x56ffa4): buildings and roads. Throws away every building and the road layer,
/// then places towns, castles and villages by sector, links them by roads with bridges,
/// puts buildings on road junctions and ruins in the wild.
pub fn buildings_and_roads(s: &mut Scenario, cells: &mut CellLayer, rng: &mut Rng, inp: &Inputs, o: &Options) -> Report {
    infra::run(s, cells, rng, inp, o)
}

/// Step 2 (0x573224): incomes, trade, spells, barracks and factions of every building.
pub fn economy(s: &mut Scenario, rng: &mut Rng, inp: &Inputs, o: &Options) -> Report {
    economy::run(s, rng, inp, o)
}

/// Step 3 (0x57466c): an army or a garrison for every building.
pub fn armies(s: &mut Scenario, cells: &mut CellLayer, rng: &mut Rng, inp: &Inputs, o: &Options) -> Report {
    armies::run(s, cells, rng, inp, o)
}
