//! JSON export of a `.DTm` map: the `razdor-dtm-json` format, version 1 (`docs/flows/testing-and-tools.md`).
//!
//! Meant for converters to other games, so the layout is stable and versioned. Numbers are the
//! file's raw values unless a field says otherwise; names are snake_case; ids are 1-based, as the
//! file and its events use them. Pictures (the scenario picture, event pictures) and every
//! `unknown_*` field of the file are never written.
//!
//! Pure data: no macroquad and no install. The DTO structs here are private so that the
//! `.DTm` types in [`super::dtm`] stay free of serde.

use serde::Serialize;
use serde_json::Value;

use super::dtm::{
    Army, ArmyModel, Building, BuildingType, Event, EventConditions, EventKind, EventResults, HeroPreset, MapObject,
    Point, RecruitSlot, Scenario, Surface, Troop,
};

/// The `format` value of every export.
pub const FORMAT: &str = "razdor-dtm-json";
/// The `version` value of every export. Bumped on any change of the layout.
pub const VERSION: u32 = 1;

const HERO_NAMES: [&str; 3] = ["knight", "archmage", "ranger"];
const UNKNOWN: &str = "unknown";
const OTHER: &str = "other";

fn surface_name(s: Surface) -> &'static str {
    match s {
        Surface::ShallowsFords => "shallows_fords",
        Surface::CoastalWater => "coastal_water",
        Surface::DeepSea => "deep_sea",
        Surface::LavaFields => "lava_fields",
        Surface::Road => "road",
        Surface::GrassLowland => "grass_lowland",
        Surface::GrassPlain => "grass_plain",
        Surface::DryPlain => "dry_plain",
        Surface::Marsh => "marsh",
        Surface::ImpassableSwamp => "impassable_swamp",
        Surface::SandDunes => "sand_dunes",
        Surface::ClaySoil => "clay_soil",
        Surface::StonySoil => "stony_soil",
        Surface::ScorchedLand => "scorched_land",
        Surface::SnowyGround => "snowy_ground",
        Surface::ImpassableSnowdrifts => "impassable_snowdrifts",
    }
}

fn building_name(b: BuildingType) -> &'static str {
    match b {
        BuildingType::Palace => "palace",
        BuildingType::Town => "town",
        BuildingType::Village => "village",
        BuildingType::Castle => "castle",
        BuildingType::Fort => "fort",
        BuildingType::Tavern => "tavern",
        BuildingType::Market => "market",
        BuildingType::Church => "church",
        BuildingType::Smithy => "smithy",
        BuildingType::Shipyard => "shipyard",
        BuildingType::Altar => "altar",
        BuildingType::DungeonEntrance => "dungeon_entrance",
        BuildingType::Ruins => "ruins",
        BuildingType::StoneBridge => "stone_bridge",
        BuildingType::WoodenBridge => "wooden_bridge",
        BuildingType::Obelisk => "obelisk",
    }
}

fn army_model_name(m: ArmyModel) -> &'static str {
    match m {
        ArmyModel::HeroKnight => "hero_knight",
        ArmyModel::HeroArchmage => "hero_archmage",
        ArmyModel::HeroRanger => "hero_ranger",
        ArmyModel::Feudal => "feudal",
        ArmyModel::Bandits => "bandits",
        ArmyModel::Peasants => "peasants",
        ArmyModel::Inactive => "inactive",
        ArmyModel::Lantern => "lantern",
        ArmyModel::EventPoint => "event_point",
        ArmyModel::Necromancer => "necromancer",
        ArmyModel::Ghosts => "ghosts",
        ArmyModel::Zombies => "zombies",
    }
}

fn event_kind_name(k: EventKind) -> &'static str {
    match k {
        EventKind::Global => "global",
        EventKind::Local => "local",
        EventKind::Quest => "quest",
        EventKind::Rumour => "rumour",
    }
}

/// Name of an object class (the legend's `object_classes` lists the six named ones).
fn object_class_name(class: u8) -> &'static str {
    match class {
        1 => "hills",
        5 => "mountains",
        8 => "stones",
        9 => "trees",
        10 => "dead_trees",
        11 => "thicket",
        _ => OTHER,
    }
}

/// Non-zero values of a slot list, in order.
fn nonzero<T: Copy + Default + PartialEq>(v: &[T]) -> Vec<T> {
    v.iter().copied().filter(|x| *x != T::default()).collect()
}

#[derive(Serialize)]
struct Export<'a> {
    format: &'static str,
    version: u32,
    source_file: &'a str,
    width: u32,
    height: u32,
    info: Info<'a>,
    heroes: Vec<HeroOut>,
    legend: Legend,
    terrain: TerrainOut,
    objects: Vec<ObjectOut>,
    buildings: Vec<BuildingOut<'a>>,
    armies: Vec<ArmyOut<'a>>,
    points: Vec<PointOut>,
    events: Vec<EventOut<'a>>,
    named_characters: Vec<NamedOut<'a>>,
    has_scenario_picture: bool,
}

#[derive(Serialize)]
struct Info<'a> {
    title: &'a str,
    description: &'a str,
    campaign_name: &'a str,
    next_map: &'a str,
    /// Raw header byte 0x10F: 0 standalone, 1 campaign start, 2 campaign continuation.
    scenario_kind: u8,
    /// Raw start time in minutes since year 0 (0 = unset).
    start_time: u32,
    /// 1-based event ids, 0 = none.
    victory_event: u16,
    defeat_event: u16,
    /// Rows and columns: player, ally, neighbour, enemy; values −3..3.
    relations: [[i8; 4]; 4],
    generator_seed: u32,
}

#[derive(Serialize)]
struct TroopOut {
    unit: u8,
    level: u8,
    count: u8,
}

fn troops(ts: &[Troop]) -> Vec<TroopOut> {
    ts.iter().filter(|t| !t.is_empty()).map(|t| TroopOut { unit: t.unit, level: t.level, count: t.count }).collect()
}

#[derive(Serialize)]
struct HeroOut {
    archetype: &'static str,
    gold: u32,
    mana: u32,
    start_building: u8,
    x: u16,
    y: u16,
    troops: Vec<TroopOut>,
    artifacts: Vec<u8>,
    spells: Vec<u8>,
}

fn hero_out(name: &'static str, h: &HeroPreset) -> HeroOut {
    HeroOut {
        archetype: name,
        gold: h.gold,
        mana: h.mana,
        start_building: h.start_building,
        x: h.x,
        y: h.y,
        troops: troops(&h.troops),
        artifacts: nonzero(&h.artifacts),
        spells: nonzero(&h.spells),
    }
}

#[derive(Serialize)]
struct CodeName {
    code: u8,
    name: &'static str,
}

#[derive(Serialize)]
struct ObjectClasses {
    #[serde(rename = "1")]
    one: &'static str,
    #[serde(rename = "5")]
    five: &'static str,
    #[serde(rename = "8")]
    eight: &'static str,
    #[serde(rename = "9")]
    nine: &'static str,
    #[serde(rename = "10")]
    ten: &'static str,
    #[serde(rename = "11")]
    eleven: &'static str,
}

#[derive(Serialize)]
struct Legend {
    surfaces: Vec<CodeName>,
    object_classes: ObjectClasses,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Rows {
    /// One lowercase hex digit per cell.
    Hex(Vec<String>),
    /// Used when a code is above 15 and has no hex digit.
    Numbers(Vec<Vec<u8>>),
}

#[derive(Serialize)]
struct TerrainOut {
    encoding: &'static str,
    rows: Rows,
}

fn terrain_out(s: &Scenario) -> TerrainOut {
    let w = s.width() as usize;
    let h = s.height() as usize;
    let rows: Vec<&[u8]> = if w == 0 { Vec::new() } else { s.terrain.chunks(w).take(h).collect() };
    if s.terrain.iter().any(|&c| c > 15) {
        TerrainOut { encoding: "numbers", rows: Rows::Numbers(rows.iter().map(|r| r.to_vec()).collect()) }
    } else {
        let hex: Vec<String> = rows
            .iter()
            .map(|r| r.iter().map(|&c| char::from_digit(u32::from(c), 16).unwrap_or('0')).collect())
            .collect();
        TerrainOut { encoding: "hex-rows", rows: Rows::Hex(hex) }
    }
}

#[derive(Serialize)]
struct ObjectOut {
    x: u16,
    y: u16,
    sprite: u8,
    class: u8,
    class_name: &'static str,
}

fn object_out(o: &MapObject) -> ObjectOut {
    ObjectOut { x: o.x, y: o.y, sprite: o.sprite, class: o.class, class_name: object_class_name(o.class) }
}

#[derive(Serialize)]
struct RecruitOut {
    unit: u8,
    start_count: u8,
    max_count: u8,
}

#[derive(Serialize)]
struct BuildingOut<'a> {
    /// 1-based record index: the id events and armies use (`Scenario::building`).
    index: usize,
    x: u16,
    y: u16,
    size_x: u8,
    size_y: u8,
    kind: u8,
    kind_name: &'static str,
    picture_type: u8,
    picture_variant: u8,
    name: &'a str,
    owner_name: &'a str,
    description: &'a str,
    /// Owner army id; 255 = the neutral owner.
    owner_army: u8,
    linked_building: u8,
    faction: u8,
    relations: [i8; 4],
    gold_per_day: u16,
    gold_max: u16,
    mana_per_day: u8,
    mana_max: u8,
    price_min: u16,
    price_max: u16,
    garrison: Vec<TroopOut>,
    garrison_extra_defence: u8,
    garrison_ai_only: u8,
    has_barracks: u8,
    barracks: Vec<RecruitOut>,
    recruit_all_types: u8,
    spells_for_sale: Vec<u8>,
    random_artifacts_for_sale: u8,
    events: Vec<u16>,
    artifacts: Vec<u16>,
    start_for: [u8; 3],
}

fn building_out(index: usize, b: &Building) -> BuildingOut<'_> {
    BuildingOut {
        index,
        x: b.x,
        y: b.y,
        size_x: b.size_x,
        size_y: b.size_y,
        kind: b.kind,
        kind_name: b.building_type().map_or(UNKNOWN, building_name),
        picture_type: b.picture_type,
        picture_variant: b.picture_variant,
        name: &b.name,
        owner_name: &b.owner_name,
        description: &b.description,
        owner_army: b.owner_army,
        linked_building: b.linked_building,
        faction: b.faction,
        relations: b.relations,
        gold_per_day: b.gold_per_day,
        gold_max: b.gold_max,
        mana_per_day: b.mana_per_day,
        mana_max: b.mana_max,
        price_min: b.price_min,
        price_max: b.price_max,
        garrison: troops(&b.garrison),
        garrison_extra_defence: b.garrison_extra_defence,
        garrison_ai_only: b.garrison_ai_only,
        has_barracks: b.has_barracks,
        barracks: b
            .barracks
            .iter()
            .filter(|s| **s != RecruitSlot::default())
            .map(|s| RecruitOut { unit: s.unit, start_count: s.start_count, max_count: s.max_count })
            .collect(),
        recruit_all_types: b.recruit_all_types,
        spells_for_sale: nonzero(&b.spells_for_sale),
        random_artifacts_for_sale: b.random_artifacts_for_sale,
        events: b.events().collect(),
        artifacts: b.artifacts().collect(),
        start_for: b.start_for,
    }
}

#[derive(Serialize)]
struct ArmyOut<'a> {
    id: u8,
    x: u16,
    y: u16,
    model: u8,
    model_name: &'static str,
    name: &'a str,
    leader_name: &'a str,
    description: &'a str,
    leader_unit: u8,
    leader_level: u8,
    troops: Vec<TroopOut>,
    artifacts: Vec<u8>,
    named_character: u8,
    faction: u8,
    relations: [i8; 4],
    aggression: i8,
    behaviour: u8,
    patrols: u8,
    patrol_radius: u8,
    home_building: u8,
    inactive: u8,
    respawn_days: u8,
    respawn_all: u8,
    ship: u8,
    speed_correction: i8,
    gold_income: u16,
    no_money: u8,
    garrison_strength: u8,
    spell: u8,
    target_model: u8,
    hunts_player_only: u8,
    ignored_by_ai: u8,
}

fn army_out(a: &Army) -> ArmyOut<'_> {
    ArmyOut {
        id: a.id,
        x: a.x,
        y: a.y,
        model: a.model,
        model_name: a.model().map_or(UNKNOWN, army_model_name),
        name: &a.name,
        leader_name: &a.leader_name,
        description: &a.description,
        leader_unit: a.leader_unit,
        leader_level: a.leader_level,
        troops: troops(&a.troops),
        artifacts: nonzero(&a.artifacts),
        named_character: a.named_character,
        faction: a.faction,
        relations: a.relations,
        aggression: a.aggression,
        behaviour: a.behaviour,
        patrols: a.patrols,
        patrol_radius: a.patrol_radius,
        home_building: a.home_building,
        inactive: a.inactive,
        respawn_days: a.respawn_days,
        respawn_all: a.respawn_all,
        ship: a.ship,
        speed_correction: a.speed_correction,
        gold_income: a.gold_income,
        no_money: a.no_money,
        garrison_strength: a.garrison_strength,
        spell: a.spell,
        target_model: a.target_model,
        hunts_player_only: a.hunts_player_only,
        ignored_by_ai: a.ignored_by_ai,
    }
}

#[derive(Serialize)]
struct PointOut {
    id: u8,
    x: u16,
    y: u16,
    model: u8,
    radius: u8,
    active: u8,
    events: Vec<u16>,
}

fn point_out(p: &Point) -> PointOut {
    PointOut { id: p.id, x: p.x, y: p.y, model: p.model, radius: p.radius, active: p.active, events: p.events().collect() }
}

#[derive(Serialize)]
struct FlagsOut<'a> {
    raw: &'a str,
    set: Option<&'a str>,
    clear: Option<&'a str>,
    require_set: Option<&'a str>,
    require_unset: Option<&'a str>,
}

#[derive(Serialize)]
struct ConditionsOut {
    squad_count: i16,
    army_strength: i16,
    army_inactive: u8,
    stats_check: u8,
    level: i16,
    gold: i16,
    holiness_mana: i16,
    buildings_check: u8,
    buildings: [u8; 3],
    buildings_owner: [u8; 3],
    units_check: u8,
    units: [u8; 3],
    units_named: [u8; 3],
    units_owner: [u8; 3],
    artifacts_check: u8,
    artifacts: [u8; 3],
    artifacts_owner: [u8; 3],
    defeated_check: u8,
    defeated_armies: [u8; 2],
    happened_yes_check: u8,
    happened_yes: [u16; 2],
    not_happened_check: u8,
    not_happened: [u16; 2],
    beaten_check: u8,
    beaten_armies: [u8; 2],
    happened_no_check: u8,
    happened_no: [u16; 2],
    meet_army: u8,
    army_active: u8,
    confirm_question: u8,
    army_at_home: u8,
}

fn conditions_out(c: &EventConditions) -> ConditionsOut {
    ConditionsOut {
        squad_count: c.squad_count,
        army_strength: c.army_strength,
        army_inactive: c.army_inactive,
        stats_check: c.stats_check,
        level: c.level,
        gold: c.gold,
        holiness_mana: c.holiness_mana,
        buildings_check: c.buildings_check,
        buildings: c.buildings,
        buildings_owner: c.buildings_owner,
        units_check: c.units_check,
        units: c.units,
        units_named: c.units_named,
        units_owner: c.units_owner,
        artifacts_check: c.artifacts_check,
        artifacts: c.artifacts,
        artifacts_owner: c.artifacts_owner,
        defeated_check: c.defeated_check,
        defeated_armies: c.defeated_armies,
        happened_yes_check: c.happened_yes_check,
        happened_yes: c.happened_yes,
        not_happened_check: c.not_happened_check,
        not_happened: c.not_happened,
        beaten_check: c.beaten_check,
        beaten_armies: c.beaten_armies,
        happened_no_check: c.happened_no_check,
        happened_no: c.happened_no,
        meet_army: c.meet_army,
        army_active: c.army_active,
        confirm_question: c.confirm_question,
        army_at_home: c.army_at_home,
    }
}

#[derive(Serialize)]
struct ResultsOut {
    patrol_army: u8,
    patrol_delta: i8,
    relative_event: u16,
    relative_delay_hours: u16,
    cast_spell: u8,
    picture: u8,
    experience: i16,
    gold: i16,
    mana: i16,
    spells_learned: [u8; 4],
    units_add: [u8; 4],
    units_add_named: [u8; 4],
    units_remove: [u8; 4],
    units_remove_named: [u8; 4],
    artifacts_add: [u8; 4],
    artifacts_remove: [u8; 4],
    activate_armies: [u8; 2],
    deactivate_army: u8,
    completes_quest: u16,
    delay_hours: u16,
    light_lanterns: [u16; 4],
    removed_units_to_army: u8,
    new_hero_class: u8,
    chained_event: u16,
    units_from_army: u8,
    move_to_hero: u8,
    show_army: u8,
    hero_one_hp: u8,
    start_battle_with: u8,
    no_meeting: u8,
    repeat_after_yes: u8,
}

fn results_out(r: &EventResults) -> ResultsOut {
    ResultsOut {
        patrol_army: r.patrol_army,
        patrol_delta: r.patrol_delta,
        relative_event: r.relative_event,
        relative_delay_hours: r.relative_delay_hours,
        cast_spell: r.cast_spell,
        picture: r.picture,
        experience: r.experience,
        gold: r.gold,
        mana: r.mana,
        spells_learned: r.spells_learned,
        units_add: r.units_add,
        units_add_named: r.units_add_named,
        units_remove: r.units_remove,
        units_remove_named: r.units_remove_named,
        artifacts_add: r.artifacts_add,
        artifacts_remove: r.artifacts_remove,
        activate_armies: r.activate_armies,
        deactivate_army: r.deactivate_army,
        completes_quest: r.completes_quest,
        delay_hours: r.delay_hours,
        light_lanterns: r.light_lanterns,
        removed_units_to_army: r.removed_units_to_army,
        new_hero_class: r.new_hero_class,
        chained_event: r.chained_event,
        units_from_army: r.units_from_army,
        move_to_hero: r.move_to_hero,
        show_army: r.show_army,
        hero_one_hp: r.hero_one_hp,
        start_battle_with: r.start_battle_with,
        no_meeting: r.no_meeting,
        repeat_after_yes: r.repeat_after_yes,
    }
}

#[derive(Serialize)]
struct EventOut<'a> {
    /// 1-based event id: the id buildings, points and the victory and defeat fields use.
    id: usize,
    kind: u8,
    kind_name: &'static str,
    /// The title as the file has it, flag script included (`flags` holds the parsed script).
    title: &'a str,
    question: &'a str,
    message: &'a str,
    flags: Option<FlagsOut<'a>>,
    start_time: u32,
    repeat: u16,
    duration: u16,
    archetype: u8,
    once: u8,
    subordinate: u8,
    generate_battle_army: u8,
    has_custom_picture: bool,
    conditions: ConditionsOut,
    results: ResultsOut,
}

fn event_out(id: usize, e: &Event) -> EventOut<'_> {
    EventOut {
        id,
        kind: e.kind,
        kind_name: e.kind().map_or(UNKNOWN, event_kind_name),
        title: &e.title,
        question: &e.question,
        message: &e.message,
        flags: e.flags.as_ref().map(|f| FlagsOut {
            raw: &f.raw,
            set: f.set.as_deref(),
            clear: f.clear.as_deref(),
            require_set: f.require_set.as_deref(),
            require_unset: f.require_unset.as_deref(),
        }),
        start_time: e.start_time,
        repeat: e.repeat,
        duration: e.duration,
        archetype: e.archetype,
        once: e.once,
        subordinate: e.subordinate,
        generate_battle_army: e.generate_battle_army,
        has_custom_picture: e.custom_picture.is_some(),
        conditions: conditions_out(&e.conditions),
        results: results_out(&e.results),
    }
}

#[derive(Serialize)]
struct NamedOut<'a> {
    unit: u8,
    name: &'a str,
}

fn export<'a>(s: &'a Scenario, source_file: &'a str) -> Export<'a> {
    let h = &s.header;
    Export {
        format: FORMAT,
        version: VERSION,
        source_file,
        width: h.width,
        height: h.height,
        info: Info {
            title: &s.title,
            description: &s.description,
            campaign_name: &s.campaign_name,
            next_map: &s.next_map,
            scenario_kind: h.scenario_kind,
            start_time: h.start_time,
            victory_event: h.victory_event,
            defeat_event: h.defeat_event,
            relations: h.relations,
            generator_seed: h.generator_seed,
        },
        heroes: HERO_NAMES.iter().zip(h.heroes.iter()).map(|(name, hero)| hero_out(name, hero)).collect(),
        legend: Legend {
            surfaces: Surface::ALL
                .iter()
                .map(|&sf| CodeName { code: sf as u8, name: surface_name(sf) })
                .collect(),
            object_classes: ObjectClasses {
                one: object_class_name(1),
                five: object_class_name(5),
                eight: object_class_name(8),
                nine: object_class_name(9),
                ten: object_class_name(10),
                eleven: object_class_name(11),
            },
        },
        terrain: terrain_out(s),
        objects: s.objects.iter().map(object_out).collect(),
        buildings: s.buildings.iter().enumerate().map(|(i, b)| building_out(i + 1, b)).collect(),
        armies: s.armies.iter().map(army_out).collect(),
        points: s.points.iter().map(point_out).collect(),
        events: s.events.iter().enumerate().map(|(i, e)| event_out(i + 1, e)).collect(),
        named_characters: s.named_characters.iter().map(|c| NamedOut { unit: c.unit, name: &c.name }).collect(),
        has_scenario_picture: s.scenario_picture.is_some(),
    }
}

/// The export as a JSON value. Its object keys come out sorted (serde_json without
/// `preserve_order`); use [`scenario_to_string`] when the key order matters.
pub fn scenario_to_json(s: &Scenario, source_file: &str) -> Value {
    serde_json::to_value(export(s, source_file)).expect("the export holds only plain data")
}

/// The export as text, keys in the documented order. `source_file` is the map's file name
/// only, never a path.
pub fn scenario_to_string(s: &Scenario, source_file: &str, pretty: bool) -> serde_json::Result<String> {
    let e = export(s, source_file);
    if pretty {
        serde_json::to_string_pretty(&e)
    } else {
        serde_json::to_string(&e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{FlagScript, Header, HeroPreset, NamedCharacter};
    use crate::dt::install;
    use std::path::Path;

    /// A 3x2 map with one object, building, army, point, event, hero troop and named character.
    fn sample() -> Scenario {
        let mut hero = HeroPreset { gold: 500, artifacts: [7, 0, 0], ..HeroPreset::default() };
        hero.troops = std::array::from_fn(|i| if i == 0 { Troop { unit: 1, level: 2, count: 3 } } else { Troop::default() });
        let mut s = Scenario {
            header: Header { width: 3, height: 2, ..Header::default() },
            terrain: vec![6, 4, 4, 0, 1, 1],
            objects: vec![MapObject { x: 2, y: 1, sprite: 42, class: 9 }],
            title: "Test map".into(),
            named_characters: vec![NamedCharacter { unit: 3, name: "Ann".into() }],
            ..Scenario::default()
        };
        s.header.heroes[0] = hero;
        let mut castle = Building {
            x: 3,
            y: 1,
            size_x: 1,
            size_y: 1,
            kind: 3,
            name: "Castle".into(),
            event_count: 1,
            event_slots: std::array::from_fn(|i| u16::from(i == 0)),
            ..Building::default()
        };
        castle.garrison[0] = Troop { unit: 9, level: 1, count: 2 };
        castle.barracks[0] = RecruitSlot { unit: 4, start_count: 3, max_count: 9 };
        s.buildings.push(castle);
        let mut army = Army { x: 1, y: 1, id: 1, model: 5, ..Army::default() };
        army.troops[0] = Troop { unit: 42, level: 0, count: 2 };
        s.armies.push(army);
        s.points.push(Point {
            id: 1,
            model: 8,
            radius: 24,
            active: 1,
            event_slots: std::array::from_fn(|i| u16::from(i == 0)),
            ..Point::default()
        });
        let mut quest = Event { kind: 3, title: "Quest%+A=/B".into(), ..Event::default() };
        quest.flags = FlagScript::from_title(&quest.title);
        quest.conditions.gold = 5;
        quest.results.units_add = [1, 0, 0, 0];
        s.events.push(quest);
        s
    }

    #[test]
    fn export_has_the_v1_header_and_names() {
        let v = scenario_to_json(&sample(), "test.DTm");
        assert_eq!(v["format"], "razdor-dtm-json");
        assert_eq!(v["version"], 1);
        assert_eq!(v["source_file"], "test.DTm");
        assert_eq!(v["width"], 3);
        assert_eq!(v["height"], 2);
        assert_eq!(v["info"]["title"], "Test map");
        assert_eq!(v["heroes"][0]["archetype"], "knight");
        assert_eq!(v["heroes"][2]["archetype"], "ranger");
        assert_eq!(v["heroes"][0]["gold"], 500);
        assert_eq!(v["heroes"][0]["troops"].as_array().unwrap().len(), 1, "empty troop slots are dropped");
        assert_eq!(v["heroes"][0]["artifacts"], serde_json::json!([7]));
        assert_eq!(v["legend"]["surfaces"].as_array().unwrap().len(), 16);
        assert_eq!(v["legend"]["surfaces"][2], serde_json::json!({"code": 2, "name": "deep_sea"}));
        assert_eq!(v["legend"]["object_classes"]["9"], "trees");
        assert_eq!(v["named_characters"][0], serde_json::json!({"unit": 3, "name": "Ann"}));
        assert_eq!(v["has_scenario_picture"], false);
    }

    #[test]
    fn terrain_is_hex_rows() {
        let v = scenario_to_json(&sample(), "test.DTm");
        assert_eq!(v["terrain"]["encoding"], "hex-rows");
        assert_eq!(v["terrain"]["rows"], serde_json::json!(["644", "011"]));
    }

    #[test]
    fn a_code_above_15_switches_to_numbers() {
        let mut s = sample();
        s.terrain[0] = 16;
        let v = scenario_to_json(&s, "test.DTm");
        assert_eq!(v["terrain"]["encoding"], "numbers");
        assert_eq!(v["terrain"]["rows"], serde_json::json!([[16, 4, 4], [0, 1, 1]]));
    }

    #[test]
    fn objects_buildings_armies_points_and_events_are_named() {
        let v = scenario_to_json(&sample(), "test.DTm");
        assert_eq!(v["objects"][0]["class_name"], "trees");
        let b = &v["buildings"][0];
        assert_eq!(b["index"], 1);
        assert_eq!(b["kind_name"], "castle");
        assert_eq!(b["owner_army"], 255, "the neutral owner is the raw 255");
        assert_eq!(b["events"], serde_json::json!([1]));
        assert_eq!(b["garrison"], serde_json::json!([{"unit": 9, "level": 1, "count": 2}]));
        assert_eq!(b["barracks"], serde_json::json!([{"unit": 4, "start_count": 3, "max_count": 9}]));
        let a = &v["armies"][0];
        assert_eq!(a["model_name"], "bandits");
        assert_eq!(a["troops"].as_array().unwrap().len(), 1);
        assert_eq!(v["points"][0]["events"], serde_json::json!([1]));
        let e = &v["events"][0];
        assert_eq!(e["id"], 1);
        assert_eq!(e["kind_name"], "quest");
        assert_eq!(e["flags"]["set"], "A");
        assert_eq!(e["flags"]["require_unset"], "B");
        assert!(e["flags"]["clear"].is_null());
        assert_eq!(e["conditions"]["gold"], 5);
        assert_eq!(e["results"]["units_add"], serde_json::json!([1, 0, 0, 0]));
        assert_eq!(e["has_custom_picture"], false);
    }

    #[test]
    fn unknown_codes_and_empty_slots_get_fallback_names() {
        let mut s = sample();
        s.armies[0].model = 0;
        s.buildings[0].kind = 99;
        s.objects[0].class = 77;
        s.events[0].kind = 0;
        let v = scenario_to_json(&s, "test.DTm");
        assert_eq!(v["armies"][0]["model_name"], "unknown");
        assert_eq!(v["buildings"][0]["kind_name"], "unknown");
        assert_eq!(v["objects"][0]["class_name"], "other");
        assert_eq!(v["events"][0]["kind_name"], "unknown");
        assert_eq!(v["buildings"][0]["barracks"].as_array().unwrap().len(), 1);
        assert!(v["armies"][0]["artifacts"].as_array().unwrap().is_empty());
    }

    #[test]
    fn no_unknown_fields_and_no_picture_bytes_in_the_text() {
        let mut s = sample();
        s.scenario_picture = Some(vec![0xAB; 64]);
        s.events[0].custom_picture = Some(vec![0xCD; 64]);
        let text = scenario_to_string(&s, "test.DTm", true).unwrap();
        assert!(!text.contains("unknown_"), "no unknown_* field may be written");
        assert!(!text.contains("scenario_picture\":["), "the scenario picture is a flag, not bytes");
        assert!(!text.contains("\"custom_picture\""), "the event picture is a flag, not bytes");
        assert!(text.contains("\"has_scenario_picture\": true"));
        assert!(text.contains("\"has_custom_picture\": true"));
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["has_scenario_picture"], true);
    }

    #[test]
    fn text_keeps_the_documented_key_order() {
        // Pretty output: the top-level keys are the ones indented by two spaces.
        let text = scenario_to_string(&sample(), "test.DTm", true).unwrap();
        let keys = ["format", "version", "source_file", "width", "info", "heroes", "legend", "terrain", "objects", "buildings", "armies", "points", "events", "named_characters", "has_scenario_picture"];
        let at: Vec<usize> = keys.iter().map(|k| text.find(&format!("\n  \"{k}\": ")).unwrap()).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "keys out of order");
    }

    #[test]
    fn real_maps_export_as_valid_json() {
        let Some(dir) = std::env::var_os(install::ENV_VAR) else { return };
        let maps = install::list_maps(Path::new(&dir)).expect("the install's maps folder");
        assert!(!maps.is_empty(), "the maps folder has no maps");
        for m in maps {
            let s = Scenario::load(&m.path).unwrap_or_else(|e| panic!("{}: {e}", m.name));
            let file = format!("{}.DTm", m.name);
            let text = scenario_to_string(&s, &file, false).unwrap();
            let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", m.name));
            let (w, h) = (v["width"].as_u64().unwrap() as usize, v["height"].as_u64().unwrap() as usize);
            let rows = v["terrain"]["rows"].as_array().unwrap();
            assert_eq!(rows.len(), h, "{}: row count", m.name);
            for row in rows {
                let len = match row {
                    Value::String(t) => t.chars().count(),
                    Value::Array(cells) => cells.len(),
                    _ => panic!("{}: a row is neither text nor an array", m.name),
                };
                assert_eq!(len, w, "{}: cells in a row", m.name);
            }
        }
    }
}
