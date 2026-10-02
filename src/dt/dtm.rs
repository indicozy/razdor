//! `.DTm` scenario maps: header, terrain, objects, buildings, armies, points, events,
//! strings and embedded pictures.
//!
//! The byte layout is documented in `docs/reference/dtm-format.md`; offsets in the comments
//! below are 0-based offsets inside each record, as there. Every byte of the payload is kept:
//! fields whose meaning is unknown are stored as raw `unknown_*` arrays, so
//! [`Scenario::to_payload`] rebuilds the original payload exactly.

use super::{container, text, DtError};
use std::path::Path;

/// First bytes of the uncompressed payload.
pub const PAYLOAD_MAGIC: &[u8; 12] = b"MapLDV V.4\r\n";
/// Size of the fixed header.
pub const HEADER_SIZE: usize = 0x12F;
/// Marker between the binary sections and the strings.
pub const TEXT_MARKER: &[u8; 8] = b"\x08>-Text-";
/// Container version of all shipped maps.
pub const CONTAINER_VERSION: u16 = 19;

pub const OBJECT_SIZE: usize = 6;
pub const BUILDING_SIZE: usize = 358;
pub const ARMY_SIZE: usize = 89;
pub const POINT_SIZE: usize = 99;
pub const EVENT_SIZE: usize = 171;
const HERO_PRESET_SIZE: usize = 50;
const MINUTES_PER_DAY: u32 = 24 * 60;
/// The start time of a "relative only" event (year 2000 of the game clock): it never opens
/// by itself; another event's "relative event" result moves its start.
pub const RELATIVE_START: u32 = 1_036_800_000;

// ------------------------------------------------------------------------------------------
// Byte helpers
// ------------------------------------------------------------------------------------------

/// Little-endian reads at fixed offsets of a record.
struct Rec<'a>(&'a [u8]);

impl Rec<'_> {
    fn u8(&self, o: usize) -> u8 {
        self.0[o]
    }
    fn i8(&self, o: usize) -> i8 {
        self.0[o] as i8
    }
    fn u16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.0[o], self.0[o + 1]])
    }
    fn i16(&self, o: usize) -> i16 {
        self.u16(o) as i16
    }
    fn u32(&self, o: usize) -> u32 {
        u32::from_le_bytes(self.arr(o))
    }
    fn arr<const N: usize>(&self, o: usize) -> [u8; N] {
        self.0[o..o + N].try_into().expect("in bounds")
    }
    fn i8s<const N: usize>(&self, o: usize) -> [i8; N] {
        self.arr::<N>(o).map(|b| b as i8)
    }
    fn u16s<const N: usize>(&self, o: usize) -> [u16; N] {
        std::array::from_fn(|k| self.u16(o + 2 * k))
    }
    fn troops(&self, o: usize) -> [Troop; 6] {
        std::array::from_fn(|k| {
            let [unit, level, count] = self.arr(o + 3 * k);
            Troop { unit, level, count }
        })
    }
}

/// Little-endian writes at fixed offsets of a record buffer.
struct Put<'a>(&'a mut [u8]);

impl Put<'_> {
    fn u8(&mut self, o: usize, v: u8) {
        self.0[o] = v;
    }
    fn i8(&mut self, o: usize, v: i8) {
        self.0[o] = v as u8;
    }
    fn u16(&mut self, o: usize, v: u16) {
        self.bytes(o, &v.to_le_bytes());
    }
    fn i16(&mut self, o: usize, v: i16) {
        self.u16(o, v as u16);
    }
    fn u32(&mut self, o: usize, v: u32) {
        self.bytes(o, &v.to_le_bytes());
    }
    fn bytes(&mut self, o: usize, v: &[u8]) {
        self.0[o..o + v.len()].copy_from_slice(v);
    }
    fn i8s(&mut self, o: usize, v: &[i8]) {
        for (k, x) in v.iter().enumerate() {
            self.i8(o + k, *x);
        }
    }
    fn u16s(&mut self, o: usize, v: &[u16]) {
        for (k, x) in v.iter().enumerate() {
            self.u16(o + 2 * k, *x);
        }
    }
    fn troops(&mut self, o: usize, v: &[Troop; 6]) {
        for (k, t) in v.iter().enumerate() {
            self.bytes(o + 3 * k, &[t.unit, t.level, t.count]);
        }
    }
}

/// Non-zero ids of a slot list.
fn nonzero<T: Copy + Default + PartialEq>(v: &[T]) -> impl Iterator<Item = T> + '_ {
    v.iter().copied().filter(|x| *x != T::default())
}

// ------------------------------------------------------------------------------------------
// Game clock
// ------------------------------------------------------------------------------------------

/// A calendar date of the game clock: minutes since year 0, month 1, day 1, 00:00, with
/// 30-day months and 12-month years.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GameDate {
    pub year: u32,
    /// 1..=12
    pub month: u32,
    /// 1..=30
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

impl GameDate {
    pub fn from_minutes(m: u32) -> GameDate {
        let days = m / MINUTES_PER_DAY;
        GameDate {
            year: days / 360,
            month: days / 30 % 12 + 1,
            day: days % 30 + 1,
            hour: m % MINUTES_PER_DAY / 60,
            minute: m % 60,
        }
    }

    pub fn to_minutes(self) -> u32 {
        (((self.year * 12 + self.month - 1) * 30 + self.day - 1) * 24 + self.hour) * 60 + self.minute
    }
}

// ------------------------------------------------------------------------------------------
// Header
// ------------------------------------------------------------------------------------------

/// A unit slot: type, level and number of units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Troop {
    pub unit: u8,
    /// Level (L: the format doc is not sure which of level/count is which).
    pub level: u8,
    pub count: u8,
}

impl Troop {
    pub fn is_empty(&self) -> bool {
        self.unit == 0 && self.level == 0 && self.count == 0
    }
}

/// A barracks slot: unit type, stock at start, maximum stock (stored like a [`Troop`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecruitSlot {
    pub unit: u8,
    pub start_count: u8,
    pub max_count: u8,
}

/// Starting hero preset, one per hero class (knight, archmage, ranger). 50 bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeroPreset {
    /// 0: always 0 in shipped maps (U).
    pub unknown_0: u32,
    /// 4: always 0 (U).
    pub unknown_4: u32,
    /// 8: starting gold, the same for every class in the shipped maps (C: the exe reads
    /// it as a signed 16-bit value).
    pub gold: u32,
    /// 12: starting mana, differing by class (C: signed 16-bit).
    pub mana: u32,
    /// 16: starting building (1-based, 0 = none).
    pub start_building: u8,
    /// 17: always 0 (U).
    pub unknown_17: [u8; 2],
    /// 19: starting troops.
    pub troops: [Troop; 6],
    /// 37, 39: start cell.
    pub x: u16,
    pub y: u16,
    /// 41: starting artifacts (GlobalIndex, 0 = none) (L).
    pub artifacts: [u8; 3],
    /// 44: starting spells (1-based spell index, 0 = none) (L).
    pub spells: [u8; 6],
}

impl HeroPreset {
    fn read(r: &Rec) -> HeroPreset {
        HeroPreset {
            unknown_0: r.u32(0),
            unknown_4: r.u32(4),
            gold: r.u32(8),
            mana: r.u32(12),
            start_building: r.u8(16),
            unknown_17: r.arr(17),
            troops: r.troops(19),
            x: r.u16(37),
            y: r.u16(39),
            artifacts: r.arr(41),
            spells: r.arr(44),
        }
    }

    fn write(&self, p: &mut Put) {
        p.u32(0, self.unknown_0);
        p.u32(4, self.unknown_4);
        p.u32(8, self.gold);
        p.u32(12, self.mana);
        p.u8(16, self.start_building);
        p.bytes(17, &self.unknown_17);
        p.troops(19, &self.troops);
        p.u16(37, self.x);
        p.u16(39, self.y);
        p.bytes(41, &self.artifacts);
        p.bytes(44, &self.spells);
    }
}

/// Hero class index into [`Header::heroes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Archetype {
    Knight = 0,
    Archmage = 1,
    Ranger = 2,
}

/// Header byte 0x10F.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenarioKind {
    Standalone,
    CampaignStart,
    CampaignContinuation,
    Other(u8),
}

/// The fixed 303-byte header, minus the section sizes and text offset, which are derived
/// from the content (and validated when parsing).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    /// 0x0C, 0x10: grid size in cells.
    pub width: u32,
    pub height: u32,
    /// 0x14: seed of the editor's map generator (L).
    pub generator_seed: u32,
    /// 0x34: always 0 (U).
    pub unknown_0x34: u32,
    /// 0x38: start time in minutes (see [`GameDate`]); 0 = unset.
    pub start_time: u32,
    /// 0x3C: knight, archmage, ranger presets.
    pub heroes: [HeroPreset; 3],
    /// 0xD2: victory event (1-based, 0 = none).
    pub victory_event: u16,
    /// 0xD4: always 0 (U).
    pub unknown_0xd4: [u8; 4],
    /// 0xD8: defeat event (1-based, 0 = none).
    pub defeat_event: u16,
    /// 0xDA: always 0 (U).
    pub unknown_0xda: [u8; 4],
    /// 0xDE: relation matrix, rows and columns player, ally, neighbour, enemy; −3..3.
    pub relations: [[i8; 4]; 4],
    /// 0xEF: unit id of each named character. Entries past the count may be stale; the
    /// used ones are repeated in [`Scenario::named_characters`].
    pub named_character_slots: [u8; 32],
    /// 0x10F: raw scenario kind, see [`Header::kind`].
    pub scenario_kind: u8,
    /// 0x110: carried over from the previous campaign map, in UI order: gold, gods' favour,
    /// fame, experience/level, personal artifacts, whole inventory, whole army (L).
    pub carry_over: [u8; 7],
    /// 0x117: the editor's demo flag ([`Header::demo_flag`]), then 4 bytes always 0 (U).
    pub unknown_0x117: [u8; 5],
    /// 0x120: built-in scenario picture choice (L).
    pub scenario_picture_index: u8,
    /// 0x121: the game's row-width byte, then the editor's playability score, save counter
    /// and quest count ([`Header::playability`] and the next two), then zeros (U).
    pub unknown_0x121: [u8; 14],
}

impl Header {
    /// 0x117: the Community editor's demo flag (1 in its `.DTs` maps, whose sections are
    /// stored in another order).
    pub fn demo_flag(&self) -> u8 {
        self.unknown_0x117[0]
    }

    pub fn set_demo_flag(&mut self, v: u8) {
        self.unknown_0x117[0] = v;
    }

    /// 0x122: the playability score of the Community editor's last scoring.
    pub fn playability(&self) -> u16 {
        u16::from_le_bytes([self.unknown_0x121[1], self.unknown_0x121[2]])
    }

    pub fn set_playability(&mut self, v: u16) {
        self.unknown_0x121[1..3].copy_from_slice(&v.to_le_bytes());
    }

    /// 0x124: the Community editor's save counter.
    pub fn save_counter(&self) -> u16 {
        u16::from_le_bytes([self.unknown_0x121[3], self.unknown_0x121[4]])
    }

    pub fn set_save_counter(&mut self, v: u16) {
        self.unknown_0x121[3..5].copy_from_slice(&v.to_le_bytes());
    }

    /// 0x126: the number of quests, written with the score.
    pub fn quest_count(&self) -> u8 {
        self.unknown_0x121[5]
    }

    pub fn set_quest_count(&mut self, v: u8) {
        self.unknown_0x121[5] = v;
    }

    pub fn kind(&self) -> ScenarioKind {
        match self.scenario_kind {
            0 => ScenarioKind::Standalone,
            1 => ScenarioKind::CampaignStart,
            2 => ScenarioKind::CampaignContinuation,
            k => ScenarioKind::Other(k),
        }
    }

    pub fn start_date(&self) -> GameDate {
        GameDate::from_minutes(self.start_time)
    }

    pub fn hero(&self, a: Archetype) -> &HeroPreset {
        &self.heroes[a as usize]
    }
}

// ------------------------------------------------------------------------------------------
// Terrain and objects
// ------------------------------------------------------------------------------------------

/// Terrain codes, in the order of the editor's surface palette (L).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    ShallowsFords = 0,
    CoastalWater = 1,
    DeepSea = 2,
    LavaFields = 3,
    Road = 4,
    GrassLowland = 5,
    GrassPlain = 6,
    DryPlain = 7,
    Marsh = 8,
    ImpassableSwamp = 9,
    SandDunes = 10,
    ClaySoil = 11,
    StonySoil = 12,
    ScorchedLand = 13,
    SnowyGround = 14,
    ImpassableSnowdrifts = 15,
}

impl Surface {
    pub const ALL: [Surface; 16] = [
        Surface::ShallowsFords,
        Surface::CoastalWater,
        Surface::DeepSea,
        Surface::LavaFields,
        Surface::Road,
        Surface::GrassLowland,
        Surface::GrassPlain,
        Surface::DryPlain,
        Surface::Marsh,
        Surface::ImpassableSwamp,
        Surface::SandDunes,
        Surface::ClaySoil,
        Surface::StonySoil,
        Surface::ScorchedLand,
        Surface::SnowyGround,
        Surface::ImpassableSnowdrifts,
    ];

    pub fn from_code(code: u8) -> Option<Surface> {
        Surface::ALL.get(code as usize).copied()
    }
}

/// Largest map Razdor allocates (the original allocates whatever the header says).
const MAX_CELLS: u64 = 1 << 26;

/// Expand the terrain stream of `(value, run − 1)` byte pairs into `width*height` cells as
/// the loader does (0x4b2504, saves-data.md §10.2): `size div 2` pairs (an odd last byte is
/// ignored), written cell after cell with no bounds check, so cells past the map's last row
/// land in its border (overwritten by the border copy) or beyond, and are lost; cells the
/// stream does not reach stay 0 *(guess: the buffer's first contents are not traced)*.
pub fn expand_terrain(rle: &[u8], width: u32, height: u32) -> Result<Vec<u8>, DtError> {
    let cells = width as u64 * height as u64;
    if cells > MAX_CELLS {
        return Err(DtError::Terrain(format!("{width}x{height} is too big a map")));
    }
    let cells = cells as usize;
    let mut out = Vec::with_capacity(cells);
    for pair in rle.chunks_exact(2) {
        if out.len() >= cells {
            break;
        }
        out.extend(std::iter::repeat_n(pair[0], pair[1] as usize + 1));
    }
    out.resize(cells, 0);
    Ok(out)
}

/// Compress cells into the terrain stream (greedy runs of up to 256).
pub fn compress_terrain(cells: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < cells.len() {
        let v = cells[i];
        let run = cells[i..].iter().take(256).take_while(|c| **c == v).count();
        out.extend_from_slice(&[v, (run - 1) as u8]);
        i += run;
    }
    out
}

/// A hill, mountain, tree or stone on a cell (6 bytes). Class meanings are L:
/// 1 hills, 5 mountains/rocks, 8 stone scatter, 9 trees, 10 dead trees, 11 dense thicket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapObject {
    pub x: u16,
    pub y: u16,
    /// Sprite id from the editor's object palette.
    pub sprite: u8,
    pub class: u8,
}

// ------------------------------------------------------------------------------------------
// Buildings
// ------------------------------------------------------------------------------------------

/// Building type (byte 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuildingType {
    Palace = 0,
    Town = 1,
    Village = 2,
    Castle = 3,
    Fort = 4,
    Tavern = 5,
    Market = 6,
    Church = 7,
    Smithy = 8,
    Shipyard = 9,
    Altar = 10,
    DungeonEntrance = 11,
    Ruins = 12,
    StoneBridge = 13,
    WoodenBridge = 14,
    Obelisk = 15,
}

impl BuildingType {
    pub const ALL: [BuildingType; 16] = [
        BuildingType::Palace,
        BuildingType::Town,
        BuildingType::Village,
        BuildingType::Castle,
        BuildingType::Fort,
        BuildingType::Tavern,
        BuildingType::Market,
        BuildingType::Church,
        BuildingType::Smithy,
        BuildingType::Shipyard,
        BuildingType::Altar,
        BuildingType::DungeonEntrance,
        BuildingType::Ruins,
        BuildingType::StoneBridge,
        BuildingType::WoodenBridge,
        BuildingType::Obelisk,
    ];

    pub fn from_code(code: u8) -> Option<BuildingType> {
        BuildingType::ALL.get(code as usize).copied()
    }
}

/// A building (358 bytes). Buildings have 1-based ids in file order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Building {
    /// 0, 2: bottom-right cell of the footprint (L).
    pub x: u16,
    pub y: u16,
    /// 4: picture variant within the type.
    pub picture_variant: u8,
    /// 5: picture type (almost always the building type).
    pub picture_type: u8,
    /// 6: raw building type, see [`Building::building_type`].
    pub kind: u8,
    /// 7: always 0 (U).
    pub unknown_7: u8,
    /// 8: local event slots (1-based event ids); see [`Building::events`].
    pub event_slots: [u16; 64],
    /// 136: fixed artifacts (market goods or ruin treasure), GlobalIndex, 0 = empty.
    pub artifact_slots: [u16; 64],
    /// 264: barracks stock.
    pub barracks: [RecruitSlot; 6],
    /// 282: gold income per day.
    pub gold_per_day: u16,
    /// 284: maximum accumulated gold (villages).
    pub gold_max: u16,
    /// 286: always 0 (U).
    pub unknown_286: u16,
    /// 288: number of used event slots.
    pub event_count: u8,
    /// 289, 290: footprint size.
    pub size_x: u8,
    pub size_y: u8,
    /// 291: always 0 (U).
    pub unknown_291: u8,
    /// 292: owner army id; 0xFF = none (the neutral owner applies).
    pub owner_army: u8,
    /// 293: linked building (1-based): a village's castle; a dungeon entrance's target (L).
    pub linked_building: u8,
    /// 294: has barracks.
    pub has_barracks: u8,
    /// 295: number of random artifacts for sale.
    pub random_artifacts_for_sale: u8,
    /// 296: stale u8 copy of the artifact list; ignore (L).
    pub stale_artifacts: [u8; 6],
    /// 302: always 0 (U).
    pub unknown_302: [u8; 6],
    /// 308: spells for sale (1-based spell index, 0 = none).
    pub spells_for_sale: [u8; 6],
    /// 314: garrison.
    pub garrison: [Troop; 6],
    /// 332: extra garrison defence.
    pub garrison_extra_defence: u8,
    /// 333: minimum random-artifact price.
    pub price_min: u16,
    /// 335: maximum random-artifact price; for ruins, the treasure gold.
    pub price_max: u16,
    /// 337: faction: 1 player, 2 ally, 3 neighbour, 4 enemy.
    pub faction: u8,
    /// 338: attitude towards player, ally, neighbour and enemy (−3..3).
    pub relations: [i8; 4],
    /// 342: always 0 (U).
    pub unknown_342: [u8; 8],
    /// 350: mana income per day.
    pub mana_per_day: u8,
    /// 351: maximum mana.
    pub mana_max: u8,
    /// 352: always 0 (U).
    pub unknown_352: u8,
    /// 353: starting building flag for knight, archmage, ranger.
    pub start_for: [u8; 3],
    /// 356: "all types" recruitment flag.
    pub recruit_all_types: u8,
    /// 357: garrison is AI-only.
    pub garrison_ai_only: u8,
    /// Strings: name, neutral owner's name, description.
    pub name: String,
    pub owner_name: String,
    pub description: String,
}

impl Default for Building {
    /// An all-zero record with the neutral owner, for building scenarios in code.
    fn default() -> Building {
        Building { owner_army: 0xFF, ..Building::read(&Rec(&[0; BUILDING_SIZE])) }
    }
}

impl Building {
    fn read(r: &Rec) -> Building {
        Building {
            x: r.u16(0),
            y: r.u16(2),
            picture_variant: r.u8(4),
            picture_type: r.u8(5),
            kind: r.u8(6),
            unknown_7: r.u8(7),
            event_slots: r.u16s(8),
            artifact_slots: r.u16s(136),
            barracks: r.troops(264).map(|t| RecruitSlot { unit: t.unit, start_count: t.level, max_count: t.count }),
            gold_per_day: r.u16(282),
            gold_max: r.u16(284),
            unknown_286: r.u16(286),
            event_count: r.u8(288),
            size_x: r.u8(289),
            size_y: r.u8(290),
            unknown_291: r.u8(291),
            owner_army: r.u8(292),
            linked_building: r.u8(293),
            has_barracks: r.u8(294),
            random_artifacts_for_sale: r.u8(295),
            stale_artifacts: r.arr(296),
            unknown_302: r.arr(302),
            spells_for_sale: r.arr(308),
            garrison: r.troops(314),
            garrison_extra_defence: r.u8(332),
            price_min: r.u16(333),
            price_max: r.u16(335),
            faction: r.u8(337),
            relations: r.i8s(338),
            unknown_342: r.arr(342),
            mana_per_day: r.u8(350),
            mana_max: r.u8(351),
            unknown_352: r.u8(352),
            start_for: r.arr(353),
            recruit_all_types: r.u8(356),
            garrison_ai_only: r.u8(357),
            name: String::new(),
            owner_name: String::new(),
            description: String::new(),
        }
    }

    fn write(&self, p: &mut Put) {
        p.u16(0, self.x);
        p.u16(2, self.y);
        p.u8(4, self.picture_variant);
        p.u8(5, self.picture_type);
        p.u8(6, self.kind);
        p.u8(7, self.unknown_7);
        p.u16s(8, &self.event_slots);
        p.u16s(136, &self.artifact_slots);
        p.troops(264, &self.barracks.map(|s| Troop { unit: s.unit, level: s.start_count, count: s.max_count }));
        p.u16(282, self.gold_per_day);
        p.u16(284, self.gold_max);
        p.u16(286, self.unknown_286);
        p.u8(288, self.event_count);
        p.u8(289, self.size_x);
        p.u8(290, self.size_y);
        p.u8(291, self.unknown_291);
        p.u8(292, self.owner_army);
        p.u8(293, self.linked_building);
        p.u8(294, self.has_barracks);
        p.u8(295, self.random_artifacts_for_sale);
        p.bytes(296, &self.stale_artifacts);
        p.bytes(302, &self.unknown_302);
        p.bytes(308, &self.spells_for_sale);
        p.troops(314, &self.garrison);
        p.u8(332, self.garrison_extra_defence);
        p.u16(333, self.price_min);
        p.u16(335, self.price_max);
        p.u8(337, self.faction);
        p.i8s(338, &self.relations);
        p.bytes(342, &self.unknown_342);
        p.u8(350, self.mana_per_day);
        p.u8(351, self.mana_max);
        p.u8(352, self.unknown_352);
        p.bytes(353, &self.start_for);
        p.u8(356, self.recruit_all_types);
        p.u8(357, self.garrison_ai_only);
    }

    pub fn building_type(&self) -> Option<BuildingType> {
        BuildingType::from_code(self.kind)
    }

    /// Local event ids: the first `event_count` slots, deleted (0) slots skipped.
    pub fn events(&self) -> impl Iterator<Item = u16> + '_ {
        nonzero(&self.event_slots[..(self.event_count as usize).min(64)])
    }

    /// Fixed artifact ids (market goods or ruin treasure).
    pub fn artifacts(&self) -> impl Iterator<Item = u16> + '_ {
        nonzero(&self.artifact_slots)
    }

    /// Owner army id, `None` for the neutral owner.
    pub fn owner(&self) -> Option<u8> {
        (self.owner_army != 0xFF).then_some(self.owner_army)
    }
}

// ------------------------------------------------------------------------------------------
// Armies
// ------------------------------------------------------------------------------------------

/// Army map model (byte 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArmyModel {
    HeroKnight = 1,
    HeroArchmage = 2,
    HeroRanger = 3,
    Feudal = 4,
    Bandits = 5,
    Peasants = 6,
    Inactive = 7,
    Lantern = 8,
    EventPoint = 9,
    Necromancer = 10,
    Ghosts = 11,
    Zombies = 12,
}

impl ArmyModel {
    pub const ALL: [ArmyModel; 12] = [
        ArmyModel::HeroKnight,
        ArmyModel::HeroArchmage,
        ArmyModel::HeroRanger,
        ArmyModel::Feudal,
        ArmyModel::Bandits,
        ArmyModel::Peasants,
        ArmyModel::Inactive,
        ArmyModel::Lantern,
        ArmyModel::EventPoint,
        ArmyModel::Necromancer,
        ArmyModel::Ghosts,
        ArmyModel::Zombies,
    ];

    pub fn from_code(code: u8) -> Option<ArmyModel> {
        code.checked_sub(1).and_then(|i| ArmyModel::ALL.get(i as usize)).copied()
    }
}

/// An AI army (89 bytes). The army id (byte 4) equals the 1-based record index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Army {
    /// 0, 2: cell.
    pub x: u16,
    pub y: u16,
    /// 4: army id (1-based).
    pub id: u8,
    /// 5: raw map model, see [`Army::model`].
    pub model: u8,
    /// 6: tactical cost, editor value 1 (L).
    pub tactical_cost_1: u16,
    /// 8: 0..3, maybe the leader archetype (U).
    pub unknown_8: u8,
    /// 9: always 0 (U).
    pub unknown_9: [u8; 4],
    /// 13: speed correction.
    pub speed_correction: i8,
    /// 14: "add experience like the player".
    pub exp_like_player: u8,
    /// 15: always 0 (U).
    pub unknown_15: [u8; 2],
    /// 17: starting gold (world.md §5; the daily income is byte 80 × 10).
    pub gold_income: u16,
    /// 19: bonus experience for hired units.
    pub hire_bonus_exp: u16,
    /// 21: always 0 (U).
    pub unknown_21: [u8; 4],
    /// 25: home building (1-based, 0 = none).
    pub home_building: u8,
    /// 26: leader unit id.
    pub leader_unit: u8,
    /// 27: leader level (L).
    pub leader_level: u8,
    /// 28: troops.
    pub troops: [Troop; 6],
    /// 46: always 0 (U).
    pub unknown_46: [u8; 4],
    /// 50: artifacts carried (GlobalIndex, 0 = none).
    pub artifacts: [u8; 3],
    /// 53: always 0 (U).
    pub unknown_53: [u8; 5],
    /// 58: named character (1-based index into the named characters, 0 = none).
    pub named_character: u8,
    /// 59: behaviour style: 0 feudal, 1 rogue, 2 peasant (L: it matches the map model of every
    /// model 4, 5 and 6 army of the shipped maps, and model-7 armies carry it too).
    pub behaviour: u8,
    /// 60: patrols.
    pub patrols: u8,
    /// 61: patrol radius.
    pub patrol_radius: u8,
    /// 62: units carry no money.
    pub no_money: u8,
    /// 63: inactive at start: 1 exactly for the armies whose model is 7 ("inactive") in every
    /// shipped map (L). Events activate such armies later.
    pub inactive: u8,
    /// 64: faction: 1 player, 2 ally, 3 neighbour, 4 enemy.
    pub faction: u8,
    /// 65: attitude towards the four factions.
    pub relations: [i8; 4],
    /// 69: aggression.
    pub aggression: i8,
    /// 70: respawn time in days.
    pub respawn_days: u8,
    /// 71: experience correction in percent (100 = normal).
    pub exp_correction: u8,
    /// 72: ship type: 0 none, then hero, pirate, merchant (L).
    pub ship: u8,
    /// 73: always 0 (U).
    pub unknown_73: u8,
    /// 74: tactical cost, editor value 2 (L).
    pub tactical_cost_2: u16,
    /// 76: ignored by AI.
    pub ignored_by_ai: u8,
    /// 77: hunts only the player.
    pub hunts_player_only: u8,
    /// 78: no random targets.
    pub no_random_targets: u8,
    /// 79: no socialising with other armies.
    pub no_socialising: u8,
    /// 80: daily gold income in tens (world.md §5: income = byte 80 × 10).
    pub unknown_80: u8,
    /// 81: no interest in buildings.
    pub no_building_interest: u8,
    /// 82: garrison strength (default 50).
    pub garrison_strength: u8,
    /// 83: respawn the whole army, not only the leader.
    pub respawn_all: u8,
    /// 84: spell cast on the army (1-based spell index).
    pub spell: u8,
    /// 85: target model: 0 standard, 1 aggressive, 2 passive, 3 hoarding, 4 trading.
    pub target_model: u8,
    /// 86: always 0 (U).
    pub unknown_86: [u8; 3],
    /// Strings: army name, leader name, description.
    pub name: String,
    pub leader_name: String,
    pub description: String,
}

impl Army {
    fn read(r: &Rec) -> Army {
        Army {
            x: r.u16(0),
            y: r.u16(2),
            id: r.u8(4),
            model: r.u8(5),
            tactical_cost_1: r.u16(6),
            unknown_8: r.u8(8),
            unknown_9: r.arr(9),
            speed_correction: r.i8(13),
            exp_like_player: r.u8(14),
            unknown_15: r.arr(15),
            gold_income: r.u16(17),
            hire_bonus_exp: r.u16(19),
            unknown_21: r.arr(21),
            home_building: r.u8(25),
            leader_unit: r.u8(26),
            leader_level: r.u8(27),
            troops: r.troops(28),
            unknown_46: r.arr(46),
            artifacts: r.arr(50),
            unknown_53: r.arr(53),
            named_character: r.u8(58),
            behaviour: r.u8(59),
            patrols: r.u8(60),
            patrol_radius: r.u8(61),
            no_money: r.u8(62),
            inactive: r.u8(63),
            faction: r.u8(64),
            relations: r.i8s(65),
            aggression: r.i8(69),
            respawn_days: r.u8(70),
            exp_correction: r.u8(71),
            ship: r.u8(72),
            unknown_73: r.u8(73),
            tactical_cost_2: r.u16(74),
            ignored_by_ai: r.u8(76),
            hunts_player_only: r.u8(77),
            no_random_targets: r.u8(78),
            no_socialising: r.u8(79),
            unknown_80: r.u8(80),
            no_building_interest: r.u8(81),
            garrison_strength: r.u8(82),
            respawn_all: r.u8(83),
            spell: r.u8(84),
            target_model: r.u8(85),
            unknown_86: r.arr(86),
            ..Army::default()
        }
    }

    fn write(&self, p: &mut Put) {
        p.u16(0, self.x);
        p.u16(2, self.y);
        p.u8(4, self.id);
        p.u8(5, self.model);
        p.u16(6, self.tactical_cost_1);
        p.u8(8, self.unknown_8);
        p.bytes(9, &self.unknown_9);
        p.i8(13, self.speed_correction);
        p.u8(14, self.exp_like_player);
        p.bytes(15, &self.unknown_15);
        p.u16(17, self.gold_income);
        p.u16(19, self.hire_bonus_exp);
        p.bytes(21, &self.unknown_21);
        p.u8(25, self.home_building);
        p.u8(26, self.leader_unit);
        p.u8(27, self.leader_level);
        p.troops(28, &self.troops);
        p.bytes(46, &self.unknown_46);
        p.bytes(50, &self.artifacts);
        p.bytes(53, &self.unknown_53);
        p.u8(58, self.named_character);
        p.u8(59, self.behaviour);
        p.u8(60, self.patrols);
        p.u8(61, self.patrol_radius);
        p.u8(62, self.no_money);
        p.u8(63, self.inactive);
        p.u8(64, self.faction);
        p.i8s(65, &self.relations);
        p.i8(69, self.aggression);
        p.u8(70, self.respawn_days);
        p.u8(71, self.exp_correction);
        p.u8(72, self.ship);
        p.u8(73, self.unknown_73);
        p.u16(74, self.tactical_cost_2);
        p.u8(76, self.ignored_by_ai);
        p.u8(77, self.hunts_player_only);
        p.u8(78, self.no_random_targets);
        p.u8(79, self.no_socialising);
        p.u8(80, self.unknown_80);
        p.u8(81, self.no_building_interest);
        p.u8(82, self.garrison_strength);
        p.u8(83, self.respawn_all);
        p.u8(84, self.spell);
        p.u8(85, self.target_model);
        p.bytes(86, &self.unknown_86);
    }

    pub fn model(&self) -> Option<ArmyModel> {
        ArmyModel::from_code(self.model)
    }

    /// On the map at start: byte 63 is clear. The loader reads only that byte (0x4b2504),
    /// not the "inactive" model 7 (which goes with it in every shipped map).
    pub fn is_active(&self) -> bool {
        self.inactive == 0
    }

    /// Occupied troop slots.
    pub fn troops(&self) -> impl Iterator<Item = &Troop> {
        self.troops.iter().filter(|t| !t.is_empty())
    }
}

// ------------------------------------------------------------------------------------------
// Points
// ------------------------------------------------------------------------------------------

/// A lantern or event point (99 bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Point {
    /// 0, 2: cell.
    pub x: u16,
    pub y: u16,
    /// 4: point id (1-based); events refer to points by it.
    pub id: u8,
    /// 5: 8 = active lantern, 9 = event point or inactive lantern.
    pub model: u8,
    /// 6: running serial number (L).
    pub serial: u16,
    /// 8: attached local events (the editor allows 5).
    pub event_slots: [u16; 10],
    /// 28: target priorities for green, blue, yellow and red (L; always 0).
    pub priorities: [u16; 4],
    /// 36: active duration (L; always 0).
    pub active_duration: u16,
    /// 38: visibility radius at start (at most 24).
    pub radius: u8,
    /// 39: number of attached events.
    pub event_count: u8,
    /// 40: active at start.
    pub active: u8,
    /// 41: always 0 (U).
    pub unknown_41: [u8; 58],
}

impl Default for Point {
    /// An all-zero record, for building scenarios in code.
    fn default() -> Point {
        Point::read(&Rec(&[0; POINT_SIZE]))
    }
}

impl Point {
    fn read(r: &Rec) -> Point {
        Point {
            x: r.u16(0),
            y: r.u16(2),
            id: r.u8(4),
            model: r.u8(5),
            serial: r.u16(6),
            event_slots: r.u16s(8),
            priorities: r.u16s(28),
            active_duration: r.u16(36),
            radius: r.u8(38),
            event_count: r.u8(39),
            active: r.u8(40),
            unknown_41: r.arr(41),
        }
    }

    fn write(&self, p: &mut Put) {
        p.u16(0, self.x);
        p.u16(2, self.y);
        p.u8(4, self.id);
        p.u8(5, self.model);
        p.u16(6, self.serial);
        p.u16s(8, &self.event_slots);
        p.u16s(28, &self.priorities);
        p.u16(36, self.active_duration);
        p.u8(38, self.radius);
        p.u8(39, self.event_count);
        p.u8(40, self.active);
        p.bytes(41, &self.unknown_41);
    }

    /// Attached event ids.
    pub fn events(&self) -> impl Iterator<Item = u16> + '_ {
        nonzero(&self.event_slots)
    }
}

// ------------------------------------------------------------------------------------------
// Events
// ------------------------------------------------------------------------------------------

/// Event type (byte 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventKind {
    Global = 1,
    Local = 2,
    Quest = 3,
    Rumour = 4,
}

/// Event conditions. A `*_check` byte enables the condition next to it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventConditions {
    /// 11: squad count; the sign encodes ≥ or ≤.
    pub squad_count: i16,
    /// 13: army strength.
    pub army_strength: i16,
    /// 15: army must be inactive.
    pub army_inactive: u8,
    /// 18: check "current stats".
    pub stats_check: u8,
    /// 19: level.
    pub level: i16,
    /// 21: gold.
    pub gold: i16,
    /// 25: holiness / mana.
    pub holiness_mana: i16,
    /// 29..36: building ownership.
    pub buildings_check: u8,
    pub buildings: [u8; 3],
    /// 1 player, 2–5 green, blue, yellow, red, 6 "not the player" (L).
    pub buildings_owner: [u8; 3],
    /// 36..46: named squads in some army.
    pub units_check: u8,
    pub units: [u8; 3],
    pub units_named: [u8; 3],
    pub units_owner: [u8; 3],
    /// 46..53: artifacts.
    pub artifacts_check: u8,
    pub artifacts: [u8; 3],
    pub artifacts_owner: [u8; 3],
    /// 53..56: player defeated armies.
    pub defeated_check: u8,
    pub defeated_armies: [u8; 2],
    /// 56..61: events happened with answer yes.
    pub happened_yes_check: u8,
    pub happened_yes: [u16; 2],
    /// 61..66: events not happened.
    pub not_happened_check: u8,
    pub not_happened: [u16; 2],
    /// 66..69: armies beaten by anyone.
    pub beaten_check: u8,
    pub beaten_armies: [u8; 2],
    /// 69..74: events happened with answer no.
    pub happened_no_check: u8,
    pub happened_no: [u16; 2],
    /// 74: meet army.
    pub meet_army: u8,
    /// 75: army is active.
    pub army_active: u8,
    /// 76: ask a yes/no question.
    pub confirm_question: u8,
    /// 146: army is in its home building.
    pub army_at_home: u8,
}

/// Event results.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventResults {
    /// 16: army whose patrol changes.
    pub patrol_army: u8,
    /// 17: patrol delta (a community opcode selector as well).
    pub patrol_delta: i8,
    /// 77: relative event.
    pub relative_event: u16,
    /// 79: relative event delay in hours.
    pub relative_delay_hours: u16,
    /// 81: spell to activate on the player.
    pub cast_spell: u8,
    /// 82: picture: 200 defeat, 201 victory, otherwise a unit id.
    pub picture: u8,
    /// 83: experience change.
    pub experience: i16,
    /// 85: gold change.
    pub gold: i16,
    /// 89: mana change.
    pub mana: i16,
    /// 93: spells learned.
    pub spells_learned: [u8; 4],
    /// 97: units added.
    pub units_add: [u8; 4],
    /// 101: named characters for the added units.
    pub units_add_named: [u8; 4],
    /// 105: units removed (0xFE "added by an event", 0xFF "any unit").
    pub units_remove: [u8; 4],
    /// 109: named characters for the removed units.
    pub units_remove_named: [u8; 4],
    /// 113: artifacts gained.
    pub artifacts_add: [u8; 4],
    /// 117: artifacts lost.
    pub artifacts_remove: [u8; 4],
    /// 121: armies activated.
    pub activate_armies: [u8; 2],
    /// 123: army deactivated.
    pub deactivate_army: u8,
    /// 124: quest completed (event id).
    pub completes_quest: u16,
    /// 126: delay in hours.
    pub delay_hours: u16,
    /// 128: lanterns lit (point ids).
    pub light_lanterns: [u16; 4],
    /// 136: army that removed units go to.
    pub removed_units_to_army: u8,
    /// 137: new hero class (unit id).
    pub new_hero_class: u8,
    /// 138: chained (subordinate) event, run at once.
    pub chained_event: u16,
    /// 142: army that added units are taken from.
    pub units_from_army: u8,
    /// 143: move that army to the hero.
    pub move_to_hero: u8,
    /// 144: show army.
    pub show_army: u8,
    /// 145: a condition: the hero has exactly 1 HP (the original never sets HP from it).
    pub hero_one_hp: u8,
    /// 147: start a battle with this army.
    pub start_battle_with: u8,
    /// 148: "no meeting with army" (also a community opcode switch).
    pub no_meeting: u8,
    /// 149: repeat after a yes answer.
    pub repeat_after_yes: u8,
}

/// Flag script after `%` in an event title: `[+X | -X][=X | =/X]`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlagScript {
    /// Everything after the first `%`, verbatim.
    pub raw: String,
    /// `+X`: result, sets flag X.
    pub set: Option<String>,
    /// `-X`: result, clears flag X.
    pub clear: Option<String>,
    /// `=X`: condition, X must be set.
    pub require_set: Option<String>,
    /// `=/X`: condition, X must be unset.
    pub require_unset: Option<String>,
}

impl FlagScript {
    /// Parse the script of a title; `None` when the title has no `%`.
    pub fn from_title(title: &str) -> Option<FlagScript> {
        let (_, raw) = title.split_once('%')?;
        let mut f = FlagScript { raw: raw.to_string(), ..FlagScript::default() };
        let (action, test) = match raw.split_once('=') {
            Some((a, t)) => (a, Some(t)),
            None => (raw, None),
        };
        if let Some(x) = action.strip_prefix('+') {
            f.set = Some(x.to_string());
        } else if let Some(x) = action.strip_prefix('-') {
            f.clear = Some(x.to_string());
        }
        if let Some(t) = test {
            match t.strip_prefix('/') {
                Some(x) => f.require_unset = Some(x.to_string()),
                None => f.require_set = Some(t.to_string()),
            }
        }
        Some(f)
    }

    /// The action as the original reads it (0x4ab2a3): the text between `%` and `=` (all of
    /// it without a `=`), sign included.
    pub fn action(&self) -> &str {
        self.raw.split_once('=').map_or(&self.raw, |(a, _)| a)
    }

    /// The test as the original reads it (0x4a8a22): the text after the `=`, empty without
    /// one.
    pub fn test(&self) -> &str {
        self.raw.split_once('=').map_or("", |(_, t)| t)
    }
}

/// A map string as the original keeps it after loading (0x4b2aa1): every run of two spaces
/// collapsed to one.
pub fn collapse_spaces(s: &str) -> String {
    let mut out = s.to_string();
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out
}

/// A scenario event (171 bytes). Events have 1-based ids in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Event {
    /// 0: group colour in the editor (0–5).
    pub group_colour: u8,
    /// 1: raw type, see [`Event::kind`].
    pub kind: u8,
    /// 2: start time (minutes).
    pub start_time: u32,
    /// 6: repeat period (minutes).
    pub repeat: u16,
    /// 8: active duration (minutes).
    pub duration: u16,
    /// 10: hero archetype: 0 all, 1 knight, 2 archmage, 3 ranger.
    pub archetype: u8,
    pub conditions: EventConditions,
    pub results: EventResults,
    /// 140: "subordinate event" flag.
    pub subordinate: u8,
    /// 141: 0 = may fire many times, 1 = once.
    pub once: u8,
    /// 23, 27, 87, 91: small or rare values (U).
    pub unknown_23: [u8; 2],
    pub unknown_27: [u8; 2],
    pub unknown_87: [u8; 2],
    pub unknown_91: [u8; 2],
    /// 150: "generate the battle army to match the player" (the original editor's check box
    /// next to "start a battle with army"; always 0 in the shipped maps; the game's use of it
    /// is not traced).
    pub generate_battle_army: u8,
    /// 151: U (byte 156 is a 0/1 flag in about 2% of events).
    pub unknown_151: [u8; 12],
    /// 165: U.
    pub unknown_165: [u8; 6],
    /// Strings: title (with an optional flag script), yes/no question, message.
    pub title: String,
    pub question: String,
    pub message: String,
    /// Parsed from `title`.
    pub flags: Option<FlagScript>,
    /// Custom picture (size: the i32 at byte 163): u16 width, u16 height, 16-bit pixels
    /// (L: RGB565).
    pub custom_picture: Option<Vec<u8>>,
}

impl Event {
    /// Returns the event and its custom picture size: a full 32-bit value at byte 163, read
    /// when positive (0x4b2bbd).
    fn read(r: &Rec) -> (Event, i32) {
        let conditions = EventConditions {
            squad_count: r.i16(11),
            army_strength: r.i16(13),
            army_inactive: r.u8(15),
            stats_check: r.u8(18),
            level: r.i16(19),
            gold: r.i16(21),
            holiness_mana: r.i16(25),
            buildings_check: r.u8(29),
            buildings: r.arr(30),
            buildings_owner: r.arr(33),
            units_check: r.u8(36),
            units: r.arr(37),
            units_named: r.arr(40),
            units_owner: r.arr(43),
            artifacts_check: r.u8(46),
            artifacts: r.arr(47),
            artifacts_owner: r.arr(50),
            defeated_check: r.u8(53),
            defeated_armies: r.arr(54),
            happened_yes_check: r.u8(56),
            happened_yes: r.u16s(57),
            not_happened_check: r.u8(61),
            not_happened: r.u16s(62),
            beaten_check: r.u8(66),
            beaten_armies: r.arr(67),
            happened_no_check: r.u8(69),
            happened_no: r.u16s(70),
            meet_army: r.u8(74),
            army_active: r.u8(75),
            confirm_question: r.u8(76),
            army_at_home: r.u8(146),
        };
        let results = EventResults {
            patrol_army: r.u8(16),
            patrol_delta: r.i8(17),
            relative_event: r.u16(77),
            relative_delay_hours: r.u16(79),
            cast_spell: r.u8(81),
            picture: r.u8(82),
            experience: r.i16(83),
            gold: r.i16(85),
            mana: r.i16(89),
            spells_learned: r.arr(93),
            units_add: r.arr(97),
            units_add_named: r.arr(101),
            units_remove: r.arr(105),
            units_remove_named: r.arr(109),
            artifacts_add: r.arr(113),
            artifacts_remove: r.arr(117),
            activate_armies: r.arr(121),
            deactivate_army: r.u8(123),
            completes_quest: r.u16(124),
            delay_hours: r.u16(126),
            light_lanterns: r.u16s(128),
            removed_units_to_army: r.u8(136),
            new_hero_class: r.u8(137),
            chained_event: r.u16(138),
            units_from_army: r.u8(142),
            move_to_hero: r.u8(143),
            show_army: r.u8(144),
            hero_one_hp: r.u8(145),
            start_battle_with: r.u8(147),
            no_meeting: r.u8(148),
            repeat_after_yes: r.u8(149),
        };
        let e = Event {
            group_colour: r.u8(0),
            kind: r.u8(1),
            start_time: r.u32(2),
            repeat: r.u16(6),
            duration: r.u16(8),
            archetype: r.u8(10),
            conditions,
            results,
            subordinate: r.u8(140),
            once: r.u8(141),
            unknown_23: r.arr(23),
            unknown_27: r.arr(27),
            unknown_87: r.arr(87),
            unknown_91: r.arr(91),
            generate_battle_army: r.u8(150),
            unknown_151: r.arr(151),
            unknown_165: r.arr(165),
            ..Event::default()
        };
        (e, r.u32(163) as i32)
    }

    fn write(&self, p: &mut Put) {
        let c = &self.conditions;
        let s = &self.results;
        p.u8(0, self.group_colour);
        p.u8(1, self.kind);
        p.u32(2, self.start_time);
        p.u16(6, self.repeat);
        p.u16(8, self.duration);
        p.u8(10, self.archetype);
        p.i16(11, c.squad_count);
        p.i16(13, c.army_strength);
        p.u8(15, c.army_inactive);
        p.u8(16, s.patrol_army);
        p.i8(17, s.patrol_delta);
        p.u8(18, c.stats_check);
        p.i16(19, c.level);
        p.i16(21, c.gold);
        p.bytes(23, &self.unknown_23);
        p.i16(25, c.holiness_mana);
        p.bytes(27, &self.unknown_27);
        p.u8(29, c.buildings_check);
        p.bytes(30, &c.buildings);
        p.bytes(33, &c.buildings_owner);
        p.u8(36, c.units_check);
        p.bytes(37, &c.units);
        p.bytes(40, &c.units_named);
        p.bytes(43, &c.units_owner);
        p.u8(46, c.artifacts_check);
        p.bytes(47, &c.artifacts);
        p.bytes(50, &c.artifacts_owner);
        p.u8(53, c.defeated_check);
        p.bytes(54, &c.defeated_armies);
        p.u8(56, c.happened_yes_check);
        p.u16s(57, &c.happened_yes);
        p.u8(61, c.not_happened_check);
        p.u16s(62, &c.not_happened);
        p.u8(66, c.beaten_check);
        p.bytes(67, &c.beaten_armies);
        p.u8(69, c.happened_no_check);
        p.u16s(70, &c.happened_no);
        p.u8(74, c.meet_army);
        p.u8(75, c.army_active);
        p.u8(76, c.confirm_question);
        p.u16(77, s.relative_event);
        p.u16(79, s.relative_delay_hours);
        p.u8(81, s.cast_spell);
        p.u8(82, s.picture);
        p.i16(83, s.experience);
        p.i16(85, s.gold);
        p.bytes(87, &self.unknown_87);
        p.i16(89, s.mana);
        p.bytes(91, &self.unknown_91);
        p.bytes(93, &s.spells_learned);
        p.bytes(97, &s.units_add);
        p.bytes(101, &s.units_add_named);
        p.bytes(105, &s.units_remove);
        p.bytes(109, &s.units_remove_named);
        p.bytes(113, &s.artifacts_add);
        p.bytes(117, &s.artifacts_remove);
        p.bytes(121, &s.activate_armies);
        p.u8(123, s.deactivate_army);
        p.u16(124, s.completes_quest);
        p.u16(126, s.delay_hours);
        p.u16s(128, &s.light_lanterns);
        p.u8(136, s.removed_units_to_army);
        p.u8(137, s.new_hero_class);
        p.u16(138, s.chained_event);
        p.u8(140, self.subordinate);
        p.u8(141, self.once);
        p.u8(142, s.units_from_army);
        p.u8(143, s.move_to_hero);
        p.u8(144, s.show_army);
        p.u8(145, s.hero_one_hp);
        p.u8(146, c.army_at_home);
        p.u8(147, s.start_battle_with);
        p.u8(148, s.no_meeting);
        p.u8(149, s.repeat_after_yes);
        p.u8(150, self.generate_battle_army);
        p.bytes(151, &self.unknown_151);
        p.u16(163, self.custom_picture.as_ref().map_or(0, |v| v.len() as u16));
        p.bytes(165, &self.unknown_165);
    }

    pub fn kind(&self) -> Option<EventKind> {
        match self.kind {
            1 => Some(EventKind::Global),
            2 => Some(EventKind::Local),
            3 => Some(EventKind::Quest),
            4 => Some(EventKind::Rumour),
            _ => None,
        }
    }

    /// The title without its flag script.
    pub fn title_text(&self) -> &str {
        self.title.split_once('%').map_or(&self.title, |(t, _)| t)
    }

    /// The title the game shows (0x4833f0): before the first `%`, then before the first `#`
    /// (an editor's note), trailing spaces removed.
    pub fn display_title(&self) -> &str {
        let t = self.title_text();
        t.split_once('#').map_or(t, |(t, _)| t).trim_end_matches(' ')
    }

    /// The engine's state the record carries from the file (bytes 156–162): last fired
    /// (minutes), times fired, and the answer (1 = No).
    pub fn runtime_state(&self) -> (i32, u16, u8) {
        let b = &self.unknown_151;
        (i32::from_le_bytes([b[5], b[6], b[7], b[8]]), u16::from_le_bytes([b[9], b[10]]), b[11])
    }

    /// The event as the game loads it: its texts with double spaces collapsed (0x4b2aa1)
    /// and its flag script read again from the collapsed title.
    pub fn for_play(&self) -> Event {
        let mut e = self.clone();
        e.title = collapse_spaces(&e.title);
        e.question = collapse_spaces(&e.question);
        e.message = collapse_spaces(&e.message);
        e.flags = FlagScript::from_title(&e.title);
        e
    }

    pub fn fires_once(&self) -> bool {
        self.once != 0
    }

    pub fn start_date(&self) -> GameDate {
        GameDate::from_minutes(self.start_time)
    }
}

// ------------------------------------------------------------------------------------------
// Scenario
// ------------------------------------------------------------------------------------------

/// A named character (именной персонаж): its class and name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedCharacter {
    pub unit: u8,
    pub name: String,
}

/// A whole `.DTm` scenario.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scenario {
    pub header: Header,
    /// `width*height` terrain codes, index `y*width + x`, row 0 at the top (see [`Surface`]).
    pub terrain: Vec<u8>,
    pub objects: Vec<MapObject>,
    pub buildings: Vec<Building>,
    pub armies: Vec<Army>,
    pub points: Vec<Point>,
    pub events: Vec<Event>,
    pub title: String,
    pub description: String,
    pub campaign_name: String,
    /// Next scenario file name (`*.DTm`), empty if none.
    pub next_map: String,
    pub named_characters: Vec<NamedCharacter>,
    /// Embedded scenario picture (a LIT image), raw.
    pub scenario_picture: Option<Vec<u8>>,
}

/// Sequential reader over the payload.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], DtError> {
        let end = self.pos.checked_add(n).filter(|e| *e <= self.data.len());
        let end = end.ok_or(DtError::Truncated { what, offset: self.pos })?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    /// A section of `size` bytes holding `size div record` records (0x4b2504); the bytes
    /// left over are skipped.
    fn records(&mut self, size: u32, record: usize, section: &'static str) -> Result<Vec<Rec<'a>>, DtError> {
        Ok(self.take(size as usize, section)?.chunks_exact(record).map(Rec).collect())
    }

    /// A string up to a NUL or the end of the data (0x473b04).
    fn cstr(&mut self) -> String {
        let rest = self.data.get(self.pos..).unwrap_or_default();
        let n = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
        let s = text::decode(&rest[..n]);
        self.pos += (n + 1).min(rest.len());
        s
    }
}

/// Size of one record of the custom-artefact section (header 0x34).
pub const CUSTOM_ARTEFACT_SIZE: usize = 230;

/// A map-specific artefact of the Community editor's custom-artefact section: the raw
/// 230-byte artefact record and its two strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomArtefact {
    pub record: Vec<u8>,
    pub name: String,
    pub description: String,
}

/// What the Community editor's loader reads ([`Scenario::parse_editor_payload`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorRead {
    pub scenario: Scenario,
    /// Header byte 9, the version digit (`b'4'` in current maps).
    pub version: u8,
    /// The first 12 bytes are exactly the version-4 signature.
    pub signature_is_current: bool,
    pub custom_artefacts: Vec<CustomArtefact>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Terrain,
    Objects,
    Buildings,
    Armies,
    Points,
    Events,
    CustomArtefacts,
}

/// The order of the binary sections in a payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionOrder {
    /// File order, the custom-artefact section after the events.
    File,
    /// The Community editor's demo maps (header 0x117 = 1): objects, custom artefacts,
    /// points, terrain, events, armies, buildings (DTMapEdit 0x5a6fe0, 0x5a50f2).
    Demo,
}

impl SectionOrder {
    fn sections(self) -> [Section; 7] {
        use Section::*;
        match self {
            SectionOrder::File => [Terrain, Objects, Buildings, Armies, Points, Events, CustomArtefacts],
            SectionOrder::Demo => [Objects, CustomArtefacts, Points, Terrain, Events, Armies, Buildings],
        }
    }
}

/// The binary sections as read, before they become records (old versions are upgraded on
/// the raw bytes, whose layout they change).
#[derive(Default)]
struct RawSections<'a> {
    terrain: &'a [u8],
    objects: Vec<Rec<'a>>,
    buildings: Vec<Vec<u8>>,
    armies: Vec<Vec<u8>>,
    points: Vec<Vec<u8>>,
    events: Vec<Rec<'a>>,
    custom: Vec<Vec<u8>>,
    /// The custom-artefact section was read (the editor's loader); the game skips it and
    /// its strings.
    custom_read: bool,
}

impl<'a> RawSections<'a> {
    fn read(&mut self, c: &mut Cursor<'a>, data: &[u8], section: Section) -> Result<(), DtError> {
        let size = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().expect("in the header"));
        let owned = |v: Vec<Rec>| v.into_iter().map(|r| r.0.to_vec()).collect::<Vec<_>>();
        match section {
            Section::Terrain => self.terrain = c.take(size(0x1C) as usize, "terrain")?,
            Section::Objects => self.objects = c.records(size(0x20), OBJECT_SIZE, "objects")?,
            Section::Buildings => self.buildings = owned(c.records(size(0x24), BUILDING_SIZE, "buildings")?),
            Section::Armies => self.armies = owned(c.records(size(0x28), ARMY_SIZE, "armies")?),
            Section::Points => self.points = owned(c.records(size(0x2C), POINT_SIZE, "points")?),
            Section::Events => self.events = c.records(size(0x30), EVENT_SIZE, "events")?,
            Section::CustomArtefacts => {
                self.custom = owned(c.records(size(0x34), CUSTOM_ARTEFACT_SIZE, "custom artefacts")?);
                self.custom_read = true;
            }
        }
        Ok(())
    }

    /// The Community editor's upgrade of an old map by its version digit (0x5a8424); each
    /// digit gets only its own step, so a version-1 map is not given the steps of 2 and 3.
    fn upgrade(&mut self, version: u8) {
        match version {
            b'1' => {
                // Event lists were 64 (buildings) and 5 (points) four-byte ids: their low
                // words become the two-byte ids. A building's goods were 6 signed bytes at
                // 301; the 128 bytes at 136 are cleared and the six slots filled from them.
                for b in &mut self.buildings {
                    for k in 0..64 {
                        let low = [b[8 + 4 * k], b[9 + 4 * k]];
                        b[8 + 2 * k..10 + 2 * k].copy_from_slice(&low);
                    }
                    b[136..264].fill(0);
                    for k in 0..6 {
                        let v = b[301 + k] as i8 as i16;
                        b[136 + 2 * k..138 + 2 * k].copy_from_slice(&v.to_le_bytes());
                    }
                }
                for p in &mut self.points {
                    for k in 0..5 {
                        let low = [p[8 + 4 * k], p[9 + 4 * k]];
                        p[8 + 2 * k..10 + 2 * k].copy_from_slice(&low);
                    }
                }
            }
            b'2' => {
                // Point model 6 became 8; an army's id and map model (bytes 4 and 5) are
                // set: its style + 4 when active, 7 when inactive.
                for p in &mut self.points {
                    if p[5] == 6 {
                        p[5] = 8;
                    }
                }
                for (i, a) in self.armies.iter_mut().enumerate() {
                    a[5] = if a[63] == 0 { a[59].wrapping_add(4) } else { 7 };
                    a[4] = (i + 1) as u8;
                }
            }
            b'3' => {
                // The garrison strength: 0 becomes 50, and any other value 0 (the original's
                // code; why is not known).
                for a in &mut self.armies {
                    a[82] = if a[82] == 0 { 50 } else { 0 };
                }
            }
            _ => {}
        }
    }

    /// The records, then the strings from the header's text offset (the game seeks there,
    /// 0x4b27a8, and never reads the text marker), then the pictures.
    fn into_scenario(self, c: &mut Cursor, data: &[u8], header: Header, named_count: u8) -> Result<EditorRead, DtError> {
        let h = Rec(&data[..HEADER_SIZE]);
        let terrain = expand_terrain(self.terrain, header.width, header.height)?;
        let objects = self.objects.iter().map(|r| MapObject { x: r.u16(0), y: r.u16(2), sprite: r.u8(4), class: r.u8(5) }).collect();
        let mut buildings: Vec<Building> = self.buildings.iter().map(|b| Building::read(&Rec(b))).collect();
        let mut armies: Vec<Army> = self.armies.iter().map(|a| Army::read(&Rec(a))).collect();
        let points = self.points.iter().map(|p| Point::read(&Rec(p))).collect();
        let (mut events, picture_sizes): (Vec<Event>, Vec<i32>) = self.events.iter().map(Event::read).unzip();
        c.pos = h.u32(0x18) as usize;

        let title = c.cstr();
        let description = c.cstr();
        let campaign_name = c.cstr();
        let next_map = c.cstr();
        for b in &mut buildings {
            (b.name, b.owner_name, b.description) = (c.cstr(), c.cstr(), c.cstr());
        }
        for a in &mut armies {
            (a.name, a.leader_name, a.description) = (c.cstr(), c.cstr(), c.cstr());
        }
        for e in &mut events {
            (e.title, e.question, e.message) = (c.cstr(), c.cstr(), c.cstr());
            e.flags = FlagScript::from_title(&e.title);
        }
        let mut custom_artefacts = Vec::new();
        if self.custom_read {
            for record in self.custom {
                let (name, description) = (c.cstr(), c.cstr());
                custom_artefacts.push(CustomArtefact { record, name, description });
            }
        }
        let mut named_characters = Vec::with_capacity(named_count as usize);
        for &unit in &header.named_character_slots[..named_count as usize] {
            named_characters.push(NamedCharacter { unit, name: c.cstr() });
        }

        let scenario_picture = match h.u32(0x11C) {
            0 => None,
            n => Some(c.take(n as usize, "scenario picture")?.to_vec()),
        };
        for (e, n) in events.iter_mut().zip(picture_sizes) {
            if n > 0 {
                e.custom_picture = Some(c.take(n as usize, "event picture")?.to_vec());
            }
        }
        let scenario = Scenario {
            header,
            terrain,
            objects,
            buildings,
            armies,
            points,
            events,
            title,
            description,
            campaign_name,
            next_map,
            named_characters,
            scenario_picture,
        };
        Ok(EditorRead { scenario, version: data[9], signature_is_current: true, custom_artefacts })
    }
}

/// The header fields and the named-character count (checked against the 32 slots).
fn read_header(data: &[u8]) -> Result<(Header, u8), DtError> {
    let h = Rec(&data[..HEADER_SIZE]);
    let named_count = h.u8(0xEE);
    let header = Header {
        width: h.u32(0x0C),
        height: h.u32(0x10),
        generator_seed: h.u32(0x14),
        unknown_0x34: h.u32(0x34),
        start_time: h.u32(0x38),
        heroes: std::array::from_fn(|k| {
            let o = 0x3C + HERO_PRESET_SIZE * k;
            HeroPreset::read(&Rec(&data[o..o + HERO_PRESET_SIZE]))
        }),
        victory_event: h.u16(0xD2),
        unknown_0xd4: h.arr(0xD4),
        defeat_event: h.u16(0xD8),
        unknown_0xda: h.arr(0xDA),
        relations: std::array::from_fn(|k| h.i8s(0xDE + 4 * k)),
        named_character_slots: h.arr(0xEF),
        scenario_kind: h.u8(0x10F),
        carry_over: h.arr(0x110),
        unknown_0x117: h.arr(0x117),
        scenario_picture_index: h.u8(0x120),
        unknown_0x121: h.arr(0x121),
    };
    if named_count as usize > header.named_character_slots.len() {
        return Err(DtError::BadValue { section: "header".into(), key: "named character count".into(), value: named_count.to_string() });
    }
    Ok((header, named_count))
}

impl Scenario {
    /// Read a `.DTm` file.
    pub fn load(path: &Path) -> Result<Scenario, DtError> {
        let bytes = std::fs::read(path).map_err(|source| DtError::Io { path: path.to_path_buf(), source })?;
        Scenario::from_file_bytes(&bytes)
    }

    /// Parse file contents: a container ([`container::has_magic`]), else the bytes as they
    /// are, as the original's stream reads a file without the magic.
    pub fn from_file_bytes(bytes: &[u8]) -> Result<Scenario, DtError> {
        if container::has_magic(bytes) {
            return Scenario::parse_payload(&container::decode(bytes)?.payload);
        }
        Scenario::parse_payload(bytes)
    }

    /// Parse an uncompressed payload as the map loader reads it (0x4b2504, saves-data.md
    /// §10.1): only header byte 9 is checked (below `'4'` nothing is loaded); the sections
    /// are read in order by the header's sizes, each holding `size div record` records; then
    /// the reader **seeks to the header's text offset** (the text marker is never read) and
    /// takes the strings, each up to a NUL or the end; the scenario picture and the event
    /// pictures with a positive 32-bit size follow. Nothing after them is looked at.
    pub fn parse_payload(data: &[u8]) -> Result<Scenario, DtError> {
        if data.len() < HEADER_SIZE {
            if data.get(9).is_some_and(|&v| v < b'4') {
                return Err(DtError::BadMagic { what: "DTm payload" });
            }
            return Err(DtError::Truncated { what: "header", offset: data.len() });
        }
        if data[9] < b'4' {
            return Err(DtError::BadMagic { what: "DTm payload" });
        }
        let (header, named_count) = read_header(data)?;
        let mut c = Cursor { data, pos: HEADER_SIZE };
        let mut raw = RawSections::default();
        // The game reads neither the custom-artefact section nor its strings.
        for section in &SectionOrder::File.sections()[..6] {
            raw.read(&mut c, data, *section)?;
        }
        let read = raw.into_scenario(&mut c, data, header, named_count)?;
        Ok(read.scenario)
    }

    /// Parse an uncompressed payload as the Community editor's loader reads it (DTMapEdit
    /// 0x5a6c20, docs/reference/editor/mapcheck-files.md §3): the first nine bytes must be
    /// the signature up to the version digit and both sides below 801; the demo flag (header
    /// 0x117) picks the section order; the custom-artefact section (header 0x34) and its two
    /// strings per artefact are read; maps of versions 1 to 3 are upgraded (§3.7). The
    /// strings come as stored: the editor's trimming is the caller's step. A demo flag other
    /// than 0 or 1 makes the original read no section at all and its strings from the wrong
    /// place; that is refused here.
    pub fn parse_editor_payload(data: &[u8]) -> Result<EditorRead, DtError> {
        if data.len() < HEADER_SIZE {
            return Err(DtError::Truncated { what: "header", offset: data.len() });
        }
        let h = Rec(&data[..HEADER_SIZE]);
        if &data[..9] != b"MapLDV V." || h.u32(0x0C) >= 801 || h.u32(0x10) >= 801 {
            return Err(DtError::BadMagic { what: "DTm payload" });
        }
        let order = match data[0x117] {
            0 => SectionOrder::File,
            1 => SectionOrder::Demo,
            v => return Err(DtError::BadValue { section: "header".into(), key: "demo flag".into(), value: v.to_string() }),
        };
        let version = data[9];
        let (header, named_count) = read_header(data)?;
        let mut c = Cursor { data, pos: HEADER_SIZE };
        let mut raw = RawSections::default();
        for section in order.sections() {
            raw.read(&mut c, data, section)?;
        }
        raw.upgrade(version);
        let mut read = raw.into_scenario(&mut c, data, header, named_count)?;
        read.version = version;
        read.signature_is_current = &data[..12] == PAYLOAD_MAGIC;
        Ok(read)
    }

    /// Serialise back to an uncompressed payload. For a parsed map this reproduces the
    /// original bytes (the terrain is recompressed with greedy runs).
    pub fn to_payload(&self) -> Vec<u8> {
        self.to_payload_in(SectionOrder::File)
    }

    /// Serialise with the sections in `order` (the custom-artefact section is written
    /// empty). The header and the strings are the same in either order.
    pub fn to_payload_in(&self, order: SectionOrder) -> Vec<u8> {
        fn records<T>(items: &[T], size: usize, write: impl Fn(&T, &mut Put)) -> Vec<u8> {
            let mut out = vec![0u8; items.len() * size];
            for (item, chunk) in items.iter().zip(out.chunks_exact_mut(size)) {
                write(item, &mut Put(chunk));
            }
            out
        }
        let hd = &self.header;
        let terrain = compress_terrain(&self.terrain);
        let objects = records(&self.objects, OBJECT_SIZE, |o, p| {
            p.u16(0, o.x);
            p.u16(2, o.y);
            p.u8(4, o.sprite);
            p.u8(5, o.class);
        });
        let buildings = records(&self.buildings, BUILDING_SIZE, Building::write);
        let armies = records(&self.armies, ARMY_SIZE, Army::write);
        let points = records(&self.points, POINT_SIZE, Point::write);
        let events = records(&self.events, EVENT_SIZE, Event::write);
        let sections = [&terrain, &objects, &buildings, &armies, &points, &events];

        let mut header = vec![0u8; HEADER_SIZE];
        let mut p = Put(&mut header);
        p.bytes(0, PAYLOAD_MAGIC);
        p.u32(0x0C, hd.width);
        p.u32(0x10, hd.height);
        p.u32(0x14, hd.generator_seed);
        let text_at = HEADER_SIZE + sections.iter().map(|s| s.len()).sum::<usize>() + TEXT_MARKER.len();
        p.u32(0x18, text_at as u32);
        for (k, s) in sections.iter().enumerate() {
            p.u32(0x1C + 4 * k, s.len() as u32);
        }
        p.u32(0x34, hd.unknown_0x34);
        p.u32(0x38, hd.start_time);
        for (k, hero) in hd.heroes.iter().enumerate() {
            let o = 0x3C + HERO_PRESET_SIZE * k;
            hero.write(&mut Put(&mut p.0[o..o + HERO_PRESET_SIZE]));
        }
        p.u16(0xD2, hd.victory_event);
        p.bytes(0xD4, &hd.unknown_0xd4);
        p.u16(0xD8, hd.defeat_event);
        p.bytes(0xDA, &hd.unknown_0xda);
        for (k, row) in hd.relations.iter().enumerate() {
            p.i8s(0xDE + 4 * k, row);
        }
        let mut slots = hd.named_character_slots;
        for (slot, nc) in slots.iter_mut().zip(&self.named_characters) {
            *slot = nc.unit;
        }
        p.u8(0xEE, self.named_characters.len() as u8);
        p.bytes(0xEF, &slots);
        p.u8(0x10F, hd.scenario_kind);
        p.bytes(0x110, &hd.carry_over);
        p.bytes(0x117, &hd.unknown_0x117);
        p.u32(0x11C, self.scenario_picture.as_ref().map_or(0, |v| v.len() as u32));
        p.u8(0x120, hd.scenario_picture_index);
        p.bytes(0x121, &hd.unknown_0x121);

        let mut out = header;
        for section in order.sections() {
            let bytes: &[u8] = match section {
                Section::Terrain => &terrain,
                Section::Objects => &objects,
                Section::Buildings => &buildings,
                Section::Armies => &armies,
                Section::Points => &points,
                Section::Events => &events,
                Section::CustomArtefacts => &[],
            };
            out.extend_from_slice(bytes);
        }
        out.extend_from_slice(TEXT_MARKER);
        let mut put_str = |s: &str| {
            out.extend(text::encode(s));
            out.push(0);
        };
        for s in [&self.title, &self.description, &self.campaign_name, &self.next_map] {
            put_str(s);
        }
        for b in &self.buildings {
            for s in [&b.name, &b.owner_name, &b.description] {
                put_str(s);
            }
        }
        for a in &self.armies {
            for s in [&a.name, &a.leader_name, &a.description] {
                put_str(s);
            }
        }
        for e in &self.events {
            for s in [&e.title, &e.question, &e.message] {
                put_str(s);
            }
        }
        for nc in &self.named_characters {
            put_str(&nc.name);
        }
        if let Some(pic) = &self.scenario_picture {
            out.extend_from_slice(pic);
        }
        for e in &self.events {
            if let Some(pic) = &e.custom_picture {
                out.extend_from_slice(pic);
            }
        }
        out
    }

    pub fn width(&self) -> u32 {
        self.header.width
    }

    pub fn height(&self) -> u32 {
        self.header.height
    }

    /// Terrain code at a cell, `None` outside the map.
    pub fn terrain_at(&self, x: u32, y: u32) -> Option<u8> {
        (x < self.width() && y < self.height()).then(|| self.terrain[(y * self.width() + x) as usize])
    }

    /// Building by 1-based id.
    pub fn building(&self, id: u16) -> Option<&Building> {
        (id as usize).checked_sub(1).and_then(|i| self.buildings.get(i))
    }

    /// Army by 1-based id.
    pub fn army(&self, id: u8) -> Option<&Army> {
        (id as usize).checked_sub(1).and_then(|i| self.armies.get(i))
    }

    /// Event by 1-based id.
    pub fn event(&self, id: u16) -> Option<&Event> {
        (id as usize).checked_sub(1).and_then(|i| self.events.get(i))
    }

    /// The victory event, if the scenario sets one.
    pub fn victory_event(&self) -> Option<&Event> {
        self.event(self.header.victory_event)
    }

    /// The defeat event, if the scenario sets one.
    pub fn defeat_event(&self) -> Option<&Event> {
        self.event(self.header.defeat_event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w16(b: &mut [u8], o: usize, v: u16) {
        b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn w32(b: &mut [u8], o: usize, v: u32) {
        b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// A 4x2 map built byte by byte: 1 object, building, army, point and event, all
    /// strings, one named character, a 5-byte scenario picture and a 6-byte event picture.
    fn sample_payload() -> Vec<u8> {
        let terrain = [6u8, 4, 4, 0, 1, 1]; // 5 × grass, 1 × road, 2 × coastal water
        let object = [2u8, 0, 1, 0, 42, 9];
        let mut building = [0u8; BUILDING_SIZE];
        w16(&mut building, 0, 3);
        w16(&mut building, 2, 1);
        building[6] = 3; // castle
        w16(&mut building, 8, 1); // event 1
        w16(&mut building, 10, 0); // deleted slot
        w16(&mut building, 12, 7); // stale beyond event_count
        w16(&mut building, 136, 146);
        building[264..267].copy_from_slice(&[4, 3, 9]);
        w16(&mut building, 282, 55);
        building[288] = 2;
        building[292] = 0xFF;
        building[314..317].copy_from_slice(&[9, 1, 2]);
        building[338] = (-2i8) as u8;
        building[353] = 1;
        let mut army = [0u8; ARMY_SIZE];
        w16(&mut army, 0, 1);
        w16(&mut army, 2, 1);
        army[4] = 1;
        army[5] = 5; // bandits
        army[13] = (-3i8) as u8;
        army[28..31].copy_from_slice(&[42, 0, 2]);
        army[69] = (-20i8) as u8;
        army[80] = 44; // unknown byte, must survive
        let mut point = [0u8; POINT_SIZE];
        point[4] = 1;
        point[5] = 8;
        point[38] = 24;
        let mut event = [0u8; EVENT_SIZE];
        event[1] = 3; // quest
        w32(&mut event, 2, 624_354_300);
        w16(&mut event, 6, 1440);
        w16(&mut event, 21, (-7499i16) as u16);
        w16(&mut event, 85, (-50i16) as u16);
        w16(&mut event, 128, 1);
        event[141] = 1;
        event[146] = 1;
        event[156] = 1; // unknown byte
        w16(&mut event, 163, 6);

        let sections: [&[u8]; 6] = [&terrain, &object, &building, &army, &point, &event];
        let mut h = vec![0u8; HEADER_SIZE];
        h[..12].copy_from_slice(PAYLOAD_MAGIC);
        w32(&mut h, 0x0C, 4);
        w32(&mut h, 0x10, 2);
        let text_at = HEADER_SIZE + sections.iter().map(|s| s.len()).sum::<usize>() + 8;
        w32(&mut h, 0x18, text_at as u32);
        for (k, s) in sections.iter().enumerate() {
            w32(&mut h, 0x1C + 4 * k, s.len() as u32);
        }
        w32(&mut h, 0x38, 624_354_300);
        w32(&mut h, 0x3C + 50 + 8, 500); // archmage gold
        w32(&mut h, 0x3C + 50 + 12, 300); // archmage mana
        w16(&mut h, 0x3C + 50 + 37, 2); // archmage x
        h[0x3C + 50 + 19..0x3C + 50 + 22].copy_from_slice(&[4, 0, 3]);
        w16(&mut h, 0xD2, 1);
        h[0xDE + 3] = (-2i8) as u8;
        h[0xEE] = 1;
        h[0xEF] = 74;
        h[0xF0] = 99; // stale slot
        h[0x10F] = 2;
        h[0x110] = 1;
        w32(&mut h, 0x11C, 5);

        let mut out = h;
        for s in sections {
            out.extend_from_slice(s);
        }
        out.extend_from_slice(TEXT_MARKER);
        for s in ["Title", "Desc", "", "next.DTm", "Castle", "Owner", "", "Gang", "Boss", "", "Quest%+Foo=/Bar", "Q?", "Msg", "Hero"] {
            out.extend_from_slice(s.as_bytes());
            out.push(0);
        }
        out.extend_from_slice(b"LIT\0!");
        out.extend_from_slice(&[1, 0, 1, 0, 0xAB, 0xCD]);
        out
    }

    #[test]
    fn the_editor_reads_what_the_game_reads() {
        let p = sample_payload();
        let r = Scenario::parse_editor_payload(&p).unwrap();
        assert_eq!(r.scenario, Scenario::parse_payload(&p).unwrap());
        assert!(r.signature_is_current && r.custom_artefacts.is_empty());
        assert_eq!(r.version, b'4');
    }

    #[test]
    fn the_editor_checks_the_signature_and_the_size() {
        let mut p = sample_payload();
        p[8] = b'X';
        assert!(Scenario::parse_editor_payload(&p).is_err(), "the dot before the version digit is checked");
        let mut p = sample_payload();
        p[10] = b'?';
        let r = Scenario::parse_editor_payload(&p).unwrap();
        assert!(!r.signature_is_current, "only the first nine bytes must match; the rest marks the map modified");
        let mut p = sample_payload();
        w32(&mut p, 0x10, 801);
        assert!(Scenario::parse_editor_payload(&p).is_err());
        let mut p = sample_payload();
        p[0x117] = 2;
        assert!(Scenario::parse_editor_payload(&p).is_err(), "a demo flag of 2 makes the original read no section");
    }

    #[test]
    fn demo_maps_store_the_sections_in_another_order() {
        let s = Scenario::parse_payload(&sample_payload()).unwrap();
        let mut demo = s.clone();
        demo.header.set_demo_flag(1);
        let bytes = demo.to_payload_in(SectionOrder::Demo);
        // Objects first, right after the header.
        assert_eq!(&bytes[HEADER_SIZE..HEADER_SIZE + OBJECT_SIZE], &[2, 0, 1, 0, 42, 9]);
        assert_eq!(bytes.len(), s.to_payload().len());
        let back = Scenario::parse_editor_payload(&bytes).unwrap().scenario;
        assert_eq!(back, demo);
        // The game reads them in file order and gets other records.
        assert_ne!(Scenario::parse_payload(&bytes).map(|g| g.objects).ok(), Some(s.objects.clone()));
    }

    #[test]
    fn the_editor_reads_custom_artefacts_and_their_strings() {
        let s = Scenario::parse_payload(&sample_payload()).unwrap();
        let mut p = s.to_payload();
        // Insert one 230-byte record after the events and its two strings before the named
        // character's name.
        let events_end = p.len() - (p.len() - u32::from_le_bytes(p[0x18..0x1C].try_into().unwrap()) as usize) - TEXT_MARKER.len();
        let mut record = vec![0u8; CUSTOM_ARTEFACT_SIZE];
        record[0x20] = 77;
        p.splice(events_end..events_end, record.iter().copied());
        w32(&mut p, 0x34, CUSTOM_ARTEFACT_SIZE as u32);
        let text_at = u32::from_le_bytes(p[0x18..0x1C].try_into().unwrap()) + CUSTOM_ARTEFACT_SIZE as u32;
        w32(&mut p, 0x18, text_at);
        let hero = p.windows(5).rposition(|w| w == b"Hero\0").unwrap();
        p.splice(hero..hero, b"Sword\0Sharp\0".iter().copied());
        let r = Scenario::parse_editor_payload(&p).unwrap();
        assert_eq!(r.custom_artefacts, [CustomArtefact { record, name: "Sword".into(), description: "Sharp".into() }]);
        assert_eq!(r.scenario.named_characters[0].name, "Hero");
        assert_eq!(r.scenario.events, s.events);
    }

    #[test]
    fn old_versions_are_upgraded() {
        let base = Scenario::parse_payload(&sample_payload()).unwrap();
        let at = |p: &[u8], what: usize| {
            let u = |o: usize| u32::from_le_bytes(p[o..o + 4].try_into().unwrap()) as usize;
            HEADER_SIZE + u(0x1C) + u(0x20) + [0, u(0x24), u(0x24) + u(0x28)][what]
        };
        // Version 1: four-byte event ids, goods as signed bytes at 301.
        let mut p = sample_payload();
        p[9] = b'1';
        let b = at(&p, 0);
        p[b + 8..b + 16].copy_from_slice(&[5, 0, 9, 9, 6, 0, 0, 0]);
        p[b + 301..b + 307].copy_from_slice(&[3, 0xFF, 0, 0, 0, 4]);
        let pt = at(&p, 2);
        p[pt + 8..pt + 16].copy_from_slice(&[2, 0, 7, 7, 3, 0, 0, 0]);
        let s = Scenario::parse_editor_payload(&p).unwrap().scenario;
        assert_eq!(&s.buildings[0].event_slots[..3], &[5, 6, 0]);
        assert_eq!(&s.buildings[0].artifact_slots[..7], &[3, 0xFFFF, 0, 0, 0, 4, 0]);
        assert_eq!(&s.points[0].event_slots[..3], &[2, 3, 0]);
        assert_eq!(s.armies, base.armies, "version 1 gets only its own step");
        // Version 2: point model 6 becomes 8, armies get their id and model.
        let mut p = sample_payload();
        p[9] = b'2';
        let pt = at(&p, 2);
        p[pt + 5] = 6;
        let a = at(&p, 1);
        (p[a + 4], p[a + 59]) = (9, 1);
        let s = Scenario::parse_editor_payload(&p).unwrap().scenario;
        assert_eq!((s.points[0].model, s.armies[0].id, s.armies[0].model), (8, 1, 5));
        p[a + 63] = 1;
        assert_eq!(Scenario::parse_editor_payload(&p).unwrap().scenario.armies[0].model, 7);
        // Version 3: garrison strength 0 becomes 50, anything else 0.
        let mut p = sample_payload();
        p[9] = b'3';
        let a = at(&p, 1);
        assert_eq!(Scenario::parse_editor_payload(&p).unwrap().scenario.armies[0].garrison_strength, 50);
        p[a + 82] = 60;
        assert_eq!(Scenario::parse_editor_payload(&p).unwrap().scenario.armies[0].garrison_strength, 0);
        // The game does not load them at all.
        assert!(Scenario::parse_payload(&p).is_err());
    }

    #[test]
    fn header_fields_of_the_editor() {
        let mut h = Header::default();
        h.set_demo_flag(1);
        h.set_playability(0x1234);
        h.set_save_counter(7);
        h.set_quest_count(3);
        let s = Scenario { header: h, ..Scenario::default() };
        let p = s.to_payload();
        assert_eq!((p[0x117], &p[0x122..0x127]), (1, &[0x34, 0x12, 7, 0, 3][..]));
    }

    #[test]
    fn parses_hand_built_payload() {
        let s = Scenario::parse_payload(&sample_payload()).unwrap();
        let h = &s.header;
        assert_eq!((s.width(), s.height()), (4, 2));
        assert_eq!(h.start_date(), GameDate { year: 1204, month: 5, day: 20, hour: 9, minute: 0 });
        assert_eq!((h.hero(Archetype::Archmage).gold, h.hero(Archetype::Archmage).mana), (500, 300));
        assert_eq!(h.hero(Archetype::Archmage).x, 2);
        assert_eq!(h.hero(Archetype::Archmage).troops[0], Troop { unit: 4, level: 0, count: 3 });
        assert_eq!(h.relations[0], [0, 0, 0, -2]);
        assert_eq!(h.kind(), ScenarioKind::CampaignContinuation);
        assert_eq!(h.carry_over[0], 1);
        assert_eq!(s.terrain, [6, 6, 6, 6, 6, 4, 1, 1]);
        assert_eq!(s.terrain_at(1, 1), Some(4));
        assert_eq!(s.terrain_at(4, 0), None);
        assert_eq!(s.objects, [MapObject { x: 2, y: 1, sprite: 42, class: 9 }]);

        let b = &s.buildings[0];
        assert_eq!(b.building_type(), Some(BuildingType::Castle));
        assert_eq!(b.events().collect::<Vec<_>>(), [1]);
        assert_eq!(b.artifacts().collect::<Vec<_>>(), [146]);
        assert_eq!(b.barracks[0], RecruitSlot { unit: 4, start_count: 3, max_count: 9 });
        assert_eq!(b.garrison[0], Troop { unit: 9, level: 1, count: 2 });
        assert_eq!((b.gold_per_day, b.owner(), b.relations[0], b.start_for), (55, None, -2, [1, 0, 0]));
        assert_eq!((b.name.as_str(), b.owner_name.as_str(), b.description.as_str()), ("Castle", "Owner", ""));

        let a = &s.armies[0];
        assert_eq!((a.id, a.model(), a.speed_correction, a.aggression, a.unknown_80), (1, Some(ArmyModel::Bandits), -3, -20, 44));
        assert!(a.is_active());
        assert_eq!(a.troops().count(), 1);
        assert_eq!((a.name.as_str(), a.leader_name.as_str()), ("Gang", "Boss"));

        assert_eq!((s.points[0].id, s.points[0].model, s.points[0].radius), (1, 8, 24));

        let e = &s.events[0];
        assert_eq!(e.kind(), Some(EventKind::Quest));
        assert_eq!((e.repeat, e.conditions.gold, e.results.gold), (1440, -7499, -50));
        assert_eq!((e.results.light_lanterns[0], e.conditions.army_at_home, e.unknown_151[5]), (1, 1, 1));
        assert!(e.fires_once());
        assert_eq!(e.title_text(), "Quest");
        let f = e.flags.as_ref().unwrap();
        assert_eq!((f.set.as_deref(), f.require_unset.as_deref()), (Some("Foo"), Some("Bar")));
        assert_eq!((e.question.as_str(), e.message.as_str()), ("Q?", "Msg"));
        assert_eq!(e.custom_picture.as_deref(), Some(&[1u8, 0, 1, 0, 0xAB, 0xCD][..]));

        assert_eq!((s.title.as_str(), s.description.as_str(), s.campaign_name.as_str(), s.next_map.as_str()), ("Title", "Desc", "", "next.DTm"));
        assert_eq!(s.named_characters, [NamedCharacter { unit: 74, name: "Hero".into() }]);
        assert_eq!(s.scenario_picture.as_deref(), Some(&b"LIT\0!"[..]));
        assert_eq!(s.victory_event().map(|e| e.kind), Some(3));
        assert!(s.defeat_event().is_none());
    }

    #[test]
    fn payload_roundtrips_byte_exactly() {
        let bytes = sample_payload();
        let s = Scenario::parse_payload(&bytes).unwrap();
        assert_eq!(s.to_payload(), bytes);
    }

    #[test]
    fn reads_through_container() {
        let bytes = sample_payload();
        let file = container::encode(CONTAINER_VERSION, &bytes);
        assert_eq!(Scenario::from_file_bytes(&file).unwrap(), Scenario::parse_payload(&bytes).unwrap());
        assert!(Scenario::from_file_bytes(&bytes).is_ok());
    }

    #[test]
    fn trailing_bytes_are_not_looked_at() {
        let mut bytes = sample_payload();
        bytes.push(0);
        assert_eq!(Scenario::parse_payload(&bytes).unwrap(), Scenario::parse_payload(&sample_payload()).unwrap());
    }

    #[test]
    fn rejects_missing_picture_bytes() {
        let mut bytes = sample_payload();
        bytes.pop();
        assert!(matches!(Scenario::parse_payload(&bytes), Err(DtError::Truncated { .. })));
    }

    #[test]
    fn the_strings_start_at_the_header_text_offset() {
        // 0x4b27a8 seeks to the header's offset; the text marker is never read.
        let mut bytes = sample_payload();
        let off = u32::from_le_bytes(bytes[0x18..0x1C].try_into().unwrap());
        w32(&mut bytes, 0x18, off + 1);
        let s = Scenario::parse_payload(&bytes).unwrap();
        assert_eq!((s.title.as_str(), s.description.as_str()), ("itle", "Desc"));
        let mut bytes = sample_payload();
        bytes[off as usize - 8..off as usize].copy_from_slice(b"no-mark!");
        assert_eq!(Scenario::parse_payload(&bytes).unwrap().title, "Title");
    }

    #[test]
    fn sections_hold_size_div_record_records() {
        // Move 1 byte from the object section to the terrain section: 5 bytes hold no
        // object, and the odd terrain byte is ignored.
        let mut bytes = sample_payload();
        w32(&mut bytes, 0x1C, 7);
        w32(&mut bytes, 0x20, 5);
        let s = Scenario::parse_payload(&bytes).unwrap();
        assert_eq!((s.objects.len(), s.terrain.len()), (0, 8));
        assert_eq!((s.buildings.len(), s.title.as_str()), (1, "Title"));
    }

    #[test]
    fn strings_end_at_a_nul_or_the_end_of_the_data() {
        let bytes = sample_payload();
        let text_at = u32::from_le_bytes(bytes[0x18..0x1C].try_into().unwrap()) as usize;
        let mut cut = bytes[..text_at + 3].to_vec();
        w32(&mut cut, 0x11C, 0);
        let event_at = HEADER_SIZE + 6 + 6 + BUILDING_SIZE + ARMY_SIZE + POINT_SIZE;
        w32(&mut cut, event_at + 163, 0);
        let s = Scenario::parse_payload(&cut).unwrap();
        assert_eq!((s.title.as_str(), s.description.as_str(), s.named_characters[0].name.as_str()), ("Tit", "", ""));
    }

    #[test]
    fn event_picture_size_is_32_bits() {
        // Byte 163 is a full i32 (0x4b2bbd): a size with high bytes set is that big, and a
        // negative one means no picture.
        let bytes = sample_payload();
        let event_at = HEADER_SIZE + 6 + 6 + BUILDING_SIZE + ARMY_SIZE + POINT_SIZE;
        let mut big = bytes.clone();
        w32(&mut big, event_at + 163, 0x0001_0006);
        assert!(matches!(Scenario::parse_payload(&big), Err(DtError::Truncated { .. })));
        let mut negative = bytes.clone();
        w32(&mut negative, event_at + 163, (-6i32) as u32);
        assert_eq!(Scenario::parse_payload(&negative).unwrap().events[0].custom_picture, None);
    }

    #[test]
    fn only_header_byte_9_is_checked() {
        // 0x4b2504 loads nothing when byte 9 is below '4'; the other magic bytes are not read.
        let mut bytes = sample_payload();
        bytes[0] = b'X';
        assert!(Scenario::parse_payload(&bytes).is_ok());
        bytes[9] = b'3';
        assert!(matches!(Scenario::parse_payload(&bytes), Err(DtError::BadMagic { .. })));
        assert!(matches!(Scenario::parse_payload(&bytes[..20]), Err(DtError::BadMagic { .. })));
        assert!(matches!(Scenario::parse_payload(PAYLOAD_MAGIC), Err(DtError::Truncated { .. })));
    }

    #[test]
    fn terrain_rle() {
        assert_eq!(expand_terrain(&[5, 2, 7, 0], 2, 2).unwrap(), [5, 5, 5, 7]);
        // As the loader: an odd last byte is ignored, cells past the end are lost, cells not
        // reached stay 0.
        assert_eq!(expand_terrain(&[5, 2, 7], 2, 2).unwrap(), [5, 5, 5, 0]);
        assert_eq!(expand_terrain(&[5, 3, 7, 0], 2, 2).unwrap(), [5, 5, 5, 5]);
        assert_eq!(expand_terrain(&[5, 1], 2, 2).unwrap(), [5, 5, 0, 0]);
        assert!(expand_terrain(&[5, 255], u32::MAX, u32::MAX).is_err());
        let cells: Vec<u8> = std::iter::repeat_n(3, 300).chain([1, 1]).collect();
        let rle = compress_terrain(&cells);
        assert_eq!(rle, [3, 255, 3, 43, 1, 1]);
        assert_eq!(expand_terrain(&rle, 302, 1).unwrap(), cells);
    }

    #[test]
    fn game_dates() {
        let d = GameDate::from_minutes(624_354_300);
        assert_eq!(d, GameDate { year: 1204, month: 5, day: 20, hour: 9, minute: 0 });
        assert_eq!(d.to_minutes(), 624_354_300);
        assert_eq!(GameDate::from_minutes(0), GameDate { year: 0, month: 1, day: 1, hour: 0, minute: 0 });
        assert_eq!(GameDate::from_minutes(1440 * 30 * 12 - 1), GameDate { year: 0, month: 12, day: 30, hour: 23, minute: 59 });
    }

    #[test]
    fn flag_scripts() {
        let f = |t: &str| FlagScript::from_title(t);
        assert_eq!(f("No script"), None);
        let s = f("T%+Foo").unwrap();
        assert_eq!((s.set.as_deref(), s.clear, s.require_set, s.require_unset), (Some("Foo"), None, None, None));
        let s = f("T%-Foo=Foo").unwrap();
        assert_eq!((s.clear.as_deref(), s.require_set.as_deref()), (Some("Foo"), Some("Foo")));
        let s = f("T%=/Foo").unwrap();
        assert_eq!((s.set, s.clear, s.require_unset.as_deref()), (None, None, Some("Foo")));
        let s = f("T%+A B=C").unwrap();
        assert_eq!((s.set.as_deref(), s.require_set.as_deref(), s.raw.as_str()), (Some("A B"), Some("C"), "+A B=C"));
        let s = f("T%").unwrap();
        assert_eq!(s, FlagScript::default());
    }

    #[test]
    fn enums_from_codes() {
        assert_eq!(BuildingType::from_code(15), Some(BuildingType::Obelisk));
        assert_eq!(BuildingType::from_code(16), None);
        assert_eq!(ArmyModel::from_code(0), None);
        assert_eq!(ArmyModel::from_code(12), Some(ArmyModel::Zombies));
        assert_eq!(Surface::from_code(4), Some(Surface::Road));
        assert_eq!(Surface::from_code(16), None);
    }
}
