//! What new maps and new records start with. The values are Razdor's own choices, picked so
//! a new record behaves like the common case in the shipped maps (neutral buildings belong
//! to the "neighbour" faction and take its row of the relation matrix, castles have
//! barracks, armies give normal experience, and so on).

use crate::dt::dtm::{Army, Building, GameDate, Header, HeroPreset, Point, Scenario};

/// Map sizes the "new map" dialog offers (the shipped maps use these).
pub const MAP_SIZES: [u32; 3] = [50, 100, 200];

/// The relation matrix of a new map (rows and columns: player, ally, neighbour, enemy).
pub const DEFAULT_RELATIONS: [[i8; 4]; 4] = [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]];

/// A new map's clock: year 1200, month 1, day 1, 09:00.
pub fn default_start() -> u32 {
    GameDate { year: 1200, month: 1, day: 1, hour: 9, minute: 0 }.to_minutes()
}

/// Options of the "new map" dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewMap {
    pub width: u32,
    pub height: u32,
    /// Terrain code every cell starts with.
    pub fill: u8,
}

impl Default for NewMap {
    fn default() -> Self {
        NewMap { width: 50, height: 50, fill: 6 }
    }
}

/// An empty scenario: one surface, no objects or records, the hero presets in the middle of
/// the map with some gold, the default relations, a standalone scenario.
pub fn new_scenario(o: NewMap) -> Scenario {
    let (cx, cy) = ((o.width / 2).min(u16::MAX as u32) as u16, (o.height / 2).min(u16::MAX as u32) as u16);
    let hero = |mana: u32| HeroPreset { gold: 500, mana, x: cx, y: cy, ..HeroPreset::default() };
    Scenario {
        header: Header {
            width: o.width,
            height: o.height,
            start_time: default_start(),
            heroes: [hero(0), hero(50), hero(0)],
            relations: DEFAULT_RELATIONS,
            ..Header::default()
        },
        terrain: vec![o.fill.min(15); o.width as usize * o.height as usize],
        title: crate::i18n::tr("New scenario").into(),
        ..Scenario::default()
    }
}

/// The original's File › New (0x5a4274) at the current size: a zeroed header with the size
/// and the generator's state as its seed (header 0x14), terrain 0 everywhere, no objects or
/// records, empty texts and a default title (Razdor's own wording).
pub fn cleared_scenario(width: u32, height: u32, seed: u32) -> Scenario {
    Scenario {
        header: Header { width, height, generator_seed: seed, ..Header::default() },
        terrain: vec![0; width as usize * height as usize],
        title: crate::i18n::tr("New scenario").into(),
        ..Scenario::default()
    }
}

/// A new building of type `kind` with its picture and footprint; `(x, y)` is the
/// bottom-right cell.
pub fn new_building(header: &Header, x: u16, y: u16, kind: u8, picture_type: u8, variant: u8, size: (u8, u8)) -> Building {
    let faction = 3;
    Building {
        x,
        y,
        kind,
        picture_type,
        picture_variant: variant,
        size_x: size.0.max(1),
        size_y: size.1.max(1),
        owner_army: 0xFF,
        faction,
        relations: header.relations[faction as usize - 1],
        // Towns, castles and forts recruit; towns' and ruins' garrisons serve the AI only.
        has_barracks: matches!(kind, 1 | 3 | 4) as u8,
        garrison_ai_only: matches!(kind, 1 | 12) as u8,
        ..Building::default()
    }
}

/// A new army as the original places one (0x595390): a zeroed record with its position,
/// id and the model picked in the menu (4 feudal, 5 rogue, 6 peasant, 7 inactive), XP
/// correction 100, the style `model − 4` (model 7: style 0 and inactive at the start) and
/// a default name ending in its number. Its faction and attitudes stay 0 until its window
/// saves it.
pub fn new_army(id: u8, x: u16, y: u16, model: u8) -> Army {
    let model = model.clamp(4, 7);
    let inactive = model == 7;
    Army {
        x,
        y,
        id,
        model,
        behaviour: if inactive { 0 } else { model - 4 },
        inactive: inactive as u8,
        exp_correction: 100,
        name: crate::trf!("Army {n}", n = id),
        ..Army::default()
    }
}

/// Point models: a lantern, an event point and an AI target point (records.md §10).
pub const LANTERN: u8 = 8;
pub const EVENT_POINT: u8 = 9;
pub const TARGET_POINT: u8 = 10;
/// A new lantern's radius (the number dialog then asks, with this default).
pub const LANTERN_RADIUS: u8 = 10;

/// A new point as the original places one (0x595390, mode 4): a zeroed record with its
/// position and the word `model·256 + id` at byte 4, so the 256th point (id 256) stores id
/// 0 and its model plus 1 there (the original's overflow, kept; Razdor's file check then
/// refuses the map). A lantern is lit at the start with radius [`LANTERN_RADIUS`].
pub fn new_point(id: u16, x: u16, y: u16, model: u8) -> Point {
    let word = ((model as u16) << 8).wrapping_add(id);
    let lantern = model == LANTERN;
    Point {
        x,
        y,
        id: word as u8,
        model: (word >> 8) as u8,
        radius: if lantern { LANTERN_RADIUS } else { 0 },
        active: lantern as u8,
        ..Point::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_scenarios() {
        let s = new_scenario(NewMap { width: 100, height: 100, fill: 2 });
        assert_eq!((s.width(), s.height(), s.terrain.len()), (100, 100, 10_000));
        assert!(s.terrain.iter().all(|c| *c == 2));
        assert_eq!(s.header.start_date(), GameDate { year: 1200, month: 1, day: 1, hour: 9, minute: 0 });
        assert!(s.header.heroes.iter().all(|h| (h.x, h.y) == (50, 50) && h.gold == 500));
        assert_eq!(s.header.relations, DEFAULT_RELATIONS);
        assert_eq!(new_scenario(NewMap { fill: 99, ..NewMap::default() }).terrain[0], 15);
    }

    #[test]
    fn the_original_new_keeps_the_size_only() {
        let s = cleared_scenario(80, 60, 1234);
        assert_eq!((s.width(), s.height(), s.header.generator_seed, s.header.start_time), (80, 60, 1234, 0));
        assert!(s.terrain.len() == 4800 && s.terrain.iter().all(|t| *t == 0));
        assert!(s.header.heroes.iter().all(|h| (h.x, h.y, h.gold) == (0, 0, 0)) && s.header.relations == [[0; 4]; 4]);
        assert!(s.buildings.is_empty() && s.objects.is_empty() && s.events.is_empty());
    }

    #[test]
    fn new_records() {
        let h = new_scenario(NewMap::default()).header;
        let b = new_building(&h, 10, 10, 3, 3, 2, (4, 4));
        assert_eq!((b.owner(), b.faction, b.relations, b.has_barracks, b.garrison_ai_only), (None, 3, [1, 1, 3, 1], 1, 0));
        assert_eq!((b.picture_type, b.picture_variant, b.size_x, b.size_y), (3, 2, 4, 4));
        assert_eq!(new_building(&h, 1, 1, 12, 12, 0, (0, 0)).size_x, 1);
        let a = new_army(3, 5, 6, 5);
        assert_eq!((a.id, a.model, a.behaviour, a.inactive, a.faction, a.relations, a.exp_correction), (3, 5, 1, 0, 0, [0; 4], 100));
        assert_eq!((a.name.as_str(), a.leader_name.as_str(), a.patrols, a.garrison_strength), ("Army 3", "", 0, 0));
        let i = new_army(4, 5, 6, 7);
        assert_eq!((i.model, i.behaviour, i.inactive), (7, 0, 1));
        assert_eq!(new_army(1, 0, 0, 4).behaviour, 0);
        let l = new_point(2, 1, 1, LANTERN);
        assert_eq!((l.id, l.model, l.radius, l.active, l.serial), (2, 8, 10, 1, 0));
        assert_eq!((new_point(1, 0, 0, EVENT_POINT).model, new_point(1, 0, 0, EVENT_POINT).radius), (9, 0));
        assert_eq!(new_point(3, 0, 0, TARGET_POINT).model, 10);
        // The 256th point overflows its id into the model byte.
        let p = new_point(256, 0, 0, LANTERN);
        assert_eq!((p.id, p.model), (0, 9));
        assert_eq!(new_point(256, 0, 0, TARGET_POINT).model, 11);
        assert_eq!(new_point(256, 0, 0, EVENT_POINT).model, 10);
    }
}
