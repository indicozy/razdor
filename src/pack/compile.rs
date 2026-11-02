//! Compiles a package into what the engine plays: the package's units and items (each cloned from
//! its base with the overrides, under a new id), and one of its maps as a `.DTm` scenario.
//!
//! Ids. A package unit gets `UNIT_BASE + n` (n = its position in `units/`), so its ids start where
//! the base game's end (units 1 to 102; 1 to 3 are the hero classes). A troop slot holds a unit id
//! in one byte, and 255 means "any unit" in an event's removal list and 254 "added by an event",
//! so a package has at most `UNIT_LIMIT` = 55 units (200 to 254). A package item gets
//! `ITEM_BASE + n`: the base game's items end at 167, and the items an event or a hero's preset
//! names are one byte too (`artifacts_add: [u8; 4]`, `HeroPreset::artifacts`), so items 200 to 255
//! are the ones events can give: `ITEM_LIMIT` = 56. The base game's units end at 102 and items at
//! 167 (`Rus_Units.ini`, `Rus_Artefacts.ini`), so 200 leaves room for both.
//!
//! Pictures. A package unit has no picture of its own: it is drawn with the picture of its root
//! base unit (`DtInstall::unit_picture_frame`), so [`Compiled::pictures`] gives the new id that frame.

use std::collections::BTreeMap;

use super::aliases;
use super::check::check;
use super::content::{ItemSpec, UnitSpec};
use super::diag::Report;
use super::load::{Located, Package};
use super::map::{parse_date, ArmySpec, BuildingSpec, MapFile, TroopSpec, Victory};
use super::refs::{DtKind, Ref};
use super::terrain::{self, Palette};
use crate::dt::data::{ArtefactDef, UnitDef};
use crate::dt::dtm::{
    Army, Building, Event, EventConditions, EventResults, GameDate, Header, HeroPreset, MapObject, NamedCharacter, Point, RecruitSlot,
    Scenario, Troop,
};
use crate::dt::install::DtInstall;
use crate::rules::content::{ItemId, UnitId};
use crate::rules::triggers::Resolve;

// The base game's units end at 102 (1 to 3 are the hero classes) and its items at 167.
const _: () = assert!(UNIT_BASE > 102 && ITEM_BASE > 167);

/// The first id of a package unit.
pub const UNIT_BASE: u32 = 200;
/// The most units a package may have (ids 200 to 254).
pub const UNIT_LIMIT: usize = 55;
/// The first id of a package item.
pub const ITEM_BASE: u32 = 200;
/// The most items a package may have (ids 200 to 255).
pub const ITEM_LIMIT: usize = 56;
/// The most named characters a scenario holds (`named_character_slots`).
pub const NAMED_LIMIT: usize = 32;
/// The most troops a slot list holds (a hero, an army, a garrison: six slots).
pub const TROOP_SLOTS: usize = 6;

/// A compiled package, for one of its maps.
#[derive(Debug)]
pub struct Compiled {
    pub scenario: Scenario,
    /// The package's units, with their new ids, in `units/` order.
    pub units: Vec<UnitDef>,
    /// The package's items, with their new ids, in `items/` order.
    pub items: Vec<ArtefactDef>,
    /// `(new unit id, picture frame)` of each package unit with a picture (see the module docs).
    pub pictures: Vec<(u32, usize)>,
    /// The ids of this map's armies and buildings, and of the package's units and items, by name.
    pub names: MapNames,
}

/// The base game a package builds on: its unit and item definitions, and the picture frame of a
/// unit type by its `GlobalIndex`.
pub struct Base<'a> {
    pub units: &'a [UnitDef],
    pub items: &'a [ArtefactDef],
    pub picture: &'a dyn Fn(u32) -> Option<usize>,
}

/// Compiles `map_id` of `pkg` on the install `install`; `lang` picks the texts (any language
/// the package has, if it lacks that one). The report holds the errors of [`check`] or of the
/// compile.
pub fn compile(pkg: &Package, install: &DtInstall, lang: &str, map_id: &str) -> Result<Compiled, Report> {
    let base = Base { units: &install.units, items: &install.artefacts, picture: &|id| install.unit_picture_frame(id) };
    compile_with(pkg, &base, lang, map_id)
}

/// [`compile`] on a base given directly (the tests use the built-in demo content).
pub fn compile_with(pkg: &Package, base: &Base, lang: &str, map_id: &str) -> Result<Compiled, Report> {
    let report = check(pkg);
    if report.has_errors() {
        return Err(report);
    }
    let mut r = Report::default();
    let texts = Texts { pkg, lang };
    let units = unit_ids(pkg);
    let items = item_ids(pkg);

    let mut out_units = Vec::new();
    let mut pictures = Vec::new();
    for u in &pkg.units {
        if !units.contains_key(u.value.id.as_str()) {
            continue;
        }
        match unit_def(pkg, &units, base, &texts, &u.value.id, 0) {
            Ok((def, root)) => {
                if let Some(frame) = (base.picture)(root) {
                    pictures.push((def.id, frame));
                }
                out_units.push(def);
            }
            Err(e) => r.error(&u.file, Some(u.line), e),
        }
    }
    let mut out_items = Vec::new();
    for i in &pkg.items {
        if !items.contains_key(i.value.id.as_str()) {
            continue;
        }
        match item_def(pkg, &items, base, &texts, &i.value.id, 0) {
            Ok(def) => out_items.push(def),
            Err(e) => r.error(&i.file, Some(i.line), e),
        }
    }
    if r.has_errors() {
        return Err(r);
    }

    let map = pkg.maps.get(map_id).ok_or_else(|| one_error(format!("no map `{map_id}` in the package: its maps are {}", list(pkg.maps.keys()))))?;
    let spec = map.spec.as_ref().ok_or_else(|| one_error(format!("maps/{map_id}/map.toml does not parse")))?;
    let scenario = scenario_of(pkg, &units, &texts, &map.file, map_id, spec)?;
    // The ids a trigger's name resolves to: the map's armies and buildings in file order (the
    // scenario's ids: army `i + 1`, building `i + 1`), the package's own units and items.
    let names = MapNames {
        units: units.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        items: items.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        armies: spec.army.iter().enumerate().map(|(i, a)| (a.get_ref().id.clone(), (i + 1) as u8)).collect(),
        buildings: spec.building.iter().enumerate().map(|(i, b)| (b.get_ref().id.clone(), (i + 1) as u16)).collect(),
        battles: spec.army.iter().enumerate().filter_map(|(i, a)| a.get_ref().battle.as_ref().map(|r| ((i + 1) as u8, r.clone()))).collect(),
    };
    Ok(Compiled { scenario, units: out_units, items: out_items, pictures, names })
}

/// The names of one compiled map's ids: what a package's triggers and dialogs refer to, resolved
/// to the engine's ids (`rules::triggers::Resolve`). Built by [`compile_with`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapNames {
    /// The battle of each map army that names one (`[[army]] battle`), by the army's id.
    pub battles: std::collections::BTreeMap<u8, String>,
    /// The package's units and items by their local ids.
    pub units: BTreeMap<String, u32>,
    pub items: BTreeMap<String, u32>,
    /// The map's armies and buildings by their ids in `map.toml`.
    pub armies: BTreeMap<String, u8>,
    pub buildings: BTreeMap<String, u16>,
}

impl Resolve for MapNames {
    fn unit(&self, local: &str) -> Option<UnitId> {
        self.units.get(local).map(|&u| UnitId(u))
    }

    fn item(&self, local: &str) -> Option<ItemId> {
        self.items.get(local).map(|&i| ItemId(i))
    }

    fn army(&self, name: &str) -> Option<u8> {
        self.armies.get(name).copied()
    }

    fn building(&self, name: &str) -> Option<u16> {
        self.buildings.get(name).copied()
    }
}

fn one_error(message: String) -> Report {
    let mut r = Report::default();
    r.error("", None, message);
    r
}

fn list<'a>(names: impl Iterator<Item = &'a String>) -> String {
    let v: Vec<&str> = names.map(String::as_str).collect();
    if v.is_empty() {
        "none".into()
    } else {
        v.join(", ")
    }
}

/// The package's unit ids: each unit's id is `UNIT_BASE` plus its position in `pkg.units`. A
/// duplicate id keeps its first position (`check` reports the duplicate).
pub fn unit_ids(pkg: &Package) -> BTreeMap<&str, u32> {
    let mut ids = BTreeMap::new();
    for (i, u) in pkg.units.iter().enumerate() {
        ids.entry(u.value.id.as_str()).or_insert(UNIT_BASE + i as u32);
    }
    ids
}

/// The package's item ids: `ITEM_BASE` plus the position in `pkg.items`.
pub fn item_ids(pkg: &Package) -> BTreeMap<&str, u32> {
    let mut ids = BTreeMap::new();
    for (i, it) in pkg.items.iter().enumerate() {
        ids.entry(it.value.id.as_str()).or_insert(ITEM_BASE + i as u32);
    }
    ids
}

/// The texts of a package in one language, with the other languages as the fallback.
struct Texts<'a> {
    pkg: &'a Package,
    lang: &'a str,
}

impl Texts<'_> {
    fn get(&self, key: &str) -> Result<String, String> {
        let preferred = self.pkg.texts.get(self.lang).and_then(|t| t.get(key));
        let any = || self.pkg.texts.values().find_map(|t| t.get(key));
        preferred.or_else(any).cloned().ok_or_else(|| format!("text key `{key}` has no text"))
    }
}

/// The unit with local id `id`, as its base game's definition changed by the package, and the
/// `GlobalIndex` of the base game unit its pictures come from.
fn unit_def(pkg: &Package, ids: &BTreeMap<&str, u32>, base: &Base, texts: &Texts, id: &str, depth: u32) -> Result<(UnitDef, u32), String> {
    if depth > UNIT_LIMIT as u32 {
        return Err(format!("base cycle through `{id}`"));
    }
    let spec: &Located<UnitSpec> = pkg.units.iter().find(|u| u.value.id == id).ok_or_else(|| format!("no unit `{id}`"))?;
    let s = &spec.value;
    let (mut def, root) = match &s.base {
        Ref::Dt { kind: DtKind::Unit, alias } => {
            let gi = aliases::resolve(DtKind::Unit, alias).ok_or_else(|| format!("unknown base `dt:{alias}`"))?;
            let def = base.units.iter().find(|d| d.id == gi).cloned().ok_or_else(|| format!("the install has no unit {gi} (`dt:{alias}`)"))?;
            (def, gi)
        }
        Ref::Local { id: local } => unit_def(pkg, ids, base, texts, local, depth + 1)?,
        other => return Err(format!("base `{other}` is not a unit")),
    };
    def.id = ids.get(id).copied().ok_or_else(|| format!("no id for `{id}`"))?;
    def.name = texts.get(&s.name)?;
    if let Some(d) = &s.description {
        def.description = texts.get(d)?;
    }
    let set = |field: &mut i32, v: Option<i32>| {
        if let Some(v) = v {
            *field = v;
        }
    };
    set(&mut def.cost, s.cost);
    set(&mut def.hits, s.hits);
    set(&mut def.attack_blow, s.attack_blow);
    set(&mut def.attack_shot, s.attack_shot);
    set(&mut def.magic_power, s.magic_power);
    set(&mut def.defence_blow, s.defence_blow);
    set(&mut def.defence_shot, s.defence_shot);
    set(&mut def.protect_life, s.protect_life);
    set(&mut def.protect_death, s.protect_death);
    set(&mut def.protect_elemental, s.protect_elemental);
    set(&mut def.initiative, s.initiative);
    set(&mut def.manevres, s.manevres);
    set(&mut def.regen, s.regen);
    set(&mut def.vampirism, s.vampirism);
    if let Some(v) = s.evasion {
        def.evasion = Some(v);
    }
    Ok((def, root))
}

/// The item with local id `id`, as its base game's definition changed by the package.
fn item_def(pkg: &Package, ids: &BTreeMap<&str, u32>, base: &Base, texts: &Texts, id: &str, depth: u32) -> Result<ArtefactDef, String> {
    if depth > ITEM_LIMIT as u32 {
        return Err(format!("base cycle through `{id}`"));
    }
    let spec: &Located<ItemSpec> = pkg.items.iter().find(|i| i.value.id == id).ok_or_else(|| format!("no item `{id}`"))?;
    let s = &spec.value;
    let mut def = match &s.base {
        Ref::Dt { kind: DtKind::Item, alias } => {
            let gi = aliases::resolve(DtKind::Item, alias).ok_or_else(|| format!("unknown base `dt:item/{alias}`"))?;
            base.items.iter().find(|d| d.id == gi).cloned().ok_or_else(|| format!("the install has no item {gi} (`dt:item/{alias}`)"))?
        }
        Ref::Local { id: local } => item_def(pkg, ids, base, texts, local, depth + 1)?,
        other => return Err(format!("base `{other}` is not an item")),
    };
    def.id = ids.get(id).copied().ok_or_else(|| format!("no id for `{id}`"))?;
    def.name = texts.get(&s.name)?;
    if let Some(d) = &s.description {
        def.description = texts.get(d)?;
    }
    if let Some(v) = s.cost {
        def.cost = v;
    }
    Ok(def)
}

/// A unit reference in a map: a `dt:` unit or one of the package's units. Other packages' units
/// are not loaded yet.
pub fn unit_of(text: &str, ids: &BTreeMap<&str, u32>) -> Result<u32, String> {
    match Ref::parse(text)? {
        Ref::Dt { kind: DtKind::Unit, alias } => aliases::resolve(DtKind::Unit, &alias).ok_or_else(|| format!("unknown unit `dt:{alias}`")),
        Ref::Dt { kind, .. } => Err(format!("`{text}` is not a unit: a {} cannot be a troop", kind.word())),
        Ref::Local { id } => ids.get(id.as_str()).copied().ok_or_else(|| format!("no unit `{id}` in the package")),
        Ref::Package { .. } => Err("units of other packages are not loaded yet (slice 1): use a dt: unit or a unit of this package".into()),
    }
}

/// The three hero classes, by their name in `map.toml`, in the header's order.
pub const CLASSES: [&str; 3] = ["knight", "archmage", "ranger"];

/// The factions by their name, with their codes (`player` is 1).
pub fn faction_code(name: &str) -> Option<u8> {
    ["player", "ally", "neighbour", "enemy"].iter().position(|f| *f == name).map(|i| i as u8 + 1)
}

/// The building types by name, in the engine's order.
pub const BUILDING_NAMES: [&str; 16] = [
    "palace",
    "town",
    "village",
    "castle",
    "fort",
    "tavern",
    "market",
    "church",
    "smithy",
    "shipyard",
    "altar",
    "dungeon_entrance",
    "ruins",
    "stone_bridge",
    "wooden_bridge",
    "obelisk",
];

/// The building type code of a name.
pub fn building_code(name: &str) -> Option<u8> {
    BUILDING_NAMES.iter().position(|b| *b == name).map(|i| i as u8)
}

/// Object kinds: their class and default sprite (`dtm-format.md` §5).
pub const OBJECT_KINDS: [(&str, u8, u8); 6] = [("hills", 1, 10), ("mountains", 5, 10), ("stones", 8, 10), ("trees", 9, 0), ("dead_trees", 10, 108), ("thicket", 11, 0)];

/// Army models by name: the map model code and the behaviour style (`dtm-format.md`, army byte 59).
pub const ARMY_MODELS: [(&str, u8, u8); 7] = [
    ("feudal", 4, 0),
    ("bandits", 5, 1),
    ("peasants", 6, 2),
    ("inactive", 7, 0),
    ("necromancer", 10, 0),
    ("ghosts", 11, 0),
    ("zombies", 12, 0),
];

/// Ships by name: the code of army byte 72.
pub fn ship_code(name: &str) -> Option<u8> {
    ["none", "hero", "pirate", "merchant"].iter().position(|s| *s == name).map(|i| i as u8)
}

/// Converts a troop list to its six slots. A count of 0 leaves the slot empty.
fn troop_slots(list: &[TroopSpec], ids: &BTreeMap<&str, u32>) -> Result<[Troop; TROOP_SLOTS], String> {
    let mut slots = [Troop::default(); TROOP_SLOTS];
    for (slot, t) in slots.iter_mut().zip(list) {
        let unit = unit_of(&t.unit, ids)?;
        *slot = Troop { unit: u8::try_from(unit).map_err(|_| format!("unit {unit} does not fit a troop slot"))?, level: t.level.saturating_sub(1), count: t.count };
    }
    Ok(slots)
}

/// The scenario of one map of the package.
fn scenario_of(pkg: &Package, ids: &BTreeMap<&str, u32>, texts: &Texts, file: &str, map_id: &str, m: &MapFile) -> Result<Scenario, Report> {
    let mut r = Report::default();
    let palette = match Palette::with_overrides(&m.terrain_palette) {
        Ok(p) => p,
        Err(e) => return Err(one_error(format!("{file}: {e}"))),
    };
    let png = pkg.files.get(&format!("maps/{map_id}/terrain.png")).ok_or_else(|| one_error(format!("maps/{map_id}/terrain.png is missing")))?;
    let terrain = terrain::decode(png, &palette).map_err(|ps| {
        let mut r = Report::default();
        for p in ps {
            r.error(&format!("maps/{map_id}/terrain.png"), None, p.message);
        }
        r
    })?;
    let (year, month, day) = parse_date(&m.start_date).map_err(|e| one_error(format!("{file}: {e}")))?;
    let start_time = GameDate { year, month, day, hour: 0, minute: 0 }.to_minutes();
    let text = |key: &str| texts.get(key).map_err(|e| one_error(format!("{file}: {e}")));
    // Every campaign hand-over byte is set: a package's campaign masks what carries (`[carry]` in
    // `campaign.toml`, `rules::campaign`), so the header takes all of it.
    let mut header = Header { width: terrain.width, height: terrain.height, start_time, relations: m.relations, carry_over: [1; 7], ..Header::default() };

    // The hero: each offered class gets the same start, gold, mana and troops.
    let troops = troop_slots(&m.hero.troops, ids).map_err(|e| one_error(format!("{file}: hero: {e}")))?;
    for class in &m.hero.classes {
        let k = CLASSES.iter().position(|c| c == class).ok_or_else(|| one_error(format!("{file}: hero class `{class}` is unknown")))?;
        header.heroes[k] = HeroPreset {
            gold: m.hero.gold,
            mana: m.hero.mana,
            x: m.hero.cell[0] as u16,
            y: m.hero.cell[1] as u16,
            troops,
            ..HeroPreset::default()
        };
    }

    let mut objects: Vec<MapObject> = Vec::new();
    for o in &m.object {
        let o = o.get_ref();
        let (_, class, sprite) = *OBJECT_KINDS.iter().find(|k| k.0 == o.kind).ok_or_else(|| one_error(format!("{file}: object kind `{}`", o.kind)))?;
        objects.push(MapObject { x: o.cell[0] as u16, y: o.cell[1] as u16, sprite: o.sprite.unwrap_or(sprite), class });
    }
    objects.sort_by_key(|o| (o.y, o.x));

    let mut buildings: Vec<Building> = Vec::new();
    for b in &m.building {
        let b: &BuildingSpec = b.get_ref();
        let faction = faction_code(&b.faction).ok_or_else(|| one_error(format!("{file}: building `{}`: faction `{}`", b.id, b.faction)))?;
        let kind = building_code(&b.kind).ok_or_else(|| one_error(format!("{file}: building `{}`: type `{}`", b.id, b.kind)))?;
        let garrison = troop_slots(&b.garrison, ids).map_err(|e| one_error(format!("{file}: building `{}`: {e}", b.id)))?;
        let mut barracks = [RecruitSlot::default(); TROOP_SLOTS];
        for (slot, rec) in barracks.iter_mut().zip(&b.recruit) {
            let unit = unit_of(&rec.unit, ids).map_err(|e| one_error(format!("{file}: building `{}`: {e}", b.id)))?;
            *slot = RecruitSlot { unit: unit as u8, start_count: rec.start, max_count: rec.max };
        }
        buildings.push(Building {
            x: b.cell[0] as u16,
            y: b.cell[1] as u16,
            kind,
            picture_type: kind,
            size_x: 1,
            size_y: 1,
            faction,
            relations: header.relations[faction as usize - 1],
            gold_per_day: b.income,
            garrison,
            barracks,
            has_barracks: u8::from(!b.recruit.is_empty()),
            name: text(&b.name)?,
            description: match &b.description {
                Some(d) => text(d)?,
                None => String::new(),
            },
            ..Building::default()
        });
    }

    let mut armies: Vec<Army> = Vec::new();
    let mut named: Vec<NamedCharacter> = Vec::new();
    for (i, a) in m.army.iter().enumerate() {
        let a: &ArmySpec = a.get_ref();
        let faction = faction_code(&a.faction).ok_or_else(|| one_error(format!("{file}: army `{}`: faction `{}`", a.id, a.faction)))?;
        let (_, model, behaviour) = *ARMY_MODELS.iter().find(|m| m.0 == a.model).ok_or_else(|| one_error(format!("{file}: army `{}`: model `{}`", a.id, a.model)))?;
        let ship = ship_code(a.ship.as_deref().unwrap_or("none")).ok_or_else(|| one_error(format!("{file}: army `{}`: ship", a.id)))?;
        let troops_of = troop_slots(&a.troops, ids).map_err(|e| one_error(format!("{file}: army `{}`: {e}", a.id)))?;
        let first = troops_of.iter().find(|t| !t.is_empty()).copied().unwrap_or_default();
        let (leader_unit, leader_level, leader_name, named_character) = match &a.leader {
            Some(l) => {
                let unit = unit_of(&l.unit, ids).map_err(|e| one_error(format!("{file}: army `{}`: leader: {e}", a.id)))?;
                let name = text(&l.name)?;
                named.push(NamedCharacter { unit: unit as u8, name: name.clone() });
                (unit as u8, l.level.saturating_sub(1), name, named.len() as u8)
            }
            None => (first.unit, first.level, String::new(), 0),
        };
        let army = Army {
            x: a.cell[0] as u16,
            y: a.cell[1] as u16,
            id: (i + 1) as u8,
            model,
            faction,
            relations: header.relations[faction as usize - 1],
            troops: troops_of,
            leader_unit,
            leader_level,
            leader_name,
            named_character,
            name: text(&a.name)?,
            gold_income: a.gold as u16,
            ship,
            behaviour,
            inactive: u8::from(a.model == "inactive"),
            garrison_strength: 50,
            exp_correction: 100,
            ..Army::default()
        };
        armies.push(army);
    }

    let mut points: Vec<Point> = Vec::new();
    for (i, p) in m.point.iter().enumerate() {
        let p = p.get_ref();
        points.push(Point {
            x: p.cell[0] as u16,
            y: p.cell[1] as u16,
            id: (i + 1) as u8,
            model: if p.active { 8 } else { 9 },
            radius: p.radius,
            active: u8::from(p.active),
            ..Point::default()
        });
    }

    // The victory: one event, scanned globally (its conditions), or local at a point (reach).
    let victory = m.victory.get_ref();
    let mut conditions = EventConditions::default();
    let mut kind = 1u8;
    match victory {
        Victory::DefeatAll { .. } => {
            let enemies: Vec<u8> = armies.iter().filter(|a| a.faction == 4).map(|a| a.id).collect();
            conditions.beaten_check = 1;
            conditions.beaten_armies = [enemies.first().copied().unwrap_or(0), enemies.get(1).copied().unwrap_or(0)];
        }
        Victory::DefeatArmy { army, .. } => {
            let idx = m.army.iter().position(|s| s.get_ref().id == *army).ok_or_else(|| one_error(format!("{file}: victory: no army `{army}`")))?;
            conditions.beaten_check = 1;
            conditions.beaten_armies = [(idx + 1) as u8, 0];
        }
        Victory::Capture { building, .. } => {
            // Buildings keep the file's order: the id is the position + 1.
            let idx = m.building.iter().position(|b| b.get_ref().id == *building).ok_or_else(|| one_error(format!("{file}: victory: no building `{building}`")))?;
            conditions.buildings_check = 1;
            conditions.buildings = [(idx + 1) as u8, 0, 0];
            conditions.buildings_owner = [1, 0, 0];
        }
        Victory::Reach { cell, .. } => {
            kind = 2;
            points.push(Point {
                x: cell[0] as u16,
                y: cell[1] as u16,
                id: (points.len() + 1) as u8,
                model: 9,
                event_slots: std::array::from_fn(|k| u16::from(k == 0)),
                event_count: 1,
                ..Point::default()
            });
        }
    }
    let message = match victory.message() {
        Some(key) => text(key)?,
        None => String::new(),
    };
    let title = text(&m.title)?;
    let event = Event {
        kind,
        start_time,
        once: 1,
        conditions,
        results: EventResults { picture: 201, ..EventResults::default() },
        title: format!("{title}#VICTORY"),
        message,
        ..Event::default()
    };
    header.victory_event = 1;
    // The header keeps the named characters' units in its slots (`Scenario::parse_payload` reads
    // them back from there).
    for (slot, nc) in header.named_character_slots.iter_mut().zip(&named) {
        *slot = nc.unit;
    }
    if named.len() > NAMED_LIMIT {
        r.error(file, None, format!("{} named characters: a scenario holds {NAMED_LIMIT}", named.len()));
        return Err(r);
    }
    Ok(Scenario {
        header,
        terrain: terrain.cells,
        objects,
        buildings,
        armies,
        points,
        events: vec![event],
        title,
        description: match &m.description {
            Some(d) => text(d)?,
            None => String::new(),
        },
        campaign_name: String::new(),
        next_map: String::new(),
        named_characters: named,
        scenario_picture: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{BuildingType, Event, HeroPreset, Scenario};

    #[test]
    fn ids_start_after_the_base_game() {
        assert_eq!(UNIT_BASE as usize + UNIT_LIMIT - 1, 254, "the last unit id is 254 (255 is `any unit`)");
        assert_eq!(ITEM_BASE as usize + ITEM_LIMIT - 1, 255, "item ids fit a byte");
    }

    use crate::dt::data::{ArtefactDef, UnitDef};
    use crate::pack::files::Files;
    use crate::pack::terrain::{encode_rgb, DEFAULT_PALETTE};
    use crate::rules::content::Content;

    /// The demo's units and items as the base game (no install needed).
    fn demo() -> (Vec<UnitDef>, Vec<ArtefactDef>) {
        let c = Content::builtin();
        (c.units.clone(), c.items.clone())
    }

    /// Any frame: a unit's picture is its root base's frame (`picture(1)` is 10).
    fn picture(id: u32) -> Option<usize> {
        Some(id as usize * 10)
    }

    const MANIFEST: &str = "id = \"demo\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"title\"\nauthor = \"A\"\nlicense = \"MIT\"\n";

    /// The texts of the package: the map's, and a name for each unit id and the item.
    fn texts(unit_ids: &[&str]) -> String {
        let mut t = String::from("title = \"Map\"\ndescription = \"About\"\n[town]\nname = \"Town\"\n[army]\nname = \"Army\"\n[win]\nmessage = \"Won\"\n[leader]\nname = \"Leader\"\n[item.i]\nname = \"Item\"\n");
        for id in unit_ids {
            t.push_str(&format!("[unit.{id}]\nname = \"Unit {id}\"\n"));
        }
        t
    }

    /// A map with hero troops `troops` (inline table items), one enemy army `e`, the extra text
    /// `rest` (buildings, armies, victory) and a plain 50×50 grass picture.
    fn map_toml(troops: &str, rest: &str) -> String {
        // A map needs a victory: a reach at (20, 20) unless the test gives one.
        let victory = if rest.contains("[victory]") { "" } else { "[victory]\nkind = \"reach\"\ncell = [20, 20]\n" };
        format!(
            "title = \"title\"\ndescription = \"description\"\nstart_date = \"1300-04-01\"\nrelations = [[3,3,0,-3],[3,3,0,-3],[0,0,3,-3],[-3,-3,-3,3]]\n\n[hero]\ncell = [5, 5]\nclasses = [\"knight\", \"ranger\"]\ngold = 100\ntroops = [{troops}]\n\n{rest}{victory}"
        )
    }

    fn enemy_army() -> &'static str {
        "[[army]]\nid = \"e\"\nname = \"army.name\"\ncell = [30, 30]\nfaction = \"enemy\"\nmodel = \"bandits\"\ntroops = [{ unit = \"dt:bandit\", count = 2 }]\n\n"
    }

    /// A package with units `(id, base)`, the item `i`, and `map` (its `maps/m/map.toml`).
    fn package(units: &[(&str, &str)], map: &str) -> Package {
        let mut f = Files::default();
        f.insert("package.toml", MANIFEST);
        let ids: Vec<&str> = units.iter().map(|u| u.0).collect();
        f.insert("texts/en.toml", texts(&ids));
        let mut us = String::new();
        for (id, base) in units {
            us.push_str(&format!("[[unit]]\nid = \"{id}\"\nbase = \"{base}\"\nname = \"unit.{id}.name\"\n\n"));
        }
        f.insert("units/a.toml", us);
        f.insert("items/i.toml", "[[item]]\nid = \"i\"\nbase = \"dt:item/viking_sword\"\nname = \"item.i.name\"\ncost = 500\n");
        f.insert("maps/m/map.toml", map);
        f.insert("maps/m/terrain.png", encode_rgb(50, 50, |_, _| DEFAULT_PALETTE[6].1));
        Package::parse(f).0
    }

    fn compile_demo(pkg: &Package) -> Result<Compiled, Report> {
        let (units, items) = demo();
        let picture: &dyn Fn(u32) -> Option<usize> = &picture;
        compile_with(pkg, &Base { units: &units, items: &items, picture }, "en", "m")
    }

    #[test]
    fn units_and_items_get_ids_from_200_and_keep_their_base() {
        let pkg = package(&[("a", "dt:knight"), ("b", "a")], &map_toml("{ unit = \"a\", count = 2 }", ""));
        let c = compile_demo(&pkg).unwrap_or_else(|r| panic!("{}", r.to_text()));
        assert_eq!(c.units.iter().map(|u| u.id).collect::<Vec<_>>(), vec![200, 201]);
        assert_eq!(c.units[1].name, "Unit b", "the name comes from the texts");
        assert_eq!(c.units[1].hits, c.units[0].hits, "b is based on a, which is based on the knight");
        assert_eq!(c.pictures, vec![(200, 10), (201, 10)], "both draw the knight's picture (frame of unit 1)");
        assert_eq!(c.items.len(), 1);
        assert_eq!(c.items[0].id, 200);
        assert_eq!(c.items[0].cost, 500, "the item's cost overrides its base");
        assert_eq!(c.scenario.header.heroes[0].troops[0].unit, 200, "a troop slot holds the package's id");
    }

    #[test]
    fn a_package_has_at_most_55_units() {
        let ids: Vec<String> = (0..56).map(|i| format!("u{i}")).collect();
        let units: Vec<(&str, &str)> = ids.iter().map(|id| (id.as_str(), "dt:knight")).collect();
        let pkg = package(&units[..55], &map_toml("{ unit = \"u0\", count = 2 }", ""));
        let c = compile_demo(&pkg).unwrap_or_else(|r| panic!("{}", r.to_text()));
        assert_eq!(c.units.last().map(|u| u.id), Some(254), "the 55th unit is 254");
        let pkg = package(&units, &map_toml("{ unit = \"u0\", count = 2 }", ""));
        let report = compile_demo(&pkg).unwrap_err();
        assert!(report.to_text().contains("56 units: 55 at most"), "{}", report.to_text());
    }

    #[test]
    fn a_package_unit_in_the_wrong_kind_of_slot_is_an_error() {
        let pkg = package(&[("a", "dt:knight")], &map_toml("{ unit = \"dt:knight\", count = 2 }", ""));
        let report = compile_demo(&pkg).unwrap_err();
        assert!(report.to_text().contains("is a hero class"), "{}", report.to_text());
    }

    /// The victory event of a compiled map.
    fn victory_of(c: &Compiled) -> &Event {
        &c.scenario.events[0]
    }

    #[test]
    fn defeat_all_beats_the_enemy_armies() {
        let rest = format!("{}[victory]\nkind = \"defeat_all\"\nmessage = \"win.message\"\n", enemy_army());
        let c = compile_demo(&package(&[], &map_toml("", &rest))).unwrap_or_else(|r| panic!("{}", r.to_text()));
        let e = victory_of(&c);
        assert_eq!(c.scenario.header.victory_event, 1);
        assert_eq!((e.kind, e.conditions.beaten_check), (1, 1));
        assert_eq!(e.conditions.beaten_armies, [1, 0]);
        assert_eq!(e.message, "Won");
        assert!(e.title.ends_with("#VICTORY"), "the victory look: {}", e.title);
    }

    #[test]
    fn defeat_army_beats_one_army() {
        let rest = format!("{}[victory]\nkind = \"defeat_army\"\narmy = \"e\"\n", enemy_army());
        let c = compile_demo(&package(&[], &map_toml("", &rest))).unwrap_or_else(|r| panic!("{}", r.to_text()));
        assert_eq!(victory_of(&c).conditions.beaten_armies, [1, 0]);
    }

    #[test]
    fn capture_wants_the_building_to_be_the_players() {
        let building = "[[building]]\nid = \"t\"\ntype = \"town\"\nname = \"town.name\"\ncell = [7, 7]\nfaction = \"enemy\"\n\n";
        let rest = format!("{building}[victory]\nkind = \"capture\"\nbuilding = \"t\"\n");
        let c = compile_demo(&package(&[], &map_toml("", &rest))).unwrap_or_else(|r| panic!("{}", r.to_text()));
        let e = victory_of(&c);
        assert_eq!(e.conditions.buildings_check, 1);
        assert_eq!(e.conditions.buildings, [1, 0, 0], "the first building of the file");
        assert_eq!(e.conditions.buildings_owner, [1, 0, 0], "owned by the player");
        assert_eq!(c.scenario.buildings[0].garrison[0].count, 0);
        let rest = rest.replace("faction = \"enemy\"", "faction = \"player\"");
        let report = compile_demo(&package(&[], &map_toml("", &rest))).unwrap_err();
        assert!(report.to_text().contains("from the start"), "{}", report.to_text());
    }

    #[test]
    fn reach_is_an_event_point_with_a_local_event() {
        let rest = format!("{}[victory]\nkind = \"reach\"\ncell = [20, 21]\n", enemy_army());
        let c = compile_demo(&package(&[], &map_toml("", &rest))).unwrap_or_else(|r| panic!("{}", r.to_text()));
        let e = victory_of(&c);
        assert_eq!(e.kind, 2, "local: only the point fires it, not every scan");
        assert_eq!(e.conditions.beaten_check, 0);
        let p = c.scenario.points.last().expect("the reach point");
        assert_eq!((p.x, p.y, p.event_count, p.event_slots[0]), (20, 21, 1, 1));
        assert_eq!(p.events().collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn the_compiled_scenario_survives_the_payload() {
        let leader = "[[army]]\nid = \"l\"\nname = \"army.name\"\ncell = [31, 31]\nfaction = \"enemy\"\nmodel = \"feudal\"\ntroops = [{ unit = \"a\", count = 1 }]\nleader = { unit = \"dt:swordsman\", level = 3, name = \"leader.name\" }\n\n";
        let rest = format!(
            "{leader}{}[[building]]\nid = \"t\"\ntype = \"town\"\nname = \"town.name\"\ncell = [7, 7]\nfaction = \"player\"\ngarrison = [{{ unit = \"a\", count = 2 }}]\nrecruit = [{{ unit = \"a\", start = 1, max = 3 }}]\n\n[[object]]\nkind = \"trees\"\ncell = [3, 4]\n\n[[point]]\ncell = [9, 9]\nradius = 3\nactive = true\n\n[victory]\nkind = \"defeat_all\"\n",
            enemy_army()
        );
        let pkg = package(&[("a", "dt:knight")], &map_toml("{ unit = \"a\", level = 2, count = 3 }", &rest));
        let c = compile_demo(&pkg).unwrap_or_else(|r| panic!("{}", r.to_text()));
        let s = c.scenario;
        let back = Scenario::parse_payload(&s.to_payload()).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.header.heroes[0].troops[0].level, 1, "level 2 is stored 0-based");
        assert_eq!(back.header.heroes[1], HeroPreset::default(), "the archmage is not offered: no preset");
        assert_eq!(back.title, "Map");
        assert_eq!(back.named_characters.len(), 1, "the named leader survives the payload");
    }

    #[test]
    fn a_map_that_names_no_such_map_is_an_error() {
        let pkg = package(&[], &map_toml("", &format!("{}[victory]\nkind = \"defeat_all\"\n", enemy_army())));
        let (units, items) = demo();
        let picture: &dyn Fn(u32) -> Option<usize> = &picture;
        let report = compile_with(&pkg, &Base { units: &units, items: &items, picture }, "en", "nope").unwrap_err();
        assert!(report.to_text().contains("no map `nope`"), "{}", report.to_text());
    }

    #[test]
    fn faction_and_building_names() {
        assert_eq!(faction_code("player"), Some(1));
        assert_eq!(faction_code("enemy"), Some(4));
        assert_eq!(faction_code("neutral"), None);
        assert_eq!(building_code("town"), Some(1));
        assert_eq!(building_code("wooden_bridge"), Some(14));
        assert_eq!(BuildingType::from_code(14), Some(BuildingType::WoodenBridge));
    }
}
