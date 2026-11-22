//! Which local saves this device mirrors to the cloud, and what state each one shows in the load
//! list. Pure: the caller passes the saves it has. The cap is per device (remote config
//! `cloud_saves_per_device`) and the account's quota is shared by all devices, so the caller says how
//! many bytes are left for this device.

use razdor_wire::cloud::SaveKind;

/// A local save as the selection sees it. `id` is the caller's own key for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub kind: SaveKind,
    pub pinned: bool,
    pub saved_at: u64,
    /// The hash of the raw save content: two local saves with one hash are one cloud save.
    pub raw_sha: String,
    /// The packed size (after zstd): what the save takes of the account's quota.
    pub size: u64,
}

/// One save's state for the load list: ☁ in the cloud, ⏳ waiting to upload, 💾 on this device only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveState {
    InCloud,
    WaitingUpload,
    LocalOnly,
}

/// The state of a local save: `in_cloud` when this device's cloud rows hold the same content,
/// `selected` when the mirror includes it (so it is due for upload).
pub fn state(in_cloud: bool, selected: bool) -> SaveState {
    if in_cloud {
        SaveState::InCloud
    } else if selected {
        SaveState::WaitingUpload
    } else {
        SaveState::LocalOnly
    }
}

/// The order a save takes in the mirror: pinned first, then the «Продолжить» slot, then the newest
/// (by the time of saving), then the id, so every run picks the same ones.
pub fn rank(a: &Candidate, b: &Candidate) -> std::cmp::Ordering {
    (!a.pinned)
        .cmp(&!b.pinned)
        .then((a.kind != SaveKind::Session).cmp(&(b.kind != SaveKind::Session)))
        .then(b.saved_at.cmp(&a.saved_at))
        .then(a.id.cmp(&b.id))
}

/// The saves this device mirrors, in [`rank`] order: at most `cap`, at most `budget` bytes in all.
/// A save that would pass the budget is skipped (it stays local), and the rest go on to fill the
/// room. Two saves with one raw hash count once: the better-ranked one goes up.
pub fn choose(candidates: &[Candidate], cap: usize, budget: u64) -> Vec<String> {
    let mut ranked: Vec<&Candidate> = candidates.iter().collect();
    ranked.sort_by(|a, b| rank(a, b));
    let mut chosen = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    let mut used: u64 = 0;
    for c in ranked {
        if chosen.len() >= cap {
            break;
        }
        if seen.contains(&c.raw_sha.as_str()) {
            continue;
        }
        if used + c.size > budget {
            continue;
        }
        used += c.size;
        seen.push(&c.raw_sha);
        chosen.push(c.id.clone());
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, kind: SaveKind, pinned: bool, saved_at: u64, size: u64) -> Candidate {
        Candidate { id: id.into(), kind, pinned, saved_at, raw_sha: format!("raw-{id}"), size }
    }

    #[test]
    fn pinned_first_then_the_session_slot_then_the_newest() {
        let saves = vec![
            c("old", SaveKind::Manual, false, 10, 1),
            c("new", SaveKind::Quick, false, 30, 1),
            c("session", SaveKind::Session, false, 5, 1),
            c("pinned-old", SaveKind::Manual, true, 1, 1),
        ];
        assert_eq!(choose(&saves, 10, 100), vec!["pinned-old", "session", "new", "old"]);
    }

    #[test]
    fn the_cap_is_per_device_not_per_map_or_kind() {
        // Five maps, four saves each: with a cap of 7, the seven newest go up, whatever map or kind.
        let mut saves = Vec::new();
        for m in 0..5u64 {
            for i in 0..4u64 {
                saves.push(c(&format!("m{m}-{i}"), SaveKind::Manual, false, m * 10 + i, 1));
            }
        }
        let chosen = choose(&saves, 7, 100);
        assert_eq!(chosen.len(), 7);
        assert_eq!(chosen[0], "m4-3");
        assert!(!chosen.contains(&"m0-0".to_string()));
    }

    #[test]
    fn the_quota_budget_skips_what_does_not_fit_and_fills_the_rest() {
        let saves = vec![c("big", SaveKind::Manual, false, 30, 8), c("mid", SaveKind::Manual, false, 20, 3), c("small", SaveKind::Manual, false, 10, 2)];
        // Ten bytes left: the big one (newest) fits, then mid (3) does not fit the 2 left, small does.
        assert_eq!(choose(&saves, 10, 10), vec!["big", "small"]);
        assert_eq!(choose(&saves, 10, 0), Vec::<String>::new());
    }

    #[test]
    fn one_raw_hash_is_one_save() {
        let mut twin = c("twin", SaveKind::Manual, false, 40, 1);
        twin.raw_sha = "raw-a".into();
        let mut a = c("a", SaveKind::Manual, false, 20, 1);
        a.raw_sha = "raw-a".into();
        assert_eq!(choose(&[a, twin], 10, 10), vec!["twin"], "the better-ranked copy goes up, once");
    }

    #[test]
    fn states_follow_the_cloud_and_the_mirror() {
        assert_eq!(state(true, true), SaveState::InCloud);
        assert_eq!(state(true, false), SaveState::InCloud);
        assert_eq!(state(false, true), SaveState::WaitingUpload);
        assert_eq!(state(false, false), SaveState::LocalOnly);
    }
}
