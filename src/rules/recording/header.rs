//! The replay header: what a replay must match before it plays. It holds the Razdor version and
//! a rules fingerprint (the version and the experimental rules), the install and the map (by
//! hash), the platform, and the rule settings a save does not hold (the snapshot's `Settings`).
//! A newer Razdor reads an older replay with the header's facts and checks the states (drift),
//! not the header alone.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::snapshot::Settings;
use crate::rules::content::ExperimentalRules;
use crate::rules::game::Game;
use crate::rules::save::{fnv1a, ScenarioRef};

/// The header's format version.
pub const HEADER_FORMAT: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplayHeader {
    pub format: u32,
    /// The Razdor version that recorded the replay (`CARGO_PKG_VERSION`).
    pub razdor: String,
    /// [`rules_fingerprint`] of the version and the experimental rules it played with.
    pub rules: u64,
    /// [`install_hash`] of the install the game was played on.
    pub install: u64,
    /// The map's file name and the hash of its bytes; none for the demo.
    pub map: Option<String>,
    pub map_hash: Option<u64>,
    /// The platform the replay was recorded on (`os arch bits`).
    pub platform: String,
    pub settings: Settings,
    pub experimental: ExperimentalRules,
}

impl ReplayHeader {
    /// The header of a game being recorded, on the install whose hash is `install`.
    pub fn of(g: &Game, install: u64) -> ReplayHeader {
        let (map, map_hash) = match &g.origin {
            Some(ScenarioRef::Map { file, hash }) => (Some(file.clone()), Some(*hash)),
            _ => (None, None),
        };
        ReplayHeader {
            format: HEADER_FORMAT,
            razdor: env!("CARGO_PKG_VERSION").to_string(),
            rules: rules_fingerprint(&g.experimental),
            install,
            map,
            map_hash,
            platform: platform(),
            settings: Settings::of(g),
            experimental: g.experimental,
        }
    }

    /// Whether this build plays the rules the replay was recorded under (the same version and
    /// experimental rules). A replay recorded under other rules may still play; its drift is
    /// then what the states show.
    pub fn same_rules(&self) -> bool {
        self.rules == rules_fingerprint(&self.experimental)
    }
}

/// A hash of the Razdor version and the experimental rules: the same for two games that play
/// the same rules in the same build.
pub fn rules_fingerprint(experimental: &ExperimentalRules) -> u64 {
    let rules = serde_json::to_string(experimental).expect("the rules serialise");
    fnv1a(format!("razdor {} rules {rules}", env!("CARGO_PKG_VERSION")).as_bytes())
}

/// A hash of the install's maps (their file names and sizes): the same install gives the same
/// value. The install's other files are not read.
pub fn install_hash(dir: &Path) -> Result<u64, String> {
    let maps = crate::dt::install::list_maps(dir).map_err(|e| e.to_string())?;
    let mut text = String::new();
    for m in maps {
        let len = std::fs::metadata(&m.path).map_err(|e| e.to_string())?.len();
        text.push_str(&format!("{}\n{len}\n", m.name));
    }
    Ok(fnv1a(text.as_bytes()))
}

/// The platform a replay is recorded on.
pub fn platform() -> String {
    format!("{} {} {}-bit", std::env::consts::OS, std::env::consts::ARCH, usize::BITS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_follows_the_experimental_rules() {
        let plain = ExperimentalRules::default();
        let mut other = plain;
        other.fast_ai = true;
        assert_eq!(rules_fingerprint(&plain), rules_fingerprint(&ExperimentalRules::default()));
        assert_ne!(rules_fingerprint(&plain), rules_fingerprint(&other));
    }

    #[test]
    fn a_header_round_trips_and_knows_its_rules() {
        let header = ReplayHeader {
            format: HEADER_FORMAT,
            razdor: "9.9.9".into(),
            rules: rules_fingerprint(&ExperimentalRules::default()),
            install: 42,
            map: Some("РК1".into()),
            map_hash: Some(7),
            platform: platform(),
            settings: Settings { improved_ai: true, max_ai: true, max_guard: false, keep_xp_on_promotion: false, keep_formation: true, allow_unit_benching: false, pursue: true, walk_pace: 1.25 },
            experimental: ExperimentalRules::default(),
        };
        let text = serde_json::to_string(&header).unwrap();
        let back: ReplayHeader = serde_json::from_str(&text).unwrap();
        assert_eq!(back, header);
        assert!(back.same_rules());
        let mut changed = back.clone();
        changed.experimental.fast_ai = true;
        assert!(!changed.same_rules(), "other experimental rules are other rules");
    }

    #[test]
    fn the_platform_names_os_arch_and_word_size() {
        let p = platform();
        assert!(p.contains(std::env::consts::OS) && p.ends_with("-bit"), "{p}");
    }
}
