//! The sync: pull and push between this device's saves and the account, over the [`Api`] and a
//! [`Local`] store. Nothing is overwritten: a save is matched by its game, map, kind and the hash of
//! its packed bytes, and a save the device lacks is downloaded as its own file. Only one thing is
//! ever removed from the cloud: a stale save of a map's window (beyond the per-map limit, and not
//! from a newer Razdor), once a newer one needs its place. Local saves are never removed here.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use razdor_wire::cloud::{per_map_limit, BenchReport, HubList, Marks, SaveEntry, SaveKind, SaveMeta, SyncedSettings};
use razdor_wire::limits::CLOUD_BYTES_PER_ACCOUNT;
use razdor_wire::SCHEMA;

use super::api::{in_group, Api, CloudError};
use super::merge::{keeps_slot, merge_marks, merge_settings, newer_version, session_name};
use super::pack::{pack, sha256_hex, unpack};
use super::select::{choose, state, SaveState};

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
}

/// This device's saves and synced documents. The game's own store implements it (the UI phase):
/// the save folders, `marks.json`, the settings file and the Hub and benchmark records.
pub trait Local: Send {
    /// This device's name as the player gave it («ПК», «телефон»).
    fn device(&self) -> String;
    /// The device's zone offset from UTC in seconds, for the time shown in a session's name.
    fn offset_secs(&self) -> i64;
    /// Every manual, quick and session save on the device.
    fn saves(&self) -> Vec<LocalSave>;
    /// A save file's bytes, or None when it cannot be read.
    fn read(&self, id: &str) -> Option<Vec<u8>>;
    /// Stores a save file from the cloud as a local save; returns its id.
    fn store(&mut self, save: &LocalSave, bytes: &[u8]) -> Result<String, String>;
    /// Changes a local save's kind and name (a session slot becoming an ordinary save).
    fn relabel(&mut self, id: &str, kind: SaveKind, name: &str) -> Result<(), String>;
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

/// What one pull or push did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub pulled: usize,
    pub pushed: usize,
    pub deleted: usize,
    /// The round stopped because the account's quota is full.
    pub full: bool,
    /// The errors of single saves; the round went on past them.
    pub errors: Vec<String>,
    /// The account's saves from a newer Razdor: listed, never downloaded or deleted.
    pub newer: Vec<SaveEntry>,
    /// Each local save's state after the round, by the store's id (the load list's icons).
    pub states: HashMap<String, SaveState>,
    pub used_bytes: u64,
}

/// A local save with its packed bytes and their hash (the cloud's identity of a save file).
struct Held {
    save: LocalSave,
    packed: Vec<u8>,
    sha: String,
}

fn hold(local: &dyn Local) -> Vec<Held> {
    local
        .saves()
        .into_iter()
        .filter_map(|save| {
            let bytes = local.read(&save.id)?;
            let packed = pack(&bytes).ok()?;
            let sha = sha256_hex(&packed);
            Some(Held { save, packed, sha })
        })
        .collect()
}

fn same_file(held: &Held, entry: &SaveEntry) -> bool {
    held.save.game == entry.meta.game
        && held.save.map == entry.meta.map
        && held.save.kind == entry.meta.kind
        && held.sha == entry.meta.sha256
}

fn is_newer(entry: &SaveEntry) -> bool {
    newer_version(&entry.meta.razdor, RAZDOR)
}

/// The error of one save: a per-save problem is recorded and the round goes on; anything about
/// the connection or the account stops the round.
fn one_save(report: &mut Report, err: CloudError) -> Result<(), CloudError> {
    match err {
        CloudError::Bad(_) | CloudError::NotFound | CloudError::Refused(_) | CloudError::Conflict(_) => {
            report.errors.push(err.to_string());
            Ok(())
        }
        other => Err(other),
    }
}

/// Downloads a cloud session and keeps it here as an ordinary save named after its device and
/// time. False when it could not be kept (the problem is in the report); an offline error stops
/// the round.
fn keep_as_save(api: &Api, local: &mut dyn Local, theirs: &SaveEntry, offset: i64, report: &mut Report) -> Result<bool, CloudError> {
    let (meta, packed) = match api.get_save(&theirs.id) {
        Ok(got) => got,
        Err(e) => {
            one_save(report, e)?;
            return Ok(false);
        }
    };
    let bytes = match unpack(&packed) {
        Ok(bytes) => bytes,
        Err(e) => {
            report.errors.push(format!("save {}: {e}", theirs.id));
            return Ok(false);
        }
    };
    let save = LocalSave {
        id: String::new(),
        game: meta.game.clone(),
        map: meta.map.clone(),
        kind: SaveKind::Manual,
        name: session_name(&meta.device, meta.saved_at, offset),
        saved_at: meta.saved_at,
        pinned: false,
    };
    match local.store(&save, &bytes) {
        Ok(_) => Ok(true),
        Err(e) => {
            report.errors.push(e);
            Ok(false)
        }
    }
}

/// The unix time now (seconds).
pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Pulls: the account's saves this device lacks, the marks, the settings and the Hub offer.
pub fn pull(api: &Api, local: &mut dyn Local) -> Result<Report, CloudError> {
    let list = api.list_saves()?;
    let mut report = Report { used_bytes: list.used_bytes, ..Report::default() };
    let held = hold(local);
    let offset = local.offset_secs();
    for entry in &list.saves {
        if is_newer(entry) {
            report.newer.push(entry.clone());
            continue;
        }
        if held.iter().any(|h| same_file(h, entry)) {
            continue;
        }
        let (meta, packed) = match api.get_save(&entry.id) {
            Ok(got) => got,
            Err(e) => {
                one_save(&mut report, e)?;
                continue;
            }
        };
        let bytes = match unpack(&packed) {
            Ok(bytes) => bytes,
            Err(e) => {
                report.errors.push(format!("save {}: {e}", entry.id));
                continue;
            }
        };
        let mut incoming = LocalSave {
            id: String::new(),
            game: meta.game.clone(),
            map: meta.map.clone(),
            kind: meta.kind,
            name: meta.name.clone(),
            saved_at: meta.saved_at,
            pinned: meta.pinned,
        };
        if meta.kind == SaveKind::Session {
            // «Продолжить» is one slot: the newer of the two keeps it, the other becomes a save.
            if let Some(mine) = held.iter().find(|h| h.save.kind == SaveKind::Session && h.save.game == meta.game && h.save.map == meta.map) {
                if keeps_slot(mine.save.saved_at, &mine.save.id, meta.saved_at, &entry.id) {
                    incoming.kind = SaveKind::Manual;
                    incoming.name = session_name(&meta.device, meta.saved_at, offset);
                } else {
                    let name = session_name(&local.device(), mine.save.saved_at, offset);
                    if let Err(e) = local.relabel(&mine.save.id, SaveKind::Manual, &name) {
                        report.errors.push(e);
                        continue;
                    }
                }
            }
        }
        match local.store(&incoming, &bytes) {
            Ok(_) => report.pulled += 1,
            Err(e) => report.errors.push(e),
        }
    }
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

/// Pushes: this device's chosen saves (the per-map window), the marks, the settings, the Hub list
/// and the benchmark reports.
pub fn push(api: &Api, local: &mut dyn Local) -> Result<Report, CloudError> {
    let list = api.list_saves()?;
    let mut cloud = list.saves;
    let mut report = Report { used_bytes: list.used_bytes, ..Report::default() };
    let held = hold(local);
    let offset = local.offset_secs();
    let candidates: Vec<super::select::Candidate> = held
        .iter()
        .map(|h| super::select::Candidate {
            id: h.save.id.clone(),
            game: h.save.game.clone(),
            map: h.save.map.clone(),
            kind: h.save.kind,
            pinned: h.save.pinned,
            saved_at: h.save.saved_at,
        })
        .collect();
    let chosen: HashSet<String> = choose(&candidates).into_iter().collect();
    let chosen_shas: HashSet<String> = held.iter().filter(|h| chosen.contains(&h.save.id)).map(|h| h.sha.clone()).collect();
    let mut used = list.used_bytes;
    let mut full = false;

    for i in 0..held.len() {
        if !chosen.contains(&held[i].save.id) {
            continue;
        }
        let h = &held[i];
        if cloud.iter().any(|e| same_file(h, e)) {
            continue;
        }
        let (game, map, kind) = (h.save.game.clone(), h.save.map.clone(), h.save.kind);
        if kind == SaveKind::Session {
            let theirs = cloud
                .iter()
                .find(|e| e.meta.game == game && e.meta.map == map && e.meta.kind == SaveKind::Session)
                .cloned();
            if let Some(theirs) = theirs {
                if keeps_slot(h.save.saved_at, &h.save.id, theirs.meta.saved_at, &theirs.id) {
                    // This device's slot is the newer: the cloud's older one becomes a save of its own
                    // (the copy already held here is relabelled, not downloaded again).
                    let copy = held
                        .iter()
                        .find(|x| x.save.kind == SaveKind::Session && x.save.game == game && x.save.map == map && x.sha == theirs.meta.sha256)
                        .map(|x| x.save.id.clone());
                    let kept = match copy {
                        Some(id) => {
                            let name = session_name(&theirs.meta.device, theirs.meta.saved_at, offset);
                            match local.relabel(&id, SaveKind::Manual, &name) {
                                Ok(()) => true,
                                Err(e) => {
                                    report.errors.push(e);
                                    false
                                }
                            }
                        }
                        None => keep_as_save(api, local, &theirs, offset, &mut report)?,
                    };
                    if !kept {
                        continue;
                    }
                    match api.delete_save(&theirs.id) {
                        Ok(()) => {
                            cloud.retain(|e| e.id != theirs.id);
                            report.deleted += 1;
                        }
                        Err(e) => {
                            one_save(&mut report, e)?;
                            continue;
                        }
                    }
                } else {
                    // The cloud's slot is newer: this device's session becomes an ordinary save,
                    // and goes up on a later round as a manual one.
                    let name = session_name(&local.device(), h.save.saved_at, offset);
                    if let Err(e) = local.relabel(&h.save.id, SaveKind::Manual, &name) {
                        report.errors.push(e);
                    }
                    continue;
                }
            }
        }
        // Make room in the map's window: remove stale saves (not chosen here, not from a newer
        // Razdor), oldest first, and only as many as the new one needs.
        let limit = per_map_limit(kind);
        if in_group(&cloud, &game, &map, kind).len() >= limit {
            let mut stale: Vec<(u64, String)> = in_group(&cloud, &game, &map, kind)
                .into_iter()
                .filter(|e| !is_newer(e) && !chosen_shas.contains(&e.meta.sha256))
                .map(|e| (e.meta.saved_at, e.id.clone()))
                .collect();
            stale.sort();
            for (_, id) in stale {
                if in_group(&cloud, &game, &map, kind).len() < limit {
                    break;
                }
                match api.delete_save(&id) {
                    Ok(()) => {
                        cloud.retain(|e| e.id != id);
                        report.deleted += 1;
                    }
                    Err(e) => {
                        one_save(&mut report, e)?;
                        break;
                    }
                }
            }
            if in_group(&cloud, &game, &map, kind).len() >= limit {
                report.errors.push(format!("map {map}: the cloud's {limit} saves of this kind are not stale"));
                continue;
            }
        }
        let h = &held[i];
        let size = h.packed.len() as u64;
        if used + size > CLOUD_BYTES_PER_ACCOUNT {
            report.full = true;
            full = true;
            break;
        }
        let meta = SaveMeta {
            schema: SCHEMA,
            game: h.save.game.clone(),
            map: h.save.map.clone(),
            kind: h.save.kind,
            device: local.device(),
            name: h.save.name.clone(),
            saved_at: h.save.saved_at,
            razdor: RAZDOR.to_string(),
            size,
            sha256: h.sha.clone(),
            pinned: h.save.pinned,
        };
        match api.put_save(&meta, &h.packed) {
            Ok(put) => {
                used = put.used_bytes;
                cloud.push(SaveEntry { id: put.id, meta });
                report.pushed += 1;
            }
            Err(CloudError::Full) => {
                report.full = true;
                full = true;
                break;
            }
            Err(e) => one_save(&mut report, e)?,
        }
    }
    report.used_bytes = used;

    if !full {
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
        .map(|h| {
            let in_cloud = cloud.iter().any(|e| same_file(h, e));
            (h.save.id.clone(), state(in_cloud, chosen.contains(&h.save.id)))
        })
        .collect();
    report.newer = cloud.iter().filter(|e| is_newer(e)).cloned().collect();
    Ok(report)
}

/// A full round: pull, then push. The pull's report carries the downloads; the push's the rest.
pub fn sync(api: &Api, local: &mut dyn Local) -> Result<Report, CloudError> {
    let pulled = pull(api, local)?;
    let mut report = push(api, local)?;
    report.pulled = pulled.pulled;
    report.errors.splice(0..0, pulled.errors);
    report.newer = pulled.newer;
    Ok(report)
}
