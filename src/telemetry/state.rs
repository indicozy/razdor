//! The analytics' own state file (`<app data>/telemetry/state.json`): the install id, the consent,
//! the version the question was last asked in, the day of the last ping, and the crash reports' server
//! ids. Kept apart from the saves and from the settings (the settings may sync between devices later).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The player's answer to «Помочь Раздору?». `Unknown` until the question is answered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Consent {
    #[default]
    Unknown,
    Yes,
    No,
}

/// What the file holds. Every field has a default, so an older or a partial file still reads.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub install_id: Option<String>,
    #[serde(default)]
    pub consent: Consent,
    /// The Razdor version the question was last asked in (asked again after an update).
    #[serde(default)]
    pub asked_version: Option<String>,
    /// The UTC day (days since 1970) of the last daily ping that was queued.
    #[serde(default)]
    pub last_ping_day: Option<u64>,
    /// The server's id of each crash report sent, by the local token (the queue item's seq).
    #[serde(default)]
    pub crash_ids: BTreeMap<String, String>,
}

/// The state file in `dir`. A missing or unreadable file is the default state.
pub fn load(dir: &Path) -> State {
    std::fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Writes the state under a temporary name, then renames it (a crash mid-write keeps the old file).
pub fn save(dir: &Path, state: &State) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?;
    let tmp = dir.join("state.json.part");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, dir.join(FILE))
}

const FILE: &str = "state.json";

/// A fresh random install id: 32 lower-case hex digits (`razdor_wire::InstallId`). From the system's
/// random source where there is one, else the std hasher's per-process random keys.
pub fn new_install_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut bytes = [0u8; 16];
    let from_system = std::fs::File::open("/dev/urandom").and_then(|mut f| {
        use std::io::Read;
        f.read_exact(&mut bytes)
    });
    if from_system.is_err() {
        for chunk in bytes.chunks_mut(8) {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
            chunk.copy_from_slice(&h.finish().to_le_bytes()[..chunk.len()]);
        }
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The folder of the analytics' files (`<app data>/telemetry`); `None` when the platform has none.
pub fn telemetry_dir() -> Option<PathBuf> {
    crate::platform::app_dir().map(|d| d.join("telemetry"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_install_id_is_32_lower_case_hex_digits_and_differs_each_time() {
        let a = new_install_id();
        let b = new_install_id();
        assert_eq!(a.len(), 32);
        assert!(razdor_wire::InstallId::parse(&a).is_ok(), "{a}");
        assert_ne!(a, b);
    }

    #[test]
    fn the_state_round_trips_and_a_missing_file_is_the_default() {
        let dir = std::env::temp_dir().join(format!("razdor-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&dir), State::default());
        let s = State { install_id: Some("ab".repeat(16)), consent: Consent::Yes, last_ping_day: Some(20_000), ..State::default() };
        save(&dir, &s).unwrap();
        assert_eq!(load(&dir), s);
        std::fs::write(dir.join(FILE), b"not json").unwrap();
        assert_eq!(load(&dir), State::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
