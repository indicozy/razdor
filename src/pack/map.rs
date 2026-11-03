//! `maps/<id>/map.toml`: one playable map of a package. The terrain is `maps/<id>/terrain.png`
//! (see [`super::terrain`]). Text keys name the map's and its buildings' and armies' texts; unit
//! references use the package's reference grammar (`dt:` aliases, the package's own unit ids).
//! The rules a map must keep are in [`super::map_check`]; [`super::compile`] turns the map into a
//! `.DTm` scenario.

use std::collections::BTreeMap;

use serde::Deserialize;
use toml::Spanned;

/// A map as read. Unknown fields are errors.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapFile {
    /// A text key: the map's name in the new-game list and the hero window.
    pub title: String,
    /// A text key.
    #[serde(default)]
    pub description: Option<String>,
    /// `YYYY-MM-DD` on the game's clock: 30-day months, 12-month years, the day the map starts.
    pub start_date: String,
    /// Faction relations, rows and columns in the order player, ally, neighbour, enemy; −3..3.
    pub relations: [[i8; 4]; 4],
    /// Terrain colours that differ from the default palette (`docs/reference/rzpack-format.md`).
    #[serde(default)]
    pub terrain_palette: BTreeMap<String, String>,
    pub hero: HeroSpec,
    #[serde(default)]
    pub object: Vec<Spanned<ObjectSpec>>,
    #[serde(default)]
    pub building: Vec<Spanned<BuildingSpec>>,
    #[serde(default)]
    pub army: Vec<Spanned<ArmySpec>>,
    #[serde(default)]
    pub point: Vec<Spanned<PointSpec>>,
    pub victory: Spanned<Victory>,
}

/// The hero's start: a cell, the classes the hero window offers, gold, mana and troops.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeroSpec {
    /// `[x, y]`, the start cell (the same for every offered class).
    pub cell: [u32; 2],
    /// Offered classes: `knight`, `archmage`, `ranger`.
    pub classes: Vec<String>,
    pub gold: u32,
    #[serde(default)]
    pub mana: u32,
    #[serde(default)]
    pub troops: Vec<TroopSpec>,
}

/// A troop slot: a unit reference, its level as the game shows it (1 = first level) and count.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TroopSpec {
    pub unit: String,
    #[serde(default = "first_level")]
    pub level: u8,
    pub count: u8,
}

fn first_level() -> u8 {
    1
}

/// A terrain object: a hill, a mountain, trees and so on.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectSpec {
    /// `hills`, `mountains`, `stones`, `trees`, `dead_trees` or `thicket`.
    pub kind: String,
    pub cell: [u32; 2],
    /// The editor's sprite id; the default for the kind when left out.
    #[serde(default)]
    pub sprite: Option<u8>,
}

/// A building: a town, a village, a castle, a bridge ….
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildingSpec {
    /// Unique among the map's buildings; victory conditions name it.
    pub id: String,
    /// The building type: `palace`, `town`, `village`, `castle`, `fort`, `tavern`, `market`,
    /// `church`, `smithy`, `shipyard`, `altar`, `dungeon_entrance`, `ruins`, `stone_bridge`,
    /// `wooden_bridge`, `obelisk`.
    #[serde(rename = "type")]
    pub kind: String,
    /// A text key.
    pub name: String,
    /// A text key.
    #[serde(default)]
    pub description: Option<String>,
    /// The bottom-right cell of the footprint.
    pub cell: [u32; 2],
    /// The footprint in cells, `[width, height]`, 1 to 8 each: the cells up and left of `cell`
    /// (one more row above when wider than tall, as the original's). Optional: the most common
    /// size of the type in the original maps (`compile::default_footprint`).
    #[serde(default)]
    pub size: Option<[u32; 2]>,
    /// `player`, `ally`, `neighbour` or `enemy`.
    pub faction: String,
    /// Gold a day (the building's income).
    #[serde(default)]
    pub income: u16,
    #[serde(default)]
    pub garrison: Vec<TroopSpec>,
    /// Units the building can recruit: the stock at the start and the most it holds.
    #[serde(default)]
    pub recruit: Vec<RecruitSpec>,
}

/// A recruit slot of a building.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecruitSpec {
    pub unit: String,
    pub start: u8,
    pub max: u8,
}

/// An AI army. Its id is unique among the map's armies; victory conditions name it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmySpec {
    pub id: String,
    /// A text key: the army's name.
    pub name: String,
    pub cell: [u32; 2],
    /// `player`, `ally`, `neighbour` or `enemy`.
    pub faction: String,
    /// `feudal`, `bandits`, `peasants`, `inactive`, `necromancer`, `ghosts` or `zombies`.
    pub model: String,
    /// `none` (the default), `hero`, `pirate` or `merchant`: an army on water needs a ship.
    #[serde(default)]
    pub ship: Option<String>,
    /// Gold the army starts with.
    #[serde(default)]
    pub gold: u32,
    #[serde(default)]
    pub troops: Vec<TroopSpec>,
    /// A named leader: a character with a name of his own.
    #[serde(default)]
    pub leader: Option<LeaderSpec>,
    /// How the battle with the hero is fought: `dt` (the default) or `lua:<script>`, a field
    /// battle run by `scripts/<script>.lua` (`docs/reference/rzpack-format.md`, «Field battles»).
    #[serde(default)]
    pub battle: Option<String>,
}

/// A named leader of an army.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaderSpec {
    pub unit: String,
    #[serde(default = "first_level")]
    pub level: u8,
    /// A text key.
    pub name: String,
}

/// A lantern: a point that shows the map around it. Points with no `active` light nothing at
/// the start.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointSpec {
    /// The point's name, for a campaign's `travel` to enter at it (`campaign.toml`); optional.
    #[serde(default)]
    pub id: Option<String>,
    pub cell: [u32; 2],
    /// Visibility radius at the start, in cells (at most 24).
    #[serde(default)]
    pub radius: u8,
    #[serde(default)]
    pub active: bool,
}

/// How the map is won. Defeat is the original's: the hero falls (his army is lost).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Victory {
    /// Every enemy army is beaten (at most two in this slice: one event checks two armies).
    DefeatAll {
        /// A text key, shown in the victory window.
        #[serde(default)]
        message: Option<String>,
    },
    /// One army is beaten.
    DefeatArmy {
        army: String,
        #[serde(default)]
        message: Option<String>,
    },
    /// The building is the player's.
    Capture {
        building: String,
        #[serde(default)]
        message: Option<String>,
    },
    /// The hero steps onto the cell (an event point with the victory event).
    Reach {
        cell: [u32; 2],
        #[serde(default)]
        message: Option<String>,
    },
}

impl Victory {
    /// The text key of the victory window, if the victory names one.
    pub fn message(&self) -> Option<&str> {
        match self {
            Victory::DefeatAll { message } | Victory::DefeatArmy { message, .. } | Victory::Capture { message, .. } | Victory::Reach { message, .. } => {
                message.as_deref()
            }
        }
    }
}

/// A map of a package as read: its folder id, the path of its `map.toml`, and the parsed file
/// (`None` when it did not parse; the reason is in the report).
#[derive(Clone, Debug)]
pub struct MapEntry {
    pub id: String,
    pub file: String,
    pub spec: Option<MapFile>,
}

/// The start date as `(year, month, day)`: `YYYY-MM-DD` with month 1–12 and day 1–30.
pub fn parse_date(text: &str) -> Result<(u32, u32, u32), String> {
    let parts: Vec<&str> = text.split('-').collect();
    let [y, m, d] = parts.as_slice() else {
        return Err(format!("`{text}` is not a date: write it as YYYY-MM-DD, e.g. 1300-04-01"));
    };
    let num = |s: &str| -> Option<u32> { (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten() };
    let (Some(y), Some(m), Some(d)) = (num(y), num(m), num(d)) else {
        return Err(format!("`{text}` is not a date: write it as YYYY-MM-DD, e.g. 1300-04-01"));
    };
    if !(1..=12).contains(&m) {
        return Err(format!("month {m} in `{text}`: months run 1 to 12 (30-day months)"));
    }
    if !(1..=30).contains(&d) {
        return Err(format!("day {d} in `{text}`: days run 1 to 30 (30-day months)"));
    }
    Ok((y, m, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_victory_reads_its_kind() {
        let v: Victory = toml::from_str("kind = \"defeat_all\"\n").unwrap();
        assert_eq!(v, Victory::DefeatAll { message: None });
        let v: Victory = toml::from_str("kind = \"reach\"\ncell = [3, 4]\nmessage = \"m.text\"\n").unwrap();
        assert_eq!(v, Victory::Reach { cell: [3, 4], message: Some("m.text".into()) });
        assert!(toml::from_str::<Victory>("kind = \"survive\"\n").is_err());
        assert!(toml::from_str::<Victory>("kind = \"defeat_all\"\narmy = \"x\"\n").is_err());
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date("1300-04-01"), Ok((1300, 4, 1)));
        assert!(parse_date("1300-13-01").unwrap_err().contains("months run 1 to 12"));
        assert!(parse_date("1300-01-31").unwrap_err().contains("days run 1 to 30"));
        assert!(parse_date("1300/01/01").is_err());
        assert!(parse_date("-1-01-01").is_err());
    }

    #[test]
    fn unknown_fields_are_errors() {
        let bad = "title = \"t\"\nstart_date = \"1300-01-01\"\nrelations = [[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0]]\nspeed = 2\n\n[hero]\ncell = [1, 1]\nclasses = [\"knight\"]\ngold = 1\n\n[victory]\nkind = \"defeat_all\"\n";
        assert!(toml::from_str::<MapFile>(bad).is_err());
    }
}
