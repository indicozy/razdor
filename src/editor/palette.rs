//! What the editor offers to place, and the names it shows.
//!
//! The object and building palettes come from the player's `Objects.ugs` at runtime (its
//! record keys are the `.DTm` object `(class, sprite)` and building `(picture type,
//! variant)` keys, and a building sprite carries its footprint). Without an install a small
//! fallback palette of our own is offered. Unit, artefact and spell names come from the
//! [`Content`] the editor runs with. The labels in this file are Razdor's own English.

use crate::dt::dtm::BuildingType;
use crate::i18n::{n_, tr};
use crate::dt::gfx::{ObjectSprite, ObjectSprites};
use crate::rules::content::Content;

use super::geometry::is_massif;

/// A map object the palette offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectKey {
    pub class: u8,
    pub sprite: u8,
}

/// A building picture the palette offers, with its footprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingPicture {
    pub picture_type: u8,
    pub variant: u8,
    pub size: (u8, u8),
    /// The brush a palette click on it sets (0x5aaa7c): the picture's width in pixels div
    /// 32, the square the placement must fit and its preview.
    pub brush: u8,
}

/// The objects and building pictures that can be placed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Palette {
    /// Sorted by class, then sprite.
    pub objects: Vec<ObjectKey>,
    /// Sorted by picture type, then variant.
    pub buildings: Vec<BuildingPicture>,
    /// Built from the install's art (keys are checked on save); otherwise the fallback.
    pub from_install: bool,
}

/// Object classes, with Razdor's labels. 1–8 are massifs, 9–12 plants.
pub const OBJECT_CLASSES: [(u8, &str); 11] = [
    (1, n_("Grass hills")),
    (2, n_("Green hill")),
    (3, n_("Rocky hills")),
    (4, n_("Yellow hill")),
    (5, n_("Mountains")),
    (6, n_("Dark mountains")),
    (8, n_("Stones")),
    (9, n_("Trees and bushes")),
    (10, n_("Dead trees")),
    (11, n_("Dense thicket")),
    (12, n_("Plants (12)")),
];

pub fn object_class_label(class: u8) -> String {
    OBJECT_CLASSES.iter().find(|c| c.0 == class).map_or(crate::trf!("Class {class}", class), |c| tr(c.1).to_string())
}

/// Building type labels (byte 6).
pub fn building_type_label(kind: u8) -> &'static str {
    tr(match BuildingType::from_code(kind) {
        Some(BuildingType::Palace) => n_("Palace"),
        Some(BuildingType::Town) => n_("Town"),
        Some(BuildingType::Village) => n_("Village"),
        Some(BuildingType::Castle) => n_("Castle"),
        Some(BuildingType::Fort) => n_("Fort"),
        Some(BuildingType::Tavern) => n_("Tavern"),
        Some(BuildingType::Market) => n_("Market"),
        Some(BuildingType::Church) => n_("Church"),
        Some(BuildingType::Smithy) => n_("Smithy / house"),
        Some(BuildingType::Shipyard) => n_("Shipyard"),
        Some(BuildingType::Altar) => n_("Altar"),
        Some(BuildingType::DungeonEntrance) => n_("Dungeon entrance"),
        Some(BuildingType::Ruins) => n_("Ruins"),
        Some(BuildingType::StoneBridge) => n_("Stone bridge"),
        Some(BuildingType::WoodenBridge) => n_("Wooden bridge"),
        Some(BuildingType::Obelisk) => n_("Obelisk"),
        None => n_("Unknown building"),
    })
}

/// Terrain labels (codes 0–15, the editor's surface palette order).
pub const SURFACE_LABELS: [&str; 16] = [
    n_("Shallows, fords"),
    n_("Coastal water"),
    n_("Deep sea"),
    n_("Lava fields"),
    n_("Road"),
    n_("Grass lowland"),
    n_("Grass plain"),
    n_("Dry plain"),
    n_("Marsh"),
    n_("Impassable swamp"),
    n_("Sand and dunes"),
    n_("Clay soil"),
    n_("Stony soil"),
    n_("Scorched land"),
    n_("Snowy ground"),
    n_("Impassable snow"),
];

/// Army map models (byte 5), 1-based.
pub const ARMY_MODELS: [(u8, &str); 12] = [
    (1, n_("Hero: knight")),
    (2, n_("Hero: archmage")),
    (3, n_("Hero: ranger")),
    (4, n_("Feudal lord")),
    (5, n_("Bandits")),
    (6, n_("Peasants")),
    (7, n_("Inactive")),
    (8, n_("Lantern")),
    (9, n_("Event point")),
    (10, n_("Necromancer")),
    (11, n_("Ghosts")),
    (12, n_("Zombies")),
];

pub const BEHAVIOURS: [&str; 3] = [n_("Feudal"), n_("Rogue"), n_("Peasant")];
pub const TARGET_MODELS: [&str; 5] = [n_("Standard"), n_("Aggressive"), n_("Passive"), n_("Hoarding"), n_("Trading")];
pub const SHIPS: [&str; 4] = [n_("No ship"), n_("Hero ship"), n_("Pirates"), n_("Merchant")];
/// Factions 1–4 (the editor's colours: green, blue, yellow, red).
pub const FACTIONS: [&str; 4] = [n_("Player (green)"), n_("Ally (blue)"), n_("Neighbour (yellow)"), n_("Enemy (red)")];
pub const HERO_CLASSES: [&str; 3] = [n_("Knight"), n_("Archmage"), n_("Ranger")];
pub const SCENARIO_KINDS: [&str; 3] = [n_("Standalone"), n_("First map of a campaign"), n_("Later campaign map")];
pub const CARRY_OVER: [&str; 7] = [n_("Gold"), n_("Gods' favour"), n_("Fame"), n_("Experience and level"), n_("Personal artefacts"), n_("Whole inventory"), n_("Whole army")];

/// Footprint Razdor gives a building type when no install tells it (our own choice).
pub fn fallback_size(kind: u8) -> (u8, u8) {
    match BuildingType::from_code(kind) {
        Some(BuildingType::Town) => (7, 7),
        Some(BuildingType::Village) => (4, 4),
        Some(BuildingType::Castle) => (4, 4),
        Some(BuildingType::Shipyard) => (3, 3),
        Some(BuildingType::StoneBridge | BuildingType::WoodenBridge) => (1, 1),
        _ => (2, 2),
    }
}

impl Palette {
    /// The palette of the install's `Objects.ugs`.
    pub fn from_sprites(sprites: &ObjectSprites) -> Palette {
        let mut objects: Vec<ObjectKey> = sprites
            .sprites
            .iter()
            .filter(|s| s.section == ObjectSprite::DECORATIONS)
            .filter_map(|s| Some(ObjectKey { class: u8::try_from(s.cat).ok()?, sprite: u8::try_from(s.idx).ok()? }))
            .collect();
        objects.sort();
        objects.dedup();
        let mut buildings: Vec<BuildingPicture> = sprites
            .sprites
            .iter()
            .filter(|s| s.section == ObjectSprite::BUILDINGS)
            .filter_map(|s| {
                let size = s.footprint()?;
                let brush = u8::try_from(s.image.width >> 5).unwrap_or(u8::MAX);
                Some(BuildingPicture { picture_type: u8::try_from(s.cat).ok()?, variant: u8::try_from(s.idx).ok()?, size, brush })
            })
            .collect();
        buildings.sort_by_key(|b| (b.picture_type, b.variant));
        buildings.dedup_by_key(|b| (b.picture_type, b.variant));
        Palette { objects, buildings, from_install: true }
    }

    /// Without an install: every object class with sprite ids 10–13 for massifs and 0–8 for
    /// plants, and variant 0 of every building type with [`fallback_size`]. The game may not
    /// have art for all of these; save checks them only against an install's palette.
    pub fn fallback() -> Palette {
        let mut objects = Vec::new();
        for (class, _) in OBJECT_CLASSES {
            let ids: Vec<u8> = if is_massif(class) { (10..=13).collect() } else { (0..=8).collect() };
            objects.extend(ids.into_iter().map(|sprite| ObjectKey { class, sprite }));
        }
        let buildings = (1..=15u8).map(|t| BuildingPicture { picture_type: t, variant: 0, size: fallback_size(t), brush: fallback_size(t).0 }).collect();
        Palette { objects, buildings, from_install: false }
    }

    /// Object classes present, in order.
    pub fn classes(&self) -> Vec<u8> {
        let mut c: Vec<u8> = self.objects.iter().map(|o| o.class).collect();
        c.dedup();
        c
    }

    pub fn sprites_of(&self, class: u8) -> impl Iterator<Item = ObjectKey> + '_ {
        self.objects.iter().copied().filter(move |o| o.class == class)
    }

    pub fn has_object(&self, class: u8, sprite: u8) -> bool {
        self.objects.binary_search(&ObjectKey { class, sprite }).is_ok()
    }

    /// Picture variants of a building picture type.
    pub fn pictures_of(&self, picture_type: u8) -> impl Iterator<Item = BuildingPicture> + '_ {
        self.buildings.iter().copied().filter(move |b| b.picture_type == picture_type)
    }

    pub fn picture(&self, picture_type: u8, variant: u8) -> Option<BuildingPicture> {
        self.buildings.iter().copied().find(|b| b.picture_type == picture_type && b.variant == variant)
    }

    /// The footprint of a picture, else the fallback size of its type.
    pub fn footprint(&self, picture_type: u8, variant: u8) -> (u8, u8) {
        self.picture(picture_type, variant).map_or(fallback_size(picture_type), |p| p.size)
    }

    /// The hills page's palette at brush size `size` (0x59a718 groups, 0x5aaa7c): the
    /// objects of classes 1–8 whose sprite div 10 is `size` (their footprint side), by
    /// class, then sprite.
    pub fn hills(&self, size: u32) -> Vec<ObjectKey> {
        self.objects.iter().copied().filter(|o| is_massif(o.class) && (o.sprite / 10) as u32 == size).collect()
    }

    /// The forests page's palette: at size 1 every sprite below 120 of classes 9–12, at
    /// larger sizes one entry per family of twelve (the sprites that are a multiple of 12).
    pub fn forests(&self, size: u32) -> Vec<ObjectKey> {
        let trees = self.objects.iter().copied().filter(|o| (9..=12).contains(&o.class) && o.sprite < 120);
        if size < 2 {
            trees.collect()
        } else {
            trees.filter(|o| o.sprite % 12 == 0).collect()
        }
    }

    /// What the forest brush and the burn read about the plant sprites.
    pub fn forest_facts(&self) -> ForestFacts {
        let mut f = ForestFacts::default();
        for o in &self.objects {
            if (9..=12).contains(&o.class) && o.sprite < 120 {
                let c = (o.class - 9) as usize;
                f.counts[c][(o.sprite / 12) as usize] += 1;
                if self.has_object(o.class, o.sprite + 120) {
                    f.alternates[c] |= 1 << o.sprite;
                }
            }
        }
        f
    }
}

/// The plant sprites as the original's palette counts them (0x59a718), for classes 9–12.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForestFacts {
    /// Sprites of each family of twelve (sprite div 12), below sprite 120.
    pub counts: [[u8; 10]; 4],
    /// Bit `s`: sprite `s + 120` exists (the "+120" alternate picture).
    pub alternates: [u128; 4],
}

impl ForestFacts {
    /// Sprites in family `family` of `class` (0 for another class).
    pub fn count(&self, class: u8, family: u8) -> u8 {
        match (class.checked_sub(9), family) {
            (Some(c @ 0..=3), f @ 0..=9) => self.counts[c as usize][f as usize],
            _ => 0,
        }
    }

    /// Whether sprite `sprite` of `class` has a "+120" alternate.
    pub fn has_alternate(&self, class: u8, sprite: u8) -> bool {
        matches!(class.checked_sub(9), Some(c @ 0..=3) if sprite < 120 && self.alternates[c as usize] >> sprite & 1 == 1)
    }
}

/// One choice of a picker: the id the map stores and the name shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: u32,
    pub name: String,
}

/// Names of the units, artefacts and spells of the editor's content (read from the player's
/// install at runtime, or the built-in demo), for the pickers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Names {
    /// By `GlobalIndex`.
    pub units: Vec<Choice>,
    /// By `GlobalIndex`.
    pub artefacts: Vec<Choice>,
    /// By 1-based index.
    pub spells: Vec<Choice>,
    /// The numbers the map check and the playability score read.
    pub facts: Facts,
    /// Unit ids in the order of the original's unit lists ([`super::menus::unit_order`]).
    pub unit_order: Vec<u32>,
    /// Artefact ids in the order of its artefact lists ([`super::menus::artefact_order`]).
    pub artefact_order: Vec<u32>,
    /// The undead units (the armies submenu files armies they lead apart).
    pub undead: Vec<u32>,
}

/// An artefact's price and type as the original editor keeps them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArtefactFact {
    pub id: u32,
    pub cost: i32,
    /// The editor's type code (DTMapEdit 0x59bc78): 0 BlowWeapon, 1 ShotWeapon, 2 Armor,
    /// 3 Helm, 4 Shield, 5 Staff, 6 Amulet, 7 Ring, 8 Potion, 9 Item.
    pub kind: u8,
}

/// The install's numbers the original editor's map check and score read: artefact prices
/// and types, unit prices, the spells' fixed hit-point change and `CostRecrutDiv`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// In list order (by `GlobalIndex`).
    pub artefacts: Vec<ArtefactFact>,
    /// (`GlobalIndex`, `Cost`).
    pub unit_costs: Vec<(u32, i32)>,
    /// `DeltaFixedHits` by 1-based spell index.
    pub spell_fixed_hits: Vec<Option<i32>>,
    /// `CostRecrutDiv` of `_Global.ini` (2 in the install).
    pub recruit_div: i32,
}

impl Facts {
    pub fn from_content(c: &Content) -> Facts {
        use crate::dt::data::ArtefactType as T;
        let kind = |a: &crate::dt::data::ArtefactDef| -> u8 {
            // The loader reads the type only of an artefact with a name and a description;
            // the others keep the default, Item.
            if a.name.is_empty() || a.description.is_empty() {
                return 9;
            }
            match a.kind {
                T::BlowWeapon => 0,
                T::ShotWeapon => 1,
                T::Armor => 2,
                T::Helm => 3,
                T::Shield => 4,
                T::Staff => 5,
                T::Amulet => 6,
                T::Ring => 7,
                T::Potion => 8,
                T::Item => 9,
            }
        };
        Facts {
            artefacts: c.items.iter().map(|a| ArtefactFact { id: a.id, cost: a.cost, kind: kind(a) }).collect(),
            unit_costs: c.units.iter().map(|u| (u.id, u.cost)).collect(),
            spell_fixed_hits: c.spells.iter().map(|s| s.delta_fixed_hits).collect(),
            recruit_div: c.options.cost_recrut_div,
        }
    }

    /// An artefact by `GlobalIndex`; one the list does not have reads as the original's
    /// empty record (price 0, type 0).
    pub fn artefact(&self, id: u32) -> ArtefactFact {
        self.artefacts.iter().copied().find(|a| a.id == id).unwrap_or(ArtefactFact { id, cost: 0, kind: 0 })
    }

    pub fn unit_cost(&self, id: u32) -> i32 {
        self.unit_costs.iter().find(|u| u.0 == id).map_or(0, |u| u.1)
    }

    /// The fixed hit-point change of a spell (1-based), 0 when it has none.
    pub fn spell_fixed_hits(&self, id: u32) -> i32 {
        (id as usize).checked_sub(1).and_then(|i| self.spell_fixed_hits.get(i).copied().flatten()).unwrap_or(0)
    }
}

/// `list` in the order of `order` (ids), then whatever `order` does not name.
fn sorted<'a>(list: &'a [Choice], order: &[u32]) -> Vec<&'a Choice> {
    let mut out: Vec<&Choice> = order.iter().filter_map(|id| list.iter().find(|c| c.id == *id)).collect();
    out.extend(list.iter().filter(|c| !order.contains(&c.id)));
    out
}

impl Names {
    pub fn from_content(c: &Content) -> Names {
        Names {
            units: c.units.iter().map(|u| Choice { id: u.id, name: u.name.clone() }).collect(),
            artefacts: c.items.iter().map(|a| Choice { id: a.id, name: a.name.clone() }).collect(),
            spells: c.spells.iter().enumerate().map(|(i, s)| Choice { id: i as u32 + 1, name: s.name.clone() }).collect(),
            facts: Facts::from_content(c),
            unit_order: super::menus::unit_order(&c.units),
            artefact_order: super::menus::artefact_order(&Facts::from_content(c).artefacts),
            undead: c.units.iter().filter(|u| u.nature == crate::dt::data::Nature::Undead).map(|u| u.id).collect(),
        }
    }

    /// The units in the lists' order (those the order does not know at the end).
    pub fn units_sorted(&self) -> Vec<&Choice> {
        sorted(&self.units, &self.unit_order)
    }

    /// The artefacts in the lists' order.
    pub fn artefacts_sorted(&self) -> Vec<&Choice> {
        sorted(&self.artefacts, &self.artefact_order)
    }

    fn name(list: &[Choice], id: u32, what: &str) -> String {
        match id {
            0 => tr("(none)").to_string(),
            _ => list.iter().find(|c| c.id == id).map_or(format!("{} #{id}", tr(what)), |c| c.name.clone()),
        }
    }

    pub fn unit(&self, id: u32) -> String {
        Names::name(&self.units, id, n_("unit"))
    }

    pub fn artefact(&self, id: u32) -> String {
        Names::name(&self.artefacts, id, n_("artefact"))
    }

    pub fn spell(&self, id: u32) -> String {
        Names::name(&self.spells, id, n_("spell"))
    }

    pub fn has_unit(&self, id: u32) -> bool {
        self.units.iter().any(|c| c.id == id)
    }

    pub fn has_artefact(&self, id: u32) -> bool {
        self.artefacts.iter().any(|c| c.id == id)
    }

    pub fn has_spell(&self, id: u32) -> bool {
        self.spells.iter().any(|c| c.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::gfx::Image;

    fn sprite(section: u32, cat: u32, idx: u32, extra: Vec<u8>) -> ObjectSprite {
        ObjectSprite { section, cat, idx, cell: 1, extra, image: Image { width: 1, height: 1, rgba: vec![0; 4] } }
    }

    fn size(a: u32, b: u32) -> Vec<u8> {
        a.to_le_bytes().into_iter().chain(b.to_le_bytes()).collect()
    }

    #[test]
    fn palette_from_sprites() {
        let sprites = ObjectSprites {
            sprites: vec![
                sprite(0, 9, 5, vec![0; 4]),
                sprite(0, 1, 20, vec![0; 4]),
                sprite(0, 9, 1, vec![0; 4]),
                sprite(1, 3, 1, size(4, 4)),
                sprite(1, 3, 0, size(4, 4)),
                sprite(1, 9, 0, size(4, 3)),
            ],
        };
        let p = Palette::from_sprites(&sprites);
        assert!(p.from_install);
        assert_eq!(p.classes(), vec![1, 9]);
        assert_eq!(p.sprites_of(9).map(|o| o.sprite).collect::<Vec<_>>(), vec![1, 5]);
        assert!(p.has_object(1, 20) && !p.has_object(1, 21));
        assert_eq!(p.pictures_of(3).map(|b| b.variant).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(p.footprint(9, 0), (4, 3));
        // Unknown pictures fall back to our own size.
        assert_eq!(p.footprint(1, 7), fallback_size(1));
    }

    #[test]
    fn hill_and_forest_palettes_and_families() {
        let key = |class, sprite| ObjectKey { class, sprite };
        let mut objects = vec![key(1, 10), key(1, 25), key(5, 21), key(8, 3), key(9, 0), key(9, 1), key(9, 2), key(9, 12), key(9, 13), key(9, 121), key(9, 132), key(11, 108), key(11, 109), key(12, 24)];
        objects.sort();
        let p = Palette { objects, buildings: vec![], from_install: true };
        assert_eq!(p.hills(2), [key(1, 25), key(5, 21)]);
        assert_eq!(p.hills(1), [key(1, 10)]);
        assert_eq!(p.forests(1).len(), 8, "every plant sprite below 120");
        assert_eq!(p.forests(3), [key(9, 0), key(9, 12), key(11, 108), key(12, 24)], "one per family");
        let f = p.forest_facts();
        assert_eq!((f.count(9, 0), f.count(9, 1), f.count(11, 9), f.count(12, 2), f.count(10, 0), f.count(5, 0)), (3, 2, 2, 1, 0, 0));
        assert!(f.has_alternate(9, 1) && f.has_alternate(9, 12) && !f.has_alternate(9, 0) && !f.has_alternate(11, 108));
    }

    #[test]
    fn fallback_palette_covers_every_type() {
        let p = Palette::fallback();
        assert!(!p.from_install);
        assert!((1..=15).all(|t| p.picture(t, 0).is_some()));
        assert_eq!(p.classes().len(), OBJECT_CLASSES.len());
    }

    #[test]
    fn names_from_content() {
        let c = Content::builtin();
        let n = Names::from_content(&c);
        assert_eq!(n.unit(1), c.units[0].name);
        assert_eq!(n.unit(0), "(none)");
        assert_eq!(n.unit(9999), "unit #9999");
        assert!(n.has_unit(1) && !n.has_unit(9999));
        assert_eq!(n.spells.first().map(|s| s.id), Some(1));
    }
}
