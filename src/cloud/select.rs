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

/// A row of this device that is still in the cloud but no longer chosen. It stays until a newly chosen
/// save needs its room ([`make_room`]): the oldest go first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spare {
    /// The cloud row's id (what the delete call takes).
    pub row: String,
    /// The raw hash of the row's content (the key the sync keeps by).
    pub raw: String,
    pub saved_at: u64,
    pub size: u64,
}

/// The room a new save meets in the account: this device's rows against its cap, every row against the
/// account's cap, and the bytes in use against the quota.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Room {
    pub device_rows: usize,
    pub device_cap: usize,
    pub account_rows: usize,
    pub account_cap: usize,
    pub bytes: u64,
    pub quota: u64,
}

impl Room {
    /// Whether one more row of `size` bytes fits.
    pub fn fits(&self, size: u64) -> bool {
        self.device_rows < self.device_cap && self.account_rows < self.account_cap && self.bytes + size <= self.quota
    }

    /// The room once `row` is deleted.
    pub fn evicted(self, row: &Spare) -> Room {
        Room {
            device_rows: self.device_rows.saturating_sub(1),
            account_rows: self.account_rows.saturating_sub(1),
            bytes: self.bytes.saturating_sub(row.size),
            ..self
        }
    }
}

/// The spare rows to delete, oldest first, so that a save of `size` bytes fits: an empty list when it
/// fits already, `None` when even deleting every spare row would not make room.
pub fn make_room(spare: &[Spare], room: Room, size: u64) -> Option<Vec<Spare>> {
    let mut order: Vec<&Spare> = spare.iter().collect();
    order.sort_by(|a, b| (a.saved_at, &a.row).cmp(&(b.saved_at, &b.row)));
    let mut now = room;
    let mut evict = Vec::new();
    for row in order {
        if now.fits(size) {
            return Some(evict);
        }
        now = now.evicted(row);
        evict.push(row.clone());
    }
    now.fits(size).then_some(evict)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, kind: SaveKind, pinned: bool, saved_at: u64, size: u64) -> Candidate {
        Candidate { id: id.into(), kind, pinned, saved_at, raw_sha: format!("raw-{id}"), size }
    }

    fn spare(row: &str, saved_at: u64, size: u64) -> Spare {
        Spare { row: row.into(), raw: format!("raw-{row}"), saved_at, size }
    }

    /// A room with `rows` of `cap` rows on the device and `bytes` of `quota` used; the account is roomy.
    fn room(rows: usize, cap: usize, bytes: u64, quota: u64) -> Room {
        Room { device_rows: rows, device_cap: cap, account_rows: rows, account_cap: 400, bytes, quota }
    }

    fn rows(list: &[Spare]) -> Vec<&str> {
        list.iter().map(|s| s.row.as_str()).collect()
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

    #[test]
    fn a_new_save_that_fits_evicts_nothing() {
        let spare_rows = vec![spare("old", 10, 5)];
        assert_eq!(make_room(&spare_rows, room(1, 2, 5, 100), 10), Some(vec![]));
    }

    #[test]
    fn the_device_cap_evicts_the_oldest_spare_row_first() {
        // Two spare rows, the cap is full: one eviction makes the room, and it is the oldest (saved 10).
        let spares = vec![spare("mid", 20, 5), spare("old", 10, 5), spare("new", 30, 5)];
        let evict = make_room(&spares, room(3, 3, 15, 100), 5).expect("room is made");
        assert_eq!(rows(&evict), vec!["old"]);
    }

    #[test]
    fn the_quota_evicts_oldest_first_until_the_bytes_fit() {
        // 20 bytes in use of 24: a 10-byte save needs 6 bytes freed, so the oldest (4) is not enough and
        // the next (in age) is taken too.
        let spares = vec![spare("c", 30, 4), spare("a", 10, 4), spare("b", 20, 4)];
        let evict = make_room(&spares, room(3, 10, 20, 24), 10).expect("room is made");
        assert_eq!(rows(&evict), vec!["a", "b"]);
    }

    #[test]
    fn no_room_even_with_every_spare_row_gives_none() {
        // The cap is full of rows that are still chosen: no spare row to give up.
        assert_eq!(make_room(&[], room(2, 2, 0, 100), 1), None);
        // The bytes are full of chosen rows; the spare one is too small to make the room.
        let spares = vec![spare("small", 10, 1)];
        assert_eq!(make_room(&spares, room(2, 10, 99, 100), 5), None);
    }

    #[test]
    fn the_account_row_cap_counts_every_device() {
        // The device has room, the account is full (of other devices' rows): a spare row of this device goes.
        let mut full = room(1, 10, 0, 100);
        full.account_rows = 400;
        full.account_cap = 400;
        let evict = make_room(&[spare("mine", 5, 1)], full, 1).expect("room is made");
        assert_eq!(rows(&evict), vec!["mine"]);
    }
}
