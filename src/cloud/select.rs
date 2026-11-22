//! Which local saves go to the cloud, and what state each one shows in the load list. Pure: the
//! caller passes the saves it has. The per-map limits are the wire's ([`per_map_limit`]).

use std::collections::BTreeMap;

use razdor_wire::cloud::{per_map_limit, SaveKind};

/// A local save as the selection sees it. `id` is the caller's own key for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub game: String,
    pub map: String,
    pub kind: SaveKind,
    pub pinned: bool,
    pub saved_at: u64,
}

/// One save's state for the load list: ☁ in the cloud, ⏳ waiting to upload, 💾 on this device only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveState {
    InCloud,
    WaitingUpload,
    LocalOnly,
}

/// The state of a local save: `in_cloud` when the account holds the same file, `selected` when the
/// per-map window includes it (so it is due for upload).
pub fn state(in_cloud: bool, selected: bool) -> SaveState {
    if in_cloud {
        SaveState::InCloud
    } else if selected {
        SaveState::WaitingUpload
    } else {
        SaveState::LocalOnly
    }
}

/// The saves that go to the cloud: per game, map and kind, the pinned first and then the newest,
/// up to the kind's limit (3 manual, 3 quick, 1 session). Autosaves are not candidates. Returns the
/// chosen ids, in the order of the groups (game, map, kind) and then the rank in the group.
pub fn choose(candidates: &[Candidate]) -> Vec<String> {
    let mut groups: BTreeMap<(String, String, u8), Vec<&Candidate>> = BTreeMap::new();
    for c in candidates {
        groups.entry((c.game.clone(), c.map.clone(), kind_rank(c.kind))).or_default().push(c);
    }
    let mut chosen = Vec::new();
    for ((_, _, _), mut group) in groups {
        group.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then(b.saved_at.cmp(&a.saved_at))
                .then(a.id.cmp(&b.id))
        });
        let limit = per_map_limit(group[0].kind);
        chosen.extend(group.iter().take(limit).map(|c| c.id.clone()));
    }
    chosen
}

/// A stable order for the kinds (the wire's enum has none).
pub fn kind_rank(kind: SaveKind) -> u8 {
    match kind {
        SaveKind::Manual => 0,
        SaveKind::Quick => 1,
        SaveKind::Session => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, map: &str, kind: SaveKind, pinned: bool, saved_at: u64) -> Candidate {
        Candidate { id: id.into(), game: "dt".into(), map: map.into(), kind, pinned, saved_at }
    }

    #[test]
    fn three_manual_saves_per_map_newest_first_and_pinned_first() {
        let saves: Vec<Candidate> = (0..5).map(|i| c(&format!("m{i}"), "map1", SaveKind::Manual, false, 100 + i)).collect();
        let chosen = choose(&saves);
        assert_eq!(chosen, vec!["m4", "m3", "m2"]);
        // A pinned old save goes up even when it is the oldest.
        let mut pinned = saves.clone();
        pinned[0].pinned = true;
        let chosen = choose(&pinned);
        assert_eq!(chosen[0], "m0");
        assert_eq!(chosen.len(), 3);
        assert!(chosen.contains(&"m4".to_string()));
    }

    #[test]
    fn each_kind_and_each_map_has_its_own_window() {
        let mut saves = Vec::new();
        for i in 0..4 {
            saves.push(c(&format!("q{i}"), "map1", SaveKind::Quick, false, 10 + i));
            saves.push(c(&format!("m{i}"), "map1", SaveKind::Manual, false, 10 + i));
            saves.push(c(&format!("m{i}b"), "map2", SaveKind::Manual, false, 10 + i));
        }
        saves.push(c("s1", "map1", SaveKind::Session, false, 1));
        saves.push(c("s2", "map1", SaveKind::Session, false, 2));
        let chosen = choose(&saves);
        let count = |id: &str| chosen.iter().filter(|x| x.starts_with(id)).count();
        assert_eq!(chosen.iter().filter(|x| x.starts_with('q')).count(), 3, "quick: 3 per map");
        assert_eq!(chosen.iter().filter(|x| x.starts_with('m') && !x.ends_with('b')).count(), 3, "manual: 3 on map1");
        assert_eq!(chosen.iter().filter(|x| x.ends_with('b')).count(), 3, "manual: 3 on map2");
        assert_eq!(count("s"), 1, "session: one slot, the newest");
        assert!(chosen.contains(&"s2".to_string()));
    }

    #[test]
    fn states_follow_the_cloud_and_the_window() {
        assert_eq!(state(true, true), SaveState::InCloud);
        assert_eq!(state(true, false), SaveState::InCloud);
        assert_eq!(state(false, true), SaveState::WaitingUpload);
        assert_eq!(state(false, false), SaveState::LocalOnly);
    }

    #[test]
    fn autosaves_never_go_up() {
        // The caller only passes manual, quick and session saves; the kinds it lists are the only ones.
        let saves = vec![c("a", "map1", SaveKind::Manual, false, 1)];
        assert_eq!(choose(&saves), vec!["a"]);
        assert_eq!(choose(&[]), Vec::<String>::new());
    }
}
