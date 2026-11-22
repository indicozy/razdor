//! A device for the tests: its saves and documents in memory (the game's own store is the UI
//! phase's [`Local`]).

use std::collections::BTreeMap;

use razdor_wire::cloud::{MarkLevel, MarkRecord, Marks, SaveKind, SettingValue, SyncedSettings};
use razdor_wire::cloud::{BenchReport, HubList};
use razdor_wire::SCHEMA;

use super::sync::{Local, LocalSave};

pub struct MemLocal {
    pub device: String,
    pub offset: i64,
    saves: Vec<(LocalSave, Vec<u8>)>,
    next: u64,
    marks: Marks,
    settings: SyncedSettings,
    hub: HubList,
    pub hub_offer: Option<HubList>,
    pub bench: Vec<BenchReport>,
}

impl MemLocal {
    pub fn new(device: &str) -> MemLocal {
        MemLocal {
            device: device.to_string(),
            offset: 0,
            saves: Vec::new(),
            next: 0,
            marks: Marks { schema: SCHEMA, updated_at: 0, maps: BTreeMap::new() },
            settings: SyncedSettings { schema: SCHEMA, values: BTreeMap::new() },
            hub: HubList { schema: SCHEMA, updated_at: 0, items: Vec::new() },
            hub_offer: None,
            bench: Vec::new(),
        }
    }

    /// Adds a save; returns its id.
    pub fn add(&mut self, game: &str, map: &str, kind: SaveKind, name: &str, saved_at: u64, bytes: &[u8]) -> String {
        self.next += 1;
        let id = format!("local-{}", self.next);
        let save = LocalSave {
            id: id.clone(),
            game: game.into(),
            map: map.into(),
            kind,
            name: name.into(),
            saved_at,
            pinned: false,
            device: String::new(),
        };
        self.saves.push((save, bytes.to_vec()));
        id
    }

    /// The local save called `name`.
    pub fn named(&self, name: &str) -> Option<&LocalSave> {
        self.saves.iter().map(|(s, _)| s).find(|s| s.name == name)
    }

    pub fn bytes_named(&self, name: &str) -> Option<Vec<u8>> {
        self.saves.iter().find(|(s, _)| s.name == name).map(|(_, b)| b.clone())
    }

    /// A setting as this device holds it, with the time of its change.
    pub fn set_setting(&mut self, key: &str, value: &str, changed_at: u64) {
        self.settings
            .values
            .insert(key.into(), SettingValue { value: value.into(), changed_at });
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        self.settings.values.get(key).map(|v| v.value.clone())
    }

    pub fn set_mark(&mut self, map: &str, level: MarkLevel) {
        self.marks.maps.insert(map.into(), MarkRecord { level, date: "2026-11-22".into(), mark: serde_json::json!({}) });
    }

    pub fn has_mark(&self, map: &str) -> bool {
        self.marks.maps.contains_key(map)
    }
}

impl Local for MemLocal {
    fn device(&self) -> String {
        self.device.clone()
    }

    fn offset_secs(&self) -> i64 {
        self.offset
    }

    fn saves(&self) -> Vec<LocalSave> {
        self.saves.iter().map(|(s, _)| s.clone()).collect()
    }

    fn read(&self, id: &str) -> Option<Vec<u8>> {
        self.saves.iter().find(|(s, _)| s.id == id).map(|(_, b)| b.clone())
    }

    fn store(&mut self, save: &LocalSave, bytes: &[u8]) -> Result<String, String> {
        let mut save = save.clone();
        self.next += 1;
        save.id = format!("local-{}", self.next);
        let id = save.id.clone();
        self.saves.push((save, bytes.to_vec()));
        Ok(id)
    }

    fn relabel(&mut self, id: &str, kind: SaveKind, name: &str) -> Result<(), String> {
        let (save, _) = self.saves.iter_mut().find(|(s, _)| s.id == id).ok_or("no such save")?;
        save.kind = kind;
        save.name = name.into();
        Ok(())
    }

    fn marks(&self) -> Marks {
        self.marks.clone()
    }

    fn set_marks(&mut self, marks: Marks) {
        self.marks = marks;
    }

    fn settings(&self) -> SyncedSettings {
        self.settings.clone()
    }

    fn set_settings(&mut self, settings: SyncedSettings) {
        self.settings = settings;
    }

    fn hub(&self) -> HubList {
        self.hub.clone()
    }

    fn set_hub_offer(&mut self, list: HubList) {
        self.hub_offer = Some(list);
    }

    fn bench_pending(&self) -> Vec<BenchReport> {
        self.bench.clone()
    }

    fn bench_sent(&mut self, at: u64) {
        self.bench.retain(|r| r.at != at);
    }
}
