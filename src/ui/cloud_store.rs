//! Cloud saves, the game's side (`razdor::cloud`; `docs/superpowers/specs/2026-11-22-cloud-saves-design.md`).
//!
//! [`Store`] is the `cloud::sync::Local` over this device: the saves of the install played
//! (`save::games_dir`), `marks.json`, the synced settings (a few keys of `audio.json`), the
//! installed Hub items. A pulled save becomes its own file beside the local ones (`cloud-<hash>.rzsave`)
//! and keeps its origin in a `.cloud` sidecar (device and the title a session got); nothing is
//! overwritten. The worker is started only once an account exists (or when the player turns the
//! cloud on); with no account nothing is sent. Nothing here runs in the snapshots or tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use razdor::cloud::account;
use razdor::cloud::sync::{Local, LocalSave};
use razdor::cloud::{Cloud, Job};
use razdor::rules::marks::{Level, Record, RunMark, Records};
use razdor::rules::save::{self, ScenarioRef, SaveMeta};
use razdor_wire::cloud::{
    BenchReport, HubItem, HubKind, HubList, MarkLevel, MarkRecord, Marks, SaveEntry, SaveKind, SettingValue, SyncedSettings,
};
use razdor_wire::SCHEMA;
use serde::{Deserialize, Serialize};

use super::audio::Settings;
use super::language::Settings as Chosen;

/// The device's name as the player sees it in the load list.
pub fn device_name() -> &'static str {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        "телефон"
    } else {
        "ПК"
    }
}

/// The cloud's folder: the account key, the synced settings' record, the off switch.
pub fn cloud_dir() -> Option<PathBuf> {
    razdor::platform::app_dir().map(|d| d.join("cloud"))
}

/// Whether the cloud runs in this process at all: not under `cargo test`, not in the off-screen
/// scenes or snapshots, and only when the remote config's `cloud` switch is on. Off, no worker starts
/// and nothing is sent, even with an account key on this device (the key stays).
pub fn active() -> bool {
    !cfg!(test) && std::env::var_os("RAZDOR_SNAPSHOT").is_none() && std::env::var_os("RAZDOR_SCENE").is_none() && razdor::remote_config::cloud_allowed()
}

/// The worker, once started (see [`request`]).
static CLOUD: OnceLock<Cloud> = OnceLock::new();
/// Requests made before the worker exists; they run once it starts. Guarded with [`CLOUD`]'s start.
static QUEUE: Mutex<Vec<Job>> = Mutex::new(Vec::new());
static STARTING: AtomicBool = AtomicBool::new(false);
/// The off switch's file in the cloud folder: while it is there, nothing is sent (the key stays).
const OFF_FILE: &str = "off";

/// The account key is here and the cloud is not switched off.
pub fn on() -> bool {
    let Some(dir) = cloud_dir() else { return false };
    account::load_key(&dir).is_some() && !dir.join(OFF_FILE).exists()
}

/// The account exists on this device (switched on or off).
pub fn has_account() -> bool {
    cloud_dir().is_some_and(|d| account::load_key(&d).is_some())
}

/// The worker's state for the settings rows (a copy); `None` until it starts.
pub fn shared() -> Option<razdor::cloud::Shared> {
    CLOUD.get().map(|c| c.snapshot())
}

/// The account's saves made by a newer Razdor: listed in the load list, never loaded or deleted
/// there. The snapshot's `saves` scene shows one made-up save.
pub fn newer_saves() -> Vec<SaveEntry> {
    if std::env::var_os("RAZDOR_SNAPSHOT").is_some() {
        return if std::env::var("RAZDOR_SCENE_CLOUD").is_ok_and(|s| s == "saves") {
            vec![SaveEntry {
                id: "newer-save".to_string(),
                meta: razdor_wire::cloud::SaveMeta {
                    schema: SCHEMA,
                    game: "demo".to_string(),
                    map: "demo".to_string(),
                    kind: SaveKind::Manual,
                    device: device_name().to_string(),
                    name: "Newer save".to_string(),
                    saved_at: 1_700_000_000,
                    razdor: String::new(),
                    size: 0,
                    sha256: String::new(),
                    pinned: false,
                },
            }]
        } else {
            Vec::new()
        };
    }
    shared().map(|s| s.newer).unwrap_or_default()
}

/// The load list's state of a local save (`LocalOnly` without a worker).
pub fn state_of(id: &str) -> razdor::cloud::SaveState {
    // The snapshots' made-up marks (`RAZDOR_SCENE_CLOUD=saves`): by the file's name.
    if std::env::var_os("RAZDOR_SNAPSHOT").is_some() && std::env::var("RAZDOR_SCENE_CLOUD").is_ok_and(|s| s == "saves") {
        let name = Path::new(id).file_name().and_then(|n| n.to_str()).unwrap_or("");
        return if name.starts_with("cloud") {
            razdor::cloud::SaveState::InCloud
        } else if name.starts_with("wait") {
            razdor::cloud::SaveState::WaitingUpload
        } else {
            razdor::cloud::SaveState::LocalOnly
        };
    }
    CLOUD.get().map_or(razdor::cloud::SaveState::LocalOnly, |c| c.state_of(id))
}

/// Queues a request. Nothing is sent without an account: only `CreateAccount` starts the worker
/// then. A switched-off cloud takes no sync or push. Never waits.
pub fn request(job: Job) {
    if !active() {
        return;
    }
    if !matches!(job, Job::CreateAccount | Job::Link | Job::Redeem(_)) && !on() {
        return;
    }
    let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = CLOUD.get() {
        c.request(job);
        return;
    }
    queue.push(job);
    drop(queue);
    start_worker();
}

/// The push on leaving the app (the exit, minimising, the phone's pause): queued, never waited for
/// except by [`finish_on_exit`].
pub fn push_when_left() {
    request(Job::Push);
}

/// The exit: pushes what is chosen, and waits up to `FINISH_WAIT` for the requests to run. Offline
/// or slow, the game goes on: what is not pushed goes up at the next start.
pub fn finish_on_exit() {
    if !active() || !on() {
        return;
    }
    request(Job::Push);
    if let Some(c) = CLOUD.get() {
        let _ = c.finish(FINISH_WAIT);
    }
}

/// How long the exit waits for the push.
const FINISH_WAIT: std::time::Duration = std::time::Duration::from_secs(4);

/// The start-up request: a pull, when the account exists and the cloud is on. Called once the
/// install is loaded (the save folders are known then).
pub fn start_if_on() {
    if active() && on() {
        request(Job::Sync);
    }
}

/// Starts the worker thread once. The remote config's API list is read on that thread, never on
/// the game's.
fn start_worker() {
    if !active() || STARTING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("razdor-cloud-start".into()).spawn(|| {
        let Some(dir) = cloud_dir() else { return };
        let Some(base) = razdor::remote_config::api().into_iter().next() else { return };
        let cloud = Cloud::start(Box::new(Store), razdor::cloud::http::platform_net(30), &base, dir);
        let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        let _ = CLOUD.set(cloud);
        if let Some(c) = CLOUD.get() {
            for job in queue.drain(..) {
                c.request(job);
            }
        }
    });
    if let Err(e) = spawned {
        razdor::diag!("cloud: the worker does not start: {e}");
    }
}

/// Switches the cloud on: the account is made when there is none, else the switch file goes.
pub fn turn_on() {
    let Some(dir) = cloud_dir() else { return };
    let _ = std::fs::remove_file(dir.join(OFF_FILE));
    if has_account() {
        request(Job::Sync);
    } else {
        request(Job::CreateAccount);
    }
}

/// Switches the cloud off: nothing is sent until it is on again. The key stays on this device.
pub fn turn_off() {
    let Some(dir) = cloud_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(OFF_FILE), b"off\n");
}

/// Name of the game the saves of this install belong to: the active mod's id, else `dt`.
pub fn game_now() -> String {
    razdor::hub::local_dir().and_then(|l| razdor::hub::active_mod(&l)).unwrap_or_else(|| "dt".to_string())
}

/// The sidecar of a save file: its origin device and the title a session got (`.cloud` beside it).
#[derive(Default, Serialize, Deserialize)]
struct Side {
    #[serde(default)]
    device: String,
    #[serde(default)]
    title: String,
}

fn side_path(save: &Path) -> PathBuf {
    let mut s = save.as_os_str().to_owned();
    s.push(".cloud");
    PathBuf::from(s)
}

fn side_of(save: &Path) -> Side {
    std::fs::read(side_path(save)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_side(save: &Path, side: &Side) -> Result<(), String> {
    let text = serde_json::to_vec(side).map_err(|e| e.to_string())?;
    std::fs::write(side_path(save), text).map_err(|e| e.to_string())
}

/// The device that made a cloud copy saved here (none for a save made on this device).
pub fn origin_of(path: &Path) -> Option<String> {
    Some(side_of(path).device).filter(|d| !d.is_empty())
}

/// The map key of a save: its map's slot key, or `demo`.
fn map_of(meta: &SaveMeta) -> String {
    match &meta.scenario {
        ScenarioRef::Map { file, .. } => save::map_slot_key_of_file(file),
        ScenarioRef::Demo => "demo".to_string(),
    }
}

/// The save file must be one of this install's save files (a path from the sync, never outside).
fn own_save(id: &str) -> Option<PathBuf> {
    let p = PathBuf::from(id);
    let dir = save::games_dir()?;
    let inside = p.starts_with(&dir) && p.extension().is_some_and(|x| x == save::EXTENSION);
    inside.then_some(p)
}

/// A new file in the manual folder for `bytes` (named by their hash; a free name if that one is taken).
fn new_save_file(bytes: &[u8]) -> Result<PathBuf, String> {
    let folder = save::manual_dir().ok_or("no save folder")?;
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let hash = save::fnv1a(bytes);
    (0..)
        .map(|k| folder.join(if k == 0 { format!("cloud-{hash:016x}.{}", save::EXTENSION) } else { format!("cloud-{hash:016x}-{k}.{}", save::EXTENSION) }))
        .find(|p| !p.exists())
        .ok_or_else(|| "no free name".to_string())
}

/// The cloud's view of this device: the save folders, the marks, the settings and the Hub.
pub struct Store;

impl Local for Store {
    fn device(&self) -> String {
        device_name().to_string()
    }

    fn offset_secs(&self) -> i64 {
        super::saves::local_offset(razdor::telemetry::now_secs())
    }

    fn saves(&self) -> Vec<LocalSave> {
        let Some(dir) = save::games_dir() else { return Vec::new() };
        let game = game_now();
        let mut out: Vec<LocalSave> = save::list(&dir, save::SaveKind::Manual)
            .into_iter()
            .map(|e| {
                let kind = if e.meta.name == save::QUICK_SAVE { SaveKind::Quick } else { SaveKind::Manual };
                entry(&e.path, &e.meta, kind, e.pinned, &game)
            })
            .collect();
        let session = save::session_path(&dir);
        if session.is_file() {
            if let Ok(meta) = save::read_meta(&session) {
                out.push(entry(&session, &meta, SaveKind::Session, false, &game));
            }
        }
        out
    }

    fn read(&self, id: &str) -> Option<Vec<u8>> {
        std::fs::read(own_save(id)?).ok()
    }

    fn store(&mut self, save: &LocalSave, bytes: &[u8]) -> Result<String, String> {
        // A session from the cloud lands as an ordinary save: the local session slot is not
        // overwritten (it is one slot for the install, not one per map).
        let path = new_save_file(bytes)?;
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        write_side(&path, &Side { device: save.device.clone(), title: save.name.clone() })?;
        Ok(path.to_string_lossy().into_owned())
    }

    fn relabel(&mut self, id: &str, kind: SaveKind, name: &str) -> Result<(), String> {
        let path = own_save(id).ok_or("no such save")?;
        let mut side = side_of(&path);
        side.title = name.to_string();
        let session = save::games_dir().map(|d| save::session_path(&d));
        if kind == SaveKind::Session || session.as_deref() != Some(path.as_path()) {
            return write_side(&path, &side);
        }
        // The session slot becomes an ordinary save: the file moves to the manual folder.
        let target = new_save_file(&std::fs::read(&path).map_err(|e| e.to_string())?)?;
        std::fs::rename(&path, &target).map_err(|e| e.to_string())?;
        write_side(&target, &side)
    }

    fn marks(&self) -> Marks {
        let records = Records::load();
        let updated_at = Records::path().and_then(|p| mtime(&p)).unwrap_or(0);
        Marks {
            schema: SCHEMA,
            updated_at,
            maps: records
                .entries()
                .map(|(key, rec)| {
                    let mark = serde_json::to_value(&rec.mark).unwrap_or(serde_json::Value::Null);
                    (key.clone(), MarkRecord { level: mark_level(rec.mark.level), date: rec.date.clone(), mark })
                })
                .collect(),
        }
    }

    fn set_marks(&mut self, marks: Marks) {
        let mut records = Records::load();
        for (key, rec) in marks.maps {
            if let Ok(mark) = serde_json::from_value::<RunMark>(rec.mark) {
                records.put_key(key, Record { mark, date: rec.date });
            }
        }
        records.save();
    }

    fn settings(&self) -> SyncedSettings {
        let now = razdor::telemetry::now_secs();
        let mut seen = SyncState::load();
        let mut values = BTreeMap::new();
        for (key, value) in synced_values(&Settings::load(), &Chosen::load()) {
            let changed_at = match seen.values.get(&key) {
                Some(s) if s.value == value => s.changed_at,
                _ => now,
            };
            seen.values.insert(key.clone(), Seen { value: value.clone(), changed_at });
            values.insert(key, SettingValue { value, changed_at });
        }
        seen.save();
        SyncedSettings { schema: SCHEMA, values }
    }

    fn set_settings(&mut self, settings: SyncedSettings) {
        let mut s = Settings::load();
        let mut chosen = Chosen::load();
        let mut seen = SyncState::load();
        let mut pulled = Vec::new();
        for (key, v) in settings.values {
            apply_synced(&mut s, &mut chosen, &key, &v.value);
            pulled.push((key.clone(), v.value.clone()));
            seen.values.insert(key, Seen { value: v.value, changed_at: v.changed_at });
        }
        s.save();
        chosen.save();
        seen.save();
        // The running game takes the same values (`ui::audio`), or its next save writes the old ones back.
        if let Ok(mut q) = PULLED.lock() {
            q.extend(pulled);
        }
    }

    fn hub(&self) -> HubList {
        let Some(local) = razdor::hub::local_dir() else { return empty_hub() };
        let installed = razdor::hub::read_installed(&local);
        let updated_at = mtime(&local.join("installed.json")).unwrap_or(0);
        let items = installed
            .entries
            .iter()
            .map(|(id, e)| HubItem {
                id: id.clone(),
                kind: match e.kind {
                    razdor::hub::Kind::Mod => HubKind::Mod,
                    _ => HubKind::Map,
                },
                version: e.version.clone(),
            })
            .collect();
        HubList { schema: SCHEMA, updated_at, items }
    }

    fn set_hub_offer(&mut self, list: HubList) {
        if let Ok(mut offer) = HUB_OFFER.lock() {
            *offer = Some(list);
        }
    }

    fn bench_pending(&self) -> Vec<BenchReport> {
        let Some(dir) = bench_dir() else { return Vec::new() };
        let mut out: Vec<BenchReport> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| std::fs::read(e.path()).ok())
            .filter_map(|b| serde_json::from_slice(&b).ok())
            .collect();
        out.sort_by_key(|r: &BenchReport| r.at);
        out
    }

    fn bench_sent(&mut self, at: u64) {
        if let Some(dir) = bench_dir() {
            let _ = std::fs::remove_file(dir.join(format!("{at}.json")));
        }
    }
}

/// The folder of the benchmark reports not sent yet (`cloud/bench`, one JSON file each, named by its time).
fn bench_dir() -> Option<PathBuf> {
    cloud_dir().map(|d| d.join("bench"))
}

/// Keeps a finished benchmark report until the cloud sends it (`Store::bench_pending`). The newest
/// `BENCH_KEEP` are kept; a report made in the same second takes the next free time.
pub fn bench_finished(report: &BenchReport) {
    let Some(dir) = bench_dir() else { return };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let mut at = report.at;
    while dir.join(format!("{at}.json")).exists() {
        at += 1;
    }
    let mut report = report.clone();
    report.at = at;
    if let Ok(text) = serde_json::to_vec(&report) {
        let _ = std::fs::write(dir.join(format!("{at}.json")), text);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    files.sort();
    let extra = files.len().saturating_sub(BENCH_KEEP);
    for old in files.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
}

/// How many benchmark reports wait for the cloud at most.
const BENCH_KEEP: usize = 20;

/// The account's installed Hub list, as the worker last got it: `cloud_view` offers its missing items
/// («Установить то же самое?») once per set.
pub static HUB_OFFER: Mutex<Option<HubList>> = Mutex::new(None);

fn empty_hub() -> HubList {
    HubList { schema: SCHEMA, updated_at: 0, items: Vec::new() }
}

fn entry(path: &Path, meta: &SaveMeta, kind: SaveKind, pinned: bool, game: &str) -> LocalSave {
    let side = side_of(path);
    let name = if meta.name.is_empty() { side.title } else { meta.name.clone() };
    LocalSave {
        id: path.to_string_lossy().into_owned(),
        game: game.to_string(),
        map: map_of(meta),
        kind,
        name,
        saved_at: meta.saved_at,
        pinned,
        device: side.device,
    }
}

fn mtime(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

fn mark_level(level: Level) -> MarkLevel {
    match level {
        Level::Green => MarkLevel::Green,
        Level::Fixes => MarkLevel::Fixes,
        Level::Grey => MarkLevel::Grey,
        Level::Yellow => MarkLevel::Yellow,
        Level::Red => MarkLevel::Red,
    }
}

/// The synced settings of this device, by key: device-independent values only (language, the
/// speeds, hints, hotkeys, the AI strength). Graphics, screen and sound are never here.
pub fn synced_values(s: &Settings, chosen: &Chosen) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    out.insert("language".into(), js(&chosen.language));
    out.insert("anim_speed".into(), js(&s.anim_speed));
    out.insert("walk_speed".into(), js(&s.walk_speed));
    out.insert("scroll_speed".into(), js(&s.scroll_speed));
    out.insert("hints_off".into(), js(&s.hints_off));
    out.insert("keys".into(), js(&s.keys));
    out.insert("ai_strength".into(), js(&(s.expert_ai, s.max_ai)));
    out
}

/// Settings pulled from the cloud and not yet applied to the running game (`(key, JSON value)`),
/// taken on the main thread by [`take_pulled`].
static PULLED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// The synced settings a pull wrote since the last call (the main thread applies them to the
/// running game, see `ui::audio::Audio::take_cloud_pulled`).
pub fn take_pulled() -> Vec<(String, String)> {
    PULLED.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

/// Writes one synced value into the settings (a value that does not read is ignored).
pub fn apply_synced(s: &mut Settings, chosen: &mut Chosen, key: &str, value: &str) {
    if key == "language" {
        set_json(value, &mut chosen.language);
    } else {
        apply_synced_audio(s, key, value);
    }
}

/// The synced values kept in the audio settings (everything but the language).
pub fn apply_synced_audio(s: &mut Settings, key: &str, value: &str) {
    match key {
        "anim_speed" => set_json(value, &mut s.anim_speed),
        "walk_speed" => set_json(value, &mut s.walk_speed),
        "scroll_speed" => set_json(value, &mut s.scroll_speed),
        "hints_off" => set_json(value, &mut s.hints_off),
        "keys" => set_json(value, &mut s.keys),
        "ai_strength" => {
            if let Ok((expert, max)) = serde_json::from_str::<(Option<bool>, bool)>(value) {
                s.expert_ai = expert;
                s.max_ai = max;
            }
        }
        _ => {}
    }
}

fn set_json<T: serde::de::DeserializeOwned>(text: &str, field: &mut T) {
    if let Ok(v) = serde_json::from_str(text) {
        *field = v;
    }
}

/// The per-key record of what this device last saw: the value and when it first had it.
#[derive(Default, Serialize, Deserialize)]
struct SyncState {
    #[serde(default)]
    values: BTreeMap<String, Seen>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Seen {
    value: String,
    changed_at: u64,
}

impl SyncState {
    fn load() -> SyncState {
        cloud_dir().and_then(|d| std::fs::read(d.join("synced.json")).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save(&self) {
        let Some(dir) = cloud_dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(text) = serde_json::to_vec(self) {
            let _ = std::fs::write(dir.join("synced.json"), text);
        }
    }
}

/// A setting value as the wire carries it: its JSON text.
fn js<T: Serialize + ?Sized>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_device_independent_settings_are_synced() {
        let values = synced_values(&Settings::default(), &Chosen::default());
        let keys: Vec<&str> = values.keys().map(String::as_str).collect();
        assert_eq!(keys, ["ai_strength", "anim_speed", "hints_off", "keys", "language", "scroll_speed", "walk_speed"]);
    }

    #[test]
    fn a_synced_value_is_applied_and_a_broken_one_ignored() {
        let (mut s, mut c) = (Settings::default(), Chosen::default());
        apply_synced(&mut s, &mut c, "anim_speed", "0.5");
        assert_eq!(s.anim_speed, Some(0.5));
        apply_synced(&mut s, &mut c, "anim_speed", "not json");
        assert_eq!(s.anim_speed, Some(0.5));
        apply_synced(&mut s, &mut c, "ai_strength", "[true,false]");
        assert_eq!((s.expert_ai, s.max_ai), (Some(true), false));
        apply_synced(&mut s, &mut c, "graphics", "0");
        assert_eq!(synced_values(&s, &c)["ai_strength"], "[true,false]");
    }

    #[test]
    fn pulled_settings_apply_to_the_running_settings_and_come_back_the_same() {
        let (mut src, chosen) = (Settings::default(), Chosen::default());
        apply_synced_audio(&mut src, "anim_speed", "0.5");
        apply_synced_audio(&mut src, "ai_strength", "[true,false]");
        let mut dst = Settings::default();
        for (key, value) in synced_values(&src, &chosen) {
            if key != "language" {
                apply_synced_audio(&mut dst, &key, &value);
            }
        }
        assert_eq!((dst.anim_speed, dst.expert_ai, dst.max_ai), (Some(0.5), Some(true), false));
        assert_eq!(synced_values(&dst, &chosen), synced_values(&src, &chosen));
        apply_synced_audio(&mut dst, "no_such_key", "1");
        assert_eq!(synced_values(&dst, &chosen), synced_values(&src, &chosen), "an unknown key changes nothing");
    }

    #[test]
    fn the_pulled_queue_is_taken_once_in_order() {
        PULLED.lock().unwrap().extend([("anim_speed".to_string(), "0.5".to_string()), ("hints_off".to_string(), "true".to_string())]);
        let taken = take_pulled();
        assert_eq!(taken.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["anim_speed", "hints_off"]);
        assert!(take_pulled().is_empty(), "a second take sees nothing");
    }

    #[test]
    fn a_cloud_copy_keeps_its_device_and_a_local_save_has_none() {
        let dir = std::env::temp_dir().join(format!("razdor-cloud-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let save = dir.join("a.rzsave");
        assert_eq!(origin_of(&save), None);
        write_side(&save, &Side { device: "телефон".into(), title: String::new() }).unwrap();
        assert_eq!(origin_of(&save).as_deref(), Some("телефон"));
        assert_eq!(side_path(&save), dir.join("a.rzsave.cloud"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_device_names_are_the_players_words() {
        assert!(matches!(device_name(), "ПК" | "телефон"));
    }
}
