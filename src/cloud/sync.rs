//! The sync: this device's mirror in the account, over the [`Api`] and a [`Local`] store.
//!
//! The mirror: the cloud rows of this device are the saves it has chosen ([`select::choose`]: pinned,
//! then «Продолжить», then the newest; at most the device's cap and the account's quota). Push uploads
//! the chosen saves the account lacks, and deletes this device's own rows whose content is gone from the
//! device (a save deleted or overwritten here). A save only pushed out of the chosen set (over the cap or
//! the quota) keeps its cloud row until a new save needs the room: then the oldest such rows go, just
//! enough ([`select::make_room`]). Pull lists the other devices' saves
//! (metadata only) and the saves of a newer Razdor (listed locked). Another device's save is
//! downloaded only on demand ([`download`]), and then it is an ordinary local save. Nothing another
//! device wrote is ever changed or deleted here.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use razdor_wire::cloud::{BenchReport, HubList, Marks, SaveEntry, SaveKind, SaveMeta, SyncedSettings};
use razdor_wire::limits::{CLOUD_BYTES_PER_ACCOUNT, SAVES_PER_ACCOUNT};
use razdor_wire::SCHEMA;

use super::api::{Api, CloudError};
use super::merge::{merge_marks, merge_settings, newer_version, session_name};
use super::pack::{pack, sha256_hex, unpack};
use super::select::{choose, make_room, state, Candidate, Room, SaveState, Spare};

/// This build's Razdor version (the saves it makes carry it).
pub const RAZDOR: &str = env!("CARGO_PKG_VERSION");

/// A local save as the sync sees it. `id` is the store's own key for it (the file path, say).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalSave {
    pub id: String,
    pub game: String,
    pub map: String,
    /// Manual, quick or session. Autosaves are never given to the sync.
    pub kind: SaveKind,
    pub name: String,
    pub saved_at: u64,
    pub pinned: bool,
    /// The device that made the save («ПК», «телефон») for a save downloaded from another device; empty
    /// for a save made here.
    pub device: String,
}

/// This device's saves and synced documents. The game's own store implements it: the save folders,
/// `marks.json`, the settings file and the Hub and benchmark records.
pub trait Local: Send {
    /// This device's random id (32 lowercase hex, kept on the device); its rows are the only ones it
    /// uploads and deletes. Empty when the device could not keep one: nothing is pushed then.
    fn device_id(&self) -> String;
    /// This device's name as the player gave it («ПК», «телефон»).
    fn device(&self) -> String;
    /// The device's zone offset from UTC in seconds, for the time shown in a session's name.
    fn offset_secs(&self) -> i64;
    /// How many cloud saves this device mirrors (`cloud_saves_per_device`).
    fn saves_per_device(&self) -> usize;
    /// Every manual, quick and session save on the device.
    fn saves(&self) -> Vec<LocalSave>;
    /// A save file's bytes, or None when it cannot be read.
    fn read(&self, id: &str) -> Option<Vec<u8>>;
    /// Stores a save file (downloaded from the cloud) as a local save; returns its id.
    fn store(&mut self, save: &LocalSave, bytes: &[u8]) -> Result<String, String>;
    fn marks(&self) -> Marks;
    fn set_marks(&mut self, marks: Marks);
    fn settings(&self) -> SyncedSettings;
    fn set_settings(&mut self, settings: SyncedSettings);
    /// This device's installed Hub items (`updated_at` 0: none recorded yet, nothing is sent).
    fn hub(&self) -> HubList;
    /// Another device's Hub list, offered («Установить то же самое?»); files come from the Hub.
    fn set_hub_offer(&mut self, list: HubList);
    /// Benchmark reports not sent yet.
    fn bench_pending(&self) -> Vec<BenchReport>;
    /// A report was sent (by its time): it is not pending any more.
    fn bench_sent(&mut self, at: u64);
}

/// What one round did, and what the account holds for the load list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Cloud saves uploaded this round.
    pub pushed: usize,
    /// This device's cloud rows deleted this round: a save deleted or overwritten here, or a kept row evicted
    /// because a newly chosen save needed its room.
    pub deleted: usize,
    /// The round stopped because the account is full (the cap or the quota).
    pub full: bool,
    /// The errors of single saves; the round went on past them.
    pub errors: Vec<String>,
    /// Other devices' saves this device does not hold: listed, downloaded on demand.
    pub others: Vec<SaveEntry>,
    /// Saves of a newer Razdor (any device's): listed locked, never downloaded or deleted.
    pub newer: Vec<SaveEntry>,
    /// Each local save's state after the round, by the store's id (the load list's icons).
    pub states: HashMap<String, SaveState>,
    /// The account's bytes in use (after zstd) at the last list.
    pub used_bytes: u64,
}

/// A local save with its packed bytes and both hashes: `raw` is the save's identity (the raw content),
/// `sha` the hash of the packed bytes the server checks.
struct Held {
    save: LocalSave,
    packed: Vec<u8>,
    raw: String,
    sha: String,
}

/// The local saves that can be read and packed, and how many could not be (their cloud rows are then
/// never deleted in a round: their hashes are unknown).
fn hold(local: &dyn Local) -> (Vec<Held>, usize) {
    let saves = local.saves();
    let total = saves.len();
    let held: Vec<Held> = saves
        .into_iter()
        .filter_map(|save| {
            let bytes = local.read(&save.id)?;
            let packed = pack(&bytes).ok()?;
            Some(Held { raw: sha256_hex(&bytes), sha: sha256_hex(&packed), packed, save })
        })
        .collect();
    let unreadable = total - held.len();
    (held, unreadable)
}

fn is_newer(entry: &SaveEntry) -> bool {
    newer_version(&entry.meta.razdor, RAZDOR)
}

/// The other devices' saves this device does not hold (by raw hash), and never a newer Razdor's.
fn others_of(list: &[SaveEntry], own: &str, held: &[Held]) -> Vec<SaveEntry> {
    list.iter()
        .filter(|e| e.meta.device_id != own && !is_newer(e))
        .filter(|e| e.meta.raw_sha256.is_empty() || !held.iter().any(|h| h.raw == e.meta.raw_sha256))
        .cloned()
        .collect()
}

/// The error of one save: a per-save problem is recorded and the round goes on; anything about the
/// connection or the account stops the round.
fn one_save(report: &mut Report, err: CloudError) -> Result<(), CloudError> {
    match err {
        CloudError::Bad(_) | CloudError::NotFound | CloudError::Refused(_) | CloudError::Conflict(_) => {
            report.errors.push(err.to_string());
            Ok(())
        }
        other => Err(other),
    }
}

/// The unix time now (seconds).
pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Lists the account's saves: the other devices' saves this device lacks, and the newer Razdor's.
/// Downloads nothing. The «Облако» tab calls it when it opens.
pub fn refresh(api: &Api, local: &dyn Local) -> Result<Report, CloudError> {
    let list = api.list_saves()?;
    let own = local.device_id();
    let (held, _) = hold(local);
    Ok(Report {
        others: others_of(&list.saves, &own, &held),
        newer: list.saves.iter().filter(|e| is_newer(e)).cloned().collect(),
        used_bytes: list.used_bytes,
        ..Report::default()
    })
}

/// Pulls: the list (see [`refresh`]), the marks, the settings and the Hub offer.
pub fn pull(api: &Api, local: &mut dyn Local) -> Result<Report, CloudError> {
    let report = refresh(api, local)?;
    let cloud_marks = api.get_marks()?;
    let merged = merge_marks(&local.marks(), &cloud_marks, now_secs());
    local.set_marks(merged);
    let cloud_settings = api.get_settings()?;
    let merged = merge_settings(&local.settings(), &cloud_settings);
    local.set_settings(merged);
    let hub = api.get_hub()?;
    if !hub.items.is_empty() {
        local.set_hub_offer(hub);
    }
    Ok(report)
}

/// This device's rows, sorted. `keep` holds the content of every row kept (the chosen ones, and the
/// spare ones kept until a new save needs their room), `spare` the rows kept but not chosen, `stale` the
/// rows to delete now: their content is no longer on the device, or it is a second row of content
/// already kept. While a local save cannot be read nothing is deleted (its hash is unknown): every row is kept.
struct Rows<'a> {
    keep: HashSet<String>,
    spare: Vec<Spare>,
    stale: Vec<&'a SaveEntry>,
}

fn sort_rows<'a>(mine: &[&'a SaveEntry], local: &HashSet<&str>, wanted: &HashSet<&str>, unreadable: usize) -> Rows<'a> {
    let mut out = Rows { keep: HashSet::new(), spare: Vec::new(), stale: Vec::new() };
    for &e in mine {
        let raw = e.meta.raw_sha256.as_str();
        if unreadable > 0 {
            out.keep.insert(raw.to_string());
        } else if !local.contains(raw) || out.keep.contains(raw) {
            out.stale.push(e);
        } else {
            out.keep.insert(raw.to_string());
            if !wanted.contains(raw) {
                out.spare.push(Spare { row: e.id.clone(), raw: raw.to_string(), saved_at: e.meta.saved_at, size: e.meta.size });
            }
        }
    }
    out
}

/// Pushes this device's mirror. Its rows whose content is gone from the device are deleted first. The
/// chosen saves the account lacks are uploaded; a row kept but no longer chosen (over the cap, or pushed
/// out by the quota) stays in the cloud until a save needs its room: then the oldest such rows go, just
/// enough to make it. Then the marks, the settings, the Hub list and the benchmark reports.
pub fn push(api: &Api, local: &mut dyn Local) -> Result<Report, CloudError> {
    let own = local.device_id();
    if own.is_empty() {
        return Err(CloudError::Refused("this device has no id".into()));
    }
    let list = api.list_saves()?;
    let mut report = Report { used_bytes: list.used_bytes, ..Report::default() };
    let (held, unreadable) = hold(local);

    // The rows this device manages: its own and not a newer Razdor's (those are listed, never touched).
    // Every other row takes its room from the cap and the quota first.
    let mine: Vec<&SaveEntry> = list.saves.iter().filter(|e| e.meta.device_id == own && !is_newer(e)).collect();
    let own_newer = list.saves.iter().filter(|e| e.meta.device_id == own && is_newer(e)).count();
    let own_rows = list.saves.iter().filter(|e| e.meta.device_id == own).count();
    let others_bytes: u64 = list.saves.iter().filter(|e| e.meta.device_id != own || is_newer(e)).map(|e| e.meta.size).sum();
    let cap = local.saves_per_device();
    let budget = CLOUD_BYTES_PER_ACCOUNT.saturating_sub(others_bytes);

    let candidates: Vec<Candidate> = held
        .iter()
        .map(|h| Candidate {
            id: h.save.id.clone(),
            kind: h.save.kind,
            pinned: h.save.pinned,
            saved_at: h.save.saved_at,
            raw_sha: h.raw.clone(),
            size: h.packed.len() as u64,
        })
        .collect();
    let chosen = choose(&candidates, cap.saturating_sub(own_newer), budget);
    let chosen_ids: HashSet<&str> = chosen.iter().map(String::as_str).collect();
    let wanted: HashSet<&str> = held.iter().filter(|h| chosen_ids.contains(h.save.id.as_str())).map(|h| h.raw.as_str()).collect();
    let local_raws: HashSet<&str> = held.iter().map(|h| h.raw.as_str()).collect();

    let Rows { mut keep, mut spare, stale } = sort_rows(&mine, &local_raws, &wanted, unreadable);
    let mut room = Room {
        device_rows: own_rows,
        device_cap: cap,
        account_rows: list.saves.len(),
        account_cap: SAVES_PER_ACCOUNT,
        bytes: list.used_bytes,
        quota: CLOUD_BYTES_PER_ACCOUNT,
    };
    for e in stale {
        let gone = match api.delete_save(&e.id) {
            Ok(()) => {
                report.deleted += 1;
                true
            }
            Err(CloudError::NotFound) => true,
            Err(err) => {
                one_save(&mut report, err)?;
                false
            }
        };
        if gone {
            room.device_rows = room.device_rows.saturating_sub(1);
            room.account_rows = room.account_rows.saturating_sub(1);
            room.bytes = room.bytes.saturating_sub(e.meta.size);
        }
    }

    let by_id: HashMap<&str, &Held> = held.iter().map(|h| (h.save.id.as_str(), h)).collect();
    for id in &chosen {
        let Some(h) = by_id.get(id.as_str()) else { continue };
        if keep.contains(&h.raw) {
            continue;
        }
        let size = h.packed.len() as u64;
        // Room for this save: the oldest kept-but-not-chosen rows go, and only as many as it takes.
        let Some(evict) = make_room(&spare, room, size) else {
            report.full = true;
            break;
        };
        let mut blocked = false;
        for s in &evict {
            let gone = match api.delete_save(&s.row) {
                Ok(()) => {
                    report.deleted += 1;
                    true
                }
                Err(CloudError::NotFound) => true,
                Err(err) => {
                    one_save(&mut report, err)?;
                    blocked = true;
                    break;
                }
            };
            if gone {
                room = room.evicted(s);
                keep.remove(&s.raw);
                spare.retain(|x| x.row != s.row);
            }
        }
        if blocked {
            break;
        }
        let meta = SaveMeta {
            schema: SCHEMA,
            game: h.save.game.clone(),
            map: h.save.map.clone(),
            kind: h.save.kind,
            device_id: own.clone(),
            device: local.device(),
            name: h.save.name.clone(),
            saved_at: h.save.saved_at,
            razdor: RAZDOR.to_string(),
            size,
            sha256: h.sha.clone(),
            raw_sha256: h.raw.clone(),
            pinned: h.save.pinned,
        };
        match api.put_save(&meta, &h.packed) {
            Ok(put) => {
                room.bytes = put.used_bytes;
                room.device_rows += 1;
                room.account_rows += 1;
                report.pushed += 1;
                keep.insert(h.raw.clone());
            }
            Err(CloudError::Full) => {
                report.full = true;
                break;
            }
            Err(e) => one_save(&mut report, e)?,
        }
    }
    report.used_bytes = room.bytes;

    if !report.full {
        let merged = api.put_marks(&local.marks())?;
        local.set_marks(merged);
        let merged = api.put_settings(&local.settings())?;
        local.set_settings(merged);
        let hub = local.hub();
        if hub.updated_at > 0 {
            api.put_hub(&hub)?;
        }
        for r in local.bench_pending() {
            api.post_bench(&r)?;
            local.bench_sent(r.at);
        }
    }

    report.states = held
        .iter()
        .map(|h| (h.save.id.clone(), state(keep.contains(&h.raw), chosen_ids.contains(h.save.id.as_str()))))
        .collect();
    report.others = others_of(&list.saves, &own, &held);
    report.newer = list.saves.iter().filter(|e| is_newer(e)).cloned().collect();
    Ok(report)
}

/// A full round: pull, then push. The push's report carries the list and the states.
pub fn sync(api: &Api, local: &mut dyn Local) -> Result<Report, CloudError> {
    pull(api, local)?;
    push(api, local)
}

/// Downloads another device's save on demand (the «Облако» tab's load) and keeps it as an ordinary
/// local save: it then joins this device's mirror on the next push. Returns the local save's id. A
/// save this device already holds (same content) is not written twice: its id comes back. A newer
/// Razdor's save and this device's own rows are refused.
pub fn download(api: &Api, local: &mut dyn Local, id: &str) -> Result<String, CloudError> {
    let own = local.device_id();
    let (meta, packed) = api.get_save(id)?;
    if newer_version(&meta.razdor, RAZDOR) {
        return Err(CloudError::Refused("a newer Razdor's save: update Razdor first".into()));
    }
    if !own.is_empty() && meta.device_id == own {
        return Err(CloudError::Refused("this device's own save".into()));
    }
    let bytes = unpack(&packed).map_err(CloudError::Bad)?;
    if !meta.raw_sha256.is_empty() && sha256_hex(&bytes) != meta.raw_sha256 {
        return Err(CloudError::Bad("the save's content differs from its raw hash".into()));
    }
    let (held, _) = hold(local);
    if let Some(h) = held.iter().find(|h| !meta.raw_sha256.is_empty() && h.raw == meta.raw_sha256) {
        return Ok(h.save.id.clone());
    }
    // A session slot is one per device: another device's «Продолжить» lands as an ordinary save.
    let (kind, name) = if meta.kind == SaveKind::Session {
        (SaveKind::Manual, session_name(&meta.device, meta.saved_at, local.offset_secs()))
    } else {
        (meta.kind, meta.name.clone())
    };
    let save = LocalSave {
        id: String::new(),
        game: meta.game.clone(),
        map: meta.map.clone(),
        kind,
        name,
        saved_at: meta.saved_at,
        pinned: meta.pinned,
        device: meta.device.clone(),
    };
    local.store(&save, &bytes).map_err(CloudError::Refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, raw: &str, saved_at: u64, size: u64) -> SaveEntry {
        SaveEntry {
            id: id.into(),
            meta: SaveMeta {
                schema: SCHEMA,
                game: "dt".into(),
                map: "ruins".into(),
                kind: SaveKind::Manual,
                device_id: "own".into(),
                device: "ПК".into(),
                name: id.into(),
                saved_at,
                razdor: RAZDOR.into(),
                size,
                sha256: format!("packed-{id}"),
                raw_sha256: raw.into(),
                pinned: false,
            },
        }
    }

    fn set<'a>(items: &[&'a str]) -> HashSet<&'a str> {
        items.iter().copied().collect()
    }

    #[test]
    fn a_row_whose_content_is_gone_from_the_device_is_deleted_now() {
        let rows = [entry("row-gone", "raw-gone", 10, 5)];
        let mine: Vec<&SaveEntry> = rows.iter().collect();
        let out = sort_rows(&mine, &set(&["raw-other"]), &set(&["raw-other"]), 0);
        assert_eq!(out.stale.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["row-gone"]);
        assert!(out.keep.is_empty() && out.spare.is_empty());
    }

    #[test]
    fn a_row_still_on_the_device_but_not_chosen_is_kept_as_spare() {
        // Its content is local and not chosen (over the cap): it stays in the cloud, nothing is deleted.
        let rows = [entry("row-old", "raw-old", 10, 5)];
        let mine: Vec<&SaveEntry> = rows.iter().collect();
        let out = sort_rows(&mine, &set(&["raw-old"]), &set(&[]), 0);
        assert!(out.stale.is_empty(), "nothing is deleted for being over the cap");
        assert!(out.keep.contains("raw-old"), "still in the cloud: the state is InCloud");
        assert_eq!(out.spare, vec![Spare { row: "row-old".into(), raw: "raw-old".into(), saved_at: 10, size: 5 }]);
    }

    #[test]
    fn a_chosen_row_is_kept_and_is_not_spare() {
        let rows = [entry("row-new", "raw-new", 30, 5)];
        let mine: Vec<&SaveEntry> = rows.iter().collect();
        let out = sort_rows(&mine, &set(&["raw-new"]), &set(&["raw-new"]), 0);
        assert!(out.stale.is_empty() && out.spare.is_empty());
        assert!(out.keep.contains("raw-new"));
    }

    #[test]
    fn a_second_row_of_one_content_is_deleted_now() {
        let rows = [entry("first", "raw-a", 10, 5), entry("second", "raw-a", 20, 5)];
        let mine: Vec<&SaveEntry> = rows.iter().collect();
        let out = sort_rows(&mine, &set(&["raw-a"]), &set(&["raw-a"]), 0);
        assert_eq!(out.stale.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["second"]);
        assert!(out.keep.contains("raw-a"));
    }

    #[test]
    fn an_unreadable_local_save_deletes_nothing() {
        let rows = [entry("row-gone", "raw-gone", 10, 5), entry("row-old", "raw-old", 20, 5)];
        let mine: Vec<&SaveEntry> = rows.iter().collect();
        let out = sort_rows(&mine, &set(&[]), &set(&[]), 1);
        assert!(out.stale.is_empty(), "the hash of the unreadable save is unknown: nothing goes");
        assert!(out.spare.is_empty(), "and nothing is evicted for room either");
        assert_eq!(out.keep.len(), 2);
    }

    #[test]
    fn the_spare_rows_go_oldest_first_when_a_new_save_needs_room() {
        // Three kept-but-not-chosen rows at the device's cap: one new save evicts only the oldest.
        let rows = [entry("mid", "raw-mid", 20, 5), entry("old", "raw-old", 10, 5), entry("new", "raw-new", 30, 5)];
        let mine: Vec<&SaveEntry> = rows.iter().collect();
        let local = set(&["raw-mid", "raw-old", "raw-new", "raw-fresh"]);
        let out = sort_rows(&mine, &local, &set(&["raw-fresh"]), 0);
        assert_eq!(out.spare.len(), 3);
        let room = Room { device_rows: 3, device_cap: 3, account_rows: 3, account_cap: 400, bytes: 15, quota: 100 };
        let evict = make_room(&out.spare, room, 5).expect("room is made");
        assert_eq!(evict.iter().map(|s| s.row.as_str()).collect::<Vec<_>>(), vec!["old"]);
    }
}
