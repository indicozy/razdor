//! The merge rules of the cloud, pure (the server applies the same ones to what it stores):
//! marks merge as a union (the better level wins per map), synced settings take the newest change
//! of each key, and two «Продолжить» saves on one map keep the newer one as the slot and turn the
//! other into an ordinary save named after its device and time.

use razdor_wire::cloud::{MarkLevel, Marks, SyncedSettings};
use razdor_wire::SCHEMA;

/// The rank of a level, the game's order: Green is the best.
fn rank(level: MarkLevel) -> u8 {
    match level {
        MarkLevel::Green => 0,
        MarkLevel::Grey => 1,
        MarkLevel::Yellow => 2,
        MarkLevel::Red => 3,
    }
}

/// Marks merged per map: the better level wins; on a tie the later date. A map completed on any
/// device stays completed.
pub fn merge_marks(local: &Marks, cloud: &Marks, now: u64) -> Marks {
    let mut maps = local.maps.clone();
    for (name, rec) in &cloud.maps {
        let better = match maps.get(name) {
            None => true,
            Some(old) => rank(rec.level) < rank(old.level) || (rank(rec.level) == rank(old.level) && rec.date > old.date),
        };
        if better {
            maps.insert(name.clone(), rec.clone());
        }
    }
    Marks { schema: SCHEMA, updated_at: now.max(local.updated_at).max(cloud.updated_at), maps }
}

/// Synced settings merged per key: the newest change wins; a tie keeps the cloud's value.
pub fn merge_settings(local: &SyncedSettings, cloud: &SyncedSettings) -> SyncedSettings {
    let mut values = local.values.clone();
    for (name, v) in &cloud.values {
        let newer = values.get(name).is_none_or(|old| v.changed_at >= old.changed_at);
        if newer {
            values.insert(name.clone(), v.clone());
        }
    }
    SyncedSettings { schema: SCHEMA, values }
}

/// The name of a session save turned into an ordinary one: «Продолжить (ПК, 21:40)». The time is
/// shown in the device's own zone, given as `offset_secs` from UTC.
pub fn session_name(device: &str, saved_at: u64, offset_secs: i64) -> String {
    let local = (saved_at as i64 + offset_secs).rem_euclid(86_400) as u64;
    format!("Продолжить ({device}, {:02}:{:02})", local / 3600, (local / 60) % 60)
}

/// Two session saves of one map: true when the first is the newer one (it keeps the slot). A tie
/// keeps the first's id order, so every device picks the same one.
pub fn keeps_slot(a_saved_at: u64, a_id: &str, b_saved_at: u64, b_id: &str) -> bool {
    (a_saved_at, a_id) > (b_saved_at, b_id)
}

/// True when `theirs` is a newer Razdor than `ours`: dotted numbers compared in order, so 1.10.0
/// is newer than 1.5.2. A version that does not parse is never newer (the save is not hidden).
pub fn newer_version(theirs: &str, ours: &str) -> bool {
    fn parts(v: &str) -> Option<Vec<u64>> {
        v.split('.').map(|p| p.trim().parse::<u64>().ok()).collect()
    }
    match (parts(theirs), parts(ours)) {
        (Some(t), Some(o)) => {
            for i in 0..t.len().max(o.len()) {
                match t.get(i).copied().unwrap_or(0).cmp(&o.get(i).copied().unwrap_or(0)) {
                    std::cmp::Ordering::Equal => continue,
                    order => return order == std::cmp::Ordering::Greater,
                }
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor_wire::cloud::{MarkRecord, SettingValue};
    use std::collections::BTreeMap;

    fn rec(level: MarkLevel, date: &str) -> MarkRecord {
        MarkRecord { level, date: date.into(), mark: serde_json::json!({}) }
    }

    fn marks(maps: Vec<(&str, MarkRecord)>) -> Marks {
        Marks { schema: 1, updated_at: 0, maps: maps.into_iter().map(|(k, v)| (k.to_string(), v)).collect() }
    }

    #[test]
    fn marks_are_a_union_with_the_better_level_winning() {
        let a = marks(vec![("one", rec(MarkLevel::Grey, "2026-01-01")), ("two", rec(MarkLevel::Red, "2026-01-01"))]);
        let b = marks(vec![("one", rec(MarkLevel::Green, "2025-01-01")), ("three", rec(MarkLevel::Yellow, "2026-02-02"))]);
        let m = merge_marks(&a, &b, 500);
        assert_eq!(m.maps.len(), 3, "every map of both devices stays");
        assert_eq!(m.maps["one"].level, MarkLevel::Green);
        assert_eq!(m.maps["two"].level, MarkLevel::Red);
        assert_eq!(m.updated_at, 500);
        // The same level: the later date wins.
        let c = marks(vec![("one", rec(MarkLevel::Grey, "2026-05-05"))]);
        assert_eq!(merge_marks(&a, &c, 0).maps["one"].date, "2026-05-05");
        // Merging is symmetric in what it keeps.
        assert_eq!(merge_marks(&b, &a, 0).maps["one"].level, MarkLevel::Green);
    }

    #[test]
    fn settings_take_the_newest_change_per_key() {
        let set = |pairs: Vec<(&str, &str, u64)>| SyncedSettings {
            schema: 1,
            values: pairs
                .into_iter()
                .map(|(k, v, t)| (k.to_string(), SettingValue { value: v.into(), changed_at: t }))
                .collect::<BTreeMap<_, _>>(),
        };
        let local = set(vec![("language", "ru", 10), ("hints", "off", 50)]);
        let cloud = set(vec![("language", "en", 20), ("hints", "on", 5), ("speed", "2", 1)]);
        let m = merge_settings(&local, &cloud);
        assert_eq!(m.values["language"].value, "en", "the cloud's change is newer");
        assert_eq!(m.values["hints"].value, "off", "the local change is newer");
        assert_eq!(m.values["speed"].value, "2", "a key only the cloud has is kept");
    }

    #[test]
    fn a_session_name_shows_the_device_and_its_local_time() {
        // 21:40 UTC is 00:40 the next day at UTC+3.
        let saved = 21 * 3600 + 40 * 60;
        assert_eq!(session_name("ПК", saved, 0), "Продолжить (ПК, 21:40)");
        assert_eq!(session_name("телефон", saved, 3 * 3600), "Продолжить (телефон, 00:40)");
        assert_eq!(session_name("ПК", saved, -22 * 3600), "Продолжить (ПК, 23:40)");
    }

    #[test]
    fn the_newer_session_keeps_the_slot_and_a_tie_is_decided_by_id() {
        assert!(keeps_slot(200, "a", 100, "b"));
        assert!(!keeps_slot(100, "b", 200, "a"));
        assert!(keeps_slot(100, "b", 100, "a"));
        assert!(!keeps_slot(100, "a", 100, "b"));
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(newer_version("1.10.0", "1.5.2"));
        assert!(!newer_version("1.5.2", "1.10.0"));
        assert!(newer_version("1.5.3", "1.5.2"));
        assert!(!newer_version("1.5.2", "1.5.2"));
        assert!(newer_version("1.5.2.1", "1.5.2"));
        assert!(!newer_version("garbage", "1.5.2"), "an unreadable version is not newer");
    }
}
