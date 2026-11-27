//! JSON export of the unit cards: the `razdor-units-json` format, version 1 (`docs/flows/testing-and-tools.md`).
//!
//! Meant for converters to other games (the OpenMW world reads it for real army stats), so the
//! layout is stable and versioned. Numbers are the install's raw values, after the install's own
//! build corrections (`data::apply_build`), with no rule applied: the field comments say what each
//! number means. Names are snake_case; `id` is the unit's `GlobalIndex`, as the map files use it.
//! The names are the install's Russian unless the export was made in another data language.
//!
//! Stats at level `L` (1 = as hired, the file's `level` plus one) follow the rules of
//! `rules::units::Stats::of_level`: a flat stat is `value + gain * (L - 1)`; a percent stat (the
//! protections, regeneration and vampirism) grows by `rules::experience::percent_stat`.
//!
//! Pure data: no macroquad and no install. The DTO structs here are private so that the unit types
//! in [`super::data`] stay free of serde.

use serde::Serialize;
use std::collections::BTreeMap;

use super::data::{Bonus, MagicDirection, MagicSchool, Nature, Stat, UnitDef};

/// The `format` value of every export.
pub const FORMAT: &str = "razdor-units-json";
/// The `version` value of every export. Bumped on any change of the layout.
pub const VERSION: u32 = 1;

#[derive(Serialize)]
struct Export<'a> {
    format: &'static str,
    version: u32,
    /// The data language of the names: `ru` (the install's own) or the prefix of the other language.
    language: &'a str,
    /// How many units follow.
    count: usize,
    units: Vec<UnitOut<'a>>,
}

#[derive(Serialize)]
struct UnitOut<'a> {
    /// The unit's `GlobalIndex` (1..=102 in vanilla); unique in the file.
    id: u32,
    /// The unit's name in the data language.
    name: &'a str,
    /// Hire price in gold; the base of wages, healing and the XP value.
    cost: i32,
    /// Hit points at level 1.
    hit_points: i32,
    attack: Attack,
    defence: Defence,
    /// Magic power at level 1; 0 for a unit without a magic school.
    magic_power: i32,
    /// Magic school: `life`, `elemental` or `death`; `null` for none.
    magic: Option<&'static str>,
    /// Who the magic hits: `to_all`, `to_enemy` or `to_ally`; `null` for none.
    magic_direction: Option<&'static str>,
    protection: Protection,
    /// Initiative at level 1: the order of the unit's actions in a battle turn.
    initiative: i32,
    /// Actions per battle turn.
    actions: i32,
    /// Percent regeneration per turn at level 1.
    regeneration: i32,
    /// Percent vampirism at level 1.
    vampirism: i32,
    /// True when the unit has a ranged attack (its `attack.shot` is above 0).
    ranged: bool,
    /// True when the unit has the `Flying` bonus.
    flying: bool,
    /// Nature: `normal`, `undead`, `elemental`, `rogue`, `animal` or `hero` (`people` is read as normal by the game).
    nature: &'static str,
    /// The unit's special ability as the file names it (a bonus token); `null` for none.
    bonus: Option<&'a str>,
    /// Community: percent of physical damage ignored; `null` when the file has none.
    evasion: Option<i32>,
    /// Community: floor of magic power in battle; `null` when the file has none.
    min_magic_power: Option<i32>,
    /// Community: magic power lost per battle turn; `null` when the file has none.
    mana_drain: Option<i32>,
    experience: Experience,
    /// Gain per level above the first, keyed by stat (see the module comment for the formula).
    /// Only the stats the file gives a value; a missing key is 0.
    level_up: BTreeMap<&'static str, i32>,
}

#[derive(Serialize)]
struct Attack {
    /// Melee attack value (`AttackBlow`); the damage of a blow is the attack minus the defence, at least 1.
    blow: i32,
    /// Ranged attack value (`AttackShot`); 0 for a unit without a ranged attack.
    shot: i32,
}

#[derive(Serialize)]
struct Defence {
    /// Melee defence value (`DefenceBlow`).
    blow: i32,
    /// Ranged defence value (`DefenceShot`).
    shot: i32,
}

#[derive(Serialize)]
struct Protection {
    /// Percent resistance to life magic.
    life: i32,
    /// Percent resistance to death magic.
    death: i32,
    /// Percent resistance to elemental magic.
    elemental: i32,
}

#[derive(Serialize)]
struct Experience {
    /// XP needed for the first level-up.
    start: i32,
    /// Percent growth of the XP needed per level.
    growth_percent: i32,
}

/// The level-up key of a stat: the snake_case name the export uses.
fn stat_key(s: Stat) -> &'static str {
    match s {
        Stat::Hits => "hit_points",
        Stat::AttackBlow => "attack_blow",
        Stat::DefenceBlow => "defence_blow",
        Stat::AttackShot => "attack_shot",
        Stat::DefenceShot => "defence_shot",
        Stat::MagicPower => "magic_power",
        Stat::Initiative => "initiative",
        Stat::Manevres => "actions",
        Stat::ProtectLife => "protect_life",
        Stat::ProtectDeath => "protect_death",
        Stat::ProtectElemental => "protect_elemental",
        Stat::Regen => "regeneration",
        Stat::Vampirizm => "vampirism",
    }
}

fn magic_name(m: MagicSchool) -> &'static str {
    match m {
        MagicSchool::Life => "life",
        MagicSchool::Elemental => "elemental",
        MagicSchool::Death => "death",
    }
}

fn direction_name(d: MagicDirection) -> &'static str {
    match d {
        MagicDirection::ToAll => "to_all",
        MagicDirection::ToEnemy => "to_enemy",
        MagicDirection::ToAlly => "to_ally",
    }
}

fn nature_name(n: Nature) -> &'static str {
    match n {
        Nature::Normal | Nature::People => "normal",
        Nature::Undead => "undead",
        Nature::Elemental => "elemental",
        Nature::Rogue => "rogue",
        Nature::Animal => "animal",
        Nature::Hero => "hero",
    }
}

fn unit_out<'a>(u: &'a UnitDef) -> UnitOut<'a> {
    let level_up = Stat::ALL.into_iter().filter_map(|s| u.level_up.get(&s).map(|&d| (stat_key(s), d))).collect();
    UnitOut {
        id: u.id,
        name: &u.name,
        cost: u.cost,
        hit_points: u.hits,
        attack: Attack { blow: u.attack_blow, shot: u.attack_shot },
        defence: Defence { blow: u.defence_blow, shot: u.defence_shot },
        magic_power: u.magic_power,
        magic: u.magic.map(magic_name),
        magic_direction: u.magic_direction.map(direction_name),
        protection: Protection { life: u.protect_life, death: u.protect_death, elemental: u.protect_elemental },
        initiative: u.initiative,
        actions: u.manevres,
        regeneration: u.regen,
        vampirism: u.vampirism,
        ranged: u.attack_shot > 0,
        flying: u.bonus == Some(Bonus::Flying),
        nature: nature_name(u.nature),
        bonus: u.bonus.as_ref().map(|b| b.token()),
        evasion: u.evasion,
        min_magic_power: u.min_magic_power,
        mana_drain: u.mana_drain,
        experience: Experience { start: u.start_experience, growth_percent: u.level_multiplier },
        level_up,
    }
}

/// The export of `units` as JSON text: pretty-printed when `pretty`, one line otherwise. `language`
/// is the data language of the names (`ru` for the install's own). The units are written in the
/// order given; the caller passes them in file order, so the output is the same on every run.
pub fn units_to_string(units: &[UnitDef], language: &str, pretty: bool) -> serde_json::Result<String> {
    let e = Export {
        format: FORMAT,
        version: VERSION,
        language,
        count: units.len(),
        units: units.iter().map(unit_out).collect(),
    };
    if pretty {
        serde_json::to_string_pretty(&e)
    } else {
        serde_json::to_string(&e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::Content;
    use serde_json::Value;

    /// A unit with the given id and name; every other field is the testkit's default.
    fn unit(id: u32, name: &str) -> UnitDef {
        crate::rules::content::testkit::unit(id, name)
    }

    fn parse(units: &[UnitDef]) -> Value {
        serde_json::from_str(&units_to_string(units, "ru", false).unwrap()).unwrap()
    }

    #[test]
    fn a_synthetic_unit_exports_its_raw_fields_under_their_names() {
        let mut u = unit(7, "Тестовый");
        u.cost = 120;
        u.hits = 40;
        u.attack_blow = 12;
        u.attack_shot = 9;
        u.defence_blow = 4;
        u.defence_shot = 3;
        u.magic_power = 6;
        u.magic = Some(MagicSchool::Death);
        u.magic_direction = Some(MagicDirection::ToEnemy);
        u.protect_life = 10;
        u.protect_death = 20;
        u.protect_elemental = 30;
        u.initiative = 5;
        u.manevres = 2;
        u.regen = 3;
        u.vampirism = 4;
        u.nature = Nature::Undead;
        u.bonus = Some(Bonus::Flying);
        u.start_experience = 60;
        u.level_multiplier = 150;
        u.level_up = [(Stat::Hits, 5), (Stat::ProtectLife, 2)].into_iter().collect();
        let v = parse(&[u]);
        assert_eq!((v["format"].as_str(), v["version"].as_u64(), v["count"].as_u64()), (Some(FORMAT), Some(1), Some(1)));
        assert_eq!(v["language"], "ru");
        let x = &v["units"][0];
        assert_eq!((x["id"].as_u64(), x["name"].as_str(), x["cost"].as_i64()), (Some(7), Some("Тестовый"), Some(120)));
        assert_eq!((x["hit_points"].as_i64(), x["initiative"].as_i64(), x["actions"].as_i64()), (Some(40), Some(5), Some(2)));
        assert_eq!((x["attack"]["blow"].as_i64(), x["attack"]["shot"].as_i64()), (Some(12), Some(9)));
        assert_eq!((x["defence"]["blow"].as_i64(), x["defence"]["shot"].as_i64()), (Some(4), Some(3)));
        assert_eq!((x["magic"].as_str(), x["magic_direction"].as_str()), (Some("death"), Some("to_enemy")));
        assert_eq!(x["protection"]["death"].as_i64(), Some(20));
        assert_eq!((x["nature"].as_str(), x["flying"].as_bool(), x["ranged"].as_bool()), (Some("undead"), Some(true), Some(true)));
        assert_eq!(x["bonus"].as_str(), Some("Flying"));
        assert_eq!((x["experience"]["start"].as_i64(), x["experience"]["growth_percent"].as_i64()), (Some(60), Some(150)));
        assert_eq!(x["level_up"]["hit_points"].as_i64(), Some(5));
        assert_eq!(x["level_up"]["protect_life"].as_i64(), Some(2));
        assert!(x["level_up"].get("initiative").is_none(), "a stat without a gain is left out");
    }

    #[test]
    fn empty_optional_fields_are_null_and_flags_are_false() {
        let v = parse(&[unit(1, "Пусто")]);
        let x = &v["units"][0];
        assert!(x["magic"].is_null() && x["magic_direction"].is_null() && x["bonus"].is_null());
        assert!(x["evasion"].is_null() && x["min_magic_power"].is_null() && x["mana_drain"].is_null());
        assert_eq!((x["ranged"].as_bool(), x["flying"].as_bool(), x["nature"].as_str()), (Some(false), Some(false), Some("normal")));
        assert_eq!(x["level_up"].as_object().map(|o| o.len()), Some(0));
    }

    #[test]
    fn people_is_exported_as_normal_nature() {
        let mut u = unit(2, "Люди");
        u.nature = Nature::People;
        assert_eq!(parse(&[u])["units"][0]["nature"], "normal");
    }

    #[test]
    fn units_keep_their_order_and_pretty_and_compact_hold_the_same_data() {
        let units = [unit(3, "а"), unit(1, "б")];
        let compact: Value = serde_json::from_str(&units_to_string(&units, "Eng", false).unwrap()).unwrap();
        let pretty: Value = serde_json::from_str(&units_to_string(&units, "Eng", true).unwrap()).unwrap();
        assert_eq!(compact, pretty);
        assert_eq!(compact["language"], "Eng");
        assert_eq!(compact["units"][0]["id"], 3);
        assert!(units_to_string(&units, "ru", true).unwrap().contains("\n  \"units\""));
    }

    #[test]
    fn real_units_export_as_valid_json_with_unique_ids() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("the install loads");
        let content = Content::from_dt_in(&dt, None);
        assert!(!content.units.is_empty(), "the install has units");
        let text = units_to_string(&content.units, "ru", false).unwrap();
        let v: Value = serde_json::from_str(&text).expect("valid JSON");
        let units = v["units"].as_array().unwrap();
        assert_eq!(v["count"].as_u64(), Some(units.len() as u64));
        assert!(!units.is_empty());
        let mut ids: Vec<u64> = units.iter().map(|u| u["id"].as_u64().unwrap()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), units.len(), "unit ids are unique");
    }
}
