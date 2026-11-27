//! The units JSON of `razdor-units-json` (format `razdor-units-json`, version 1 or 2; written by
//! `razdor::dt::units_json`): the unit cards of the install, read for the real army stats (spec §3.8,
//! Q3). The file is game data and is never committed; tests here use synthetic files only.
//!
//! Levels follow the game: a map's troop level is 0-based (the game reads `level + 1`, as hired =
//! 1), and the gain per level above the first applies `level` times. A percent stat (the
//! protections) grows by the game's rule, [`percent_at`].

use serde::Deserialize;
use std::collections::BTreeMap;

/// The `format` the converter accepts.
pub const FORMAT: &str = "razdor-units-json";
/// The `version` the converter writes in its own tests. The reader takes [`VERSIONS`].
pub const VERSION: u32 = 2;
/// The versions the reader accepts: 1 (the units) and 2 (the units and the world spells). A
/// version 1 file loads with no spells.
pub const VERSIONS: [u32; 2] = [1, 2];

#[derive(Debug, Deserialize)]
struct File {
    format: String,
    version: u32,
    #[serde(default)]
    units: Vec<Unit>,
    #[serde(default)]
    spells: Vec<WorldSpell>,
}

/// One world spell of the install (the hero's), as the version-2 units JSON writes it: the raw
/// fields and the summary `kind` (`damage`, `heal`, `modifier` or `none`). `spells.rs` turns it into
/// a SPEL record.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorldSpell {
    /// 1-based position in the install's spell list; the events refer to spells by it.
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub cost_gold: i64,
    #[serde(default)]
    pub cost_mana: i64,
    /// `life`, `elemental`, `death` or `None`.
    pub school: Option<String>,
    /// `hero`, `enemy`, `one_enemy` or `None`.
    pub target: Option<String>,
    /// Duration in game hours at caster level 0; `None` = instant.
    pub time_work_hours: Option<i64>,
    pub kind: String,
    pub delta_fixed_hits: Option<i64>,
    /// Flat change of each stat while it lasts, keyed by the unit stat's name (`attack_blow`, ...).
    #[serde(default)]
    pub add: BTreeMap<String, i64>,
    /// Percent change of each stat while it lasts, keyed like `add`.
    #[serde(default)]
    pub percent: BTreeMap<String, i64>,
}

/// One unit card (the fields the converter uses; the rest of the file is ignored).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Unit {
    pub id: i64,
    pub name: String,
    /// The install's price (`cost`). The army-strength test (events.lua) takes it as the unit's cost; 0 when absent.
    #[serde(default)]
    pub cost: i64,
    pub hit_points: i64,
    pub attack: Pair,
    pub defence: Pair,
    pub magic_power: i64,
    /// `life`, `elemental`, `death`, or `None` for no school.
    pub magic: Option<String>,
    /// `to_enemy`, `to_ally`, `to_all`, or `None`.
    #[serde(default)]
    pub magic_direction: Option<String>,
    pub protection: Protection,
    pub initiative: i64,
    #[serde(default)]
    pub level_up: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Pair {
    /// Melee (`blow`) or ranged (`shot`) value.
    pub blow: i64,
    pub shot: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Protection {
    pub life: i64,
    pub death: i64,
    pub elemental: i64,
}

/// The units of an install, by id.
#[derive(Debug, Clone, Default)]
pub struct Units {
    by_id: BTreeMap<i64, Unit>,
    spells: Vec<WorldSpell>,
}

impl Units {
    /// Reads the JSON text. The format and version must match; a duplicate id is an error.
    pub fn from_json(text: &str) -> Result<Units, String> {
        let file: File = serde_json::from_str(text).map_err(|e| format!("not a units file: {e}"))?;
        if file.format != FORMAT {
            return Err(format!("the format is {}, not {FORMAT}", file.format));
        }
        if !VERSIONS.contains(&file.version) {
            return Err(format!("the units version is {}, the converter reads {VERSIONS:?}", file.version));
        }
        let mut by_id = BTreeMap::new();
        for u in file.units {
            let id = u.id;
            if by_id.insert(id, u).is_some() {
                return Err(format!("unit id {id} is in the file twice"));
            }
        }
        // Version 1 has no spells; a version 2 file without the array has none either.
        let spells = if file.version >= 2 { file.spells } else { Vec::new() };
        Ok(Units { by_id, spells })
    }

    pub fn get(&self, id: i64) -> Option<&Unit> {
        self.by_id.get(&id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The world spells in file order (empty for a version 1 file).
    pub fn spells(&self) -> &[WorldSpell] {
        &self.spells
    }
}

impl Unit {
    /// A flat stat at a 0-based level: `base + gain · level` (the game's `d-*` rule). A stat the
    /// file has no gain for does not grow.
    pub fn flat_at(&self, base: i64, key: &str, level: i64) -> i64 {
        base + self.level_up.get(key).copied().unwrap_or(0) * level.max(0)
    }

    /// Hit points at a 0-based level.
    pub fn hit_points_at(&self, level: i64) -> i64 {
        self.flat_at(self.hit_points, "hit_points", level)
    }

    /// Melee attack at a 0-based level.
    pub fn attack_blow_at(&self, level: i64) -> i64 {
        self.flat_at(self.attack.blow, "attack_blow", level)
    }

    /// Ranged attack at a 0-based level.
    pub fn attack_shot_at(&self, level: i64) -> i64 {
        self.flat_at(self.attack.shot, "attack_shot", level)
    }

    /// Melee defence at a 0-based level.
    pub fn defence_blow_at(&self, level: i64) -> i64 {
        self.flat_at(self.defence.blow, "defence_blow", level)
    }

    /// Ranged defence at a 0-based level.
    pub fn defence_shot_at(&self, level: i64) -> i64 {
        self.flat_at(self.defence.shot, "defence_shot", level)
    }

    /// Magic power at a 0-based level (0 without a school, as the game has it).
    pub fn magic_power_at(&self, level: i64) -> i64 {
        if self.magic.is_none() {
            return 0;
        }
        self.flat_at(self.magic_power, "magic_power", level)
    }

    /// Initiative at a 0-based level.
    pub fn initiative_at(&self, level: i64) -> i64 {
        self.flat_at(self.initiative, "initiative", level)
    }

    /// The protection of school `key` (`life`, `death` or `elemental`) at a 0-based level.
    pub fn protection_at(&self, key: &str, level: i64) -> i64 {
        let base = match key {
            "life" => self.protection.life,
            "death" => self.protection.death,
            _ => self.protection.elemental,
        };
        percent_at(base, self.level_up.get(&format!("protect_{key}")).copied().unwrap_or(0), level)
    }
}

/// A percent stat after `level` level-ups, as the game computes it (`rules::experience::percent_stat`):
/// each level cuts what is left to 100 by `d` percent, rounded half to even, at most 99.
pub fn percent_at(base: i64, d: i64, level: i64) -> i64 {
    let mut rest = (100 - base) as f32;
    for _ in 0..level.max(0) {
        rest -= d as f32 * rest / 100.0;
    }
    let rounded = (rest as f64).round_ties_even() as i64;
    (100 - rounded).min(99)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic file with one knight-like unit and one mage (no real game data).
    fn sample() -> String {
        serde_json::json!({
            "format": FORMAT, "version": VERSION, "language": "ru", "count": 2,
            "units": [
                {
                    "id": 1, "name": "Тест", "cost": 10, "hit_points": 80,
                    "attack": {"blow": 40, "shot": 0}, "defence": {"blow": 10, "shot": 6},
                    "magic_power": 0, "magic": null, "magic_direction": null,
                    "protection": {"life": 20, "death": 0, "elemental": 50},
                    "initiative": 9, "actions": 1, "regeneration": 0, "vampirism": 0,
                    "ranged": false, "flying": false, "nature": "normal", "bonus": null,
                    "evasion": null, "min_magic_power": null, "mana_drain": null,
                    "experience": {"start": 100, "growth_percent": 150},
                    "level_up": {"hit_points": 8, "attack_blow": 5, "protect_life": 10}
                },
                {
                    "id": 2, "name": "Маг", "cost": 20, "hit_points": 50,
                    "attack": {"blow": 0, "shot": 0}, "defence": {"blow": 0, "shot": 0},
                    "magic_power": 25, "magic": "elemental", "magic_direction": "to_enemy",
                    "protection": {"life": 35, "death": 35, "elemental": 35},
                    "initiative": 26, "actions": 2, "regeneration": 0, "vampirism": 0,
                    "ranged": false, "flying": true, "nature": "normal", "bonus": "Flying",
                    "evasion": null, "min_magic_power": null, "mana_drain": null,
                    "experience": {"start": 90, "growth_percent": 160},
                    "level_up": {"magic_power": 5}
                }
            ]
        })
        .to_string()
    }

    #[test]
    fn a_units_file_reads_by_id() {
        let units = Units::from_json(&sample()).unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units.get(1).unwrap().name, "Тест");
        assert_eq!(units.get(2).unwrap().magic.as_deref(), Some("elemental"));
        assert!(units.get(3).is_none());
    }

    #[test]
    fn a_wrong_format_or_version_is_refused() {
        let text = sample().replace(FORMAT, "other-format");
        assert!(Units::from_json(&text).unwrap_err().contains("format"));
        let text = sample().replace("\"version\":2", "\"version\":3");
        assert!(Units::from_json(&text).unwrap_err().contains("version"));
        let text = sample().replace("\"version\":2", "\"version\":1");
        assert!(Units::from_json(&text).is_ok(), "version 1 is still read");
        assert!(Units::from_json("{").is_err());
    }

    #[test]
    fn a_duplicate_id_is_refused() {
        let text = sample().replace("\"id\":2", "\"id\":1");
        assert!(Units::from_json(&text).unwrap_err().contains("twice"));
    }

    #[test]
    fn flat_stats_grow_by_the_gain_for_each_level_above_the_first() {
        let units = Units::from_json(&sample()).unwrap();
        let u = units.get(1).unwrap();
        // Level 1 (0 gains): the base values; level 3 (2 gains).
        assert_eq!((u.hit_points_at(0), u.hit_points_at(2)), (80, 96));
        assert_eq!(u.attack_blow_at(3), 55);
        assert_eq!(u.defence_blow_at(2), 10, "no gain in the file: no growth");
        assert_eq!(u.magic_power_at(2), 0, "no school, no magic power");
        assert_eq!(units.get(2).unwrap().magic_power_at(2), 35);
    }

    #[test]
    fn a_percent_protection_grows_as_the_game_grows_it() {
        // 20 percent with 10 percent growth: 80 * 0.9 = 72 left after one level, so 28 at level 2.
        assert_eq!(percent_at(20, 10, 0), 20);
        assert_eq!(percent_at(20, 10, 1), 28);
        assert_eq!(percent_at(0, 0, 5), 0);
        assert_eq!(percent_at(0, 100, 1), 99, "at most 99");
        let units = Units::from_json(&sample()).unwrap();
        assert_eq!(units.get(1).unwrap().protection_at("life", 1), 28);
        assert_eq!(units.get(1).unwrap().protection_at("elemental", 4), 50, "no gain in the file");
    }
}
