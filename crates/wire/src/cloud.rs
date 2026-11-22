//! The cloud bodies (`2026-11-22-cloud-saves-design.md`): the account, the link between devices,
//! the saves, the completed-game marks, the synced settings, the installed Hub items and the
//! performance test's reports. Keyed requests carry the account key in [`crate::paths::KEY_HEADER`].
//!
//! A save put is framed ([`crate::frame`]): the header is [`SaveMeta`], the body the save file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::analytics::DeviceInfo;
use crate::check;
use crate::ids::{AccountKey, LinkCode};
use crate::limits;
use crate::{Validate, WireError, WireResult};

/// Creates an account; the answer carries its key (the only secret; no login, no e-mail).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountNew {
    pub schema: u32,
}

impl Validate for AccountNew {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountCreated {
    pub key: AccountKey,
}

impl Validate for AccountCreated {
    fn validate(&self) -> WireResult {
        self.key.validate()
    }
}

/// The answer to `POST LINK` (keyed): a code another device types within the time shown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkIssued {
    pub code: LinkCode,
    /// When the code stops working (seconds since 1970).
    pub expires_at: u64,
}

impl Validate for LinkIssued {
    fn validate(&self) -> WireResult {
        self.code.validate()?;
        check::number("expires_at", self.expires_at, 4_102_444_800)
    }
}

/// A second device types the code: `POST LINK_REDEEM`. `code` is what was typed (any case, spaces).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkRedeem {
    pub schema: u32,
    pub code: String,
}

impl Validate for LinkRedeem {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        LinkCode::parse(&self.code).map(|_| ())
    }
}

/// The key the redeeming device receives: the same key as the issuing device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkRedeemed {
    pub key: AccountKey,
}

impl Validate for LinkRedeemed {
    fn validate(&self) -> WireResult {
        self.key.validate()
    }
}

/// The three kinds of cloud save. Limits per map: [`per_map_limit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveKind {
    Manual,
    Quick,
    /// The «Продолжить» slot: one per map.
    Session,
}

/// How many saves of `kind` one account keeps per map.
pub fn per_map_limit(kind: SaveKind) -> usize {
    match kind {
        SaveKind::Manual => limits::MANUAL_SAVES_PER_MAP,
        SaveKind::Quick => limits::QUICK_SAVES_PER_MAP,
        SaveKind::Session => limits::SESSION_SAVES_PER_MAP,
    }
}

/// A save's metadata: the header of a put, and what a list shows. Its key is the game or mod, its
/// map key the map's file name (as saves are keyed locally).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveMeta {
    pub schema: u32,
    pub game: String,
    pub map: String,
    pub kind: SaveKind,
    /// The device's name as the player gave it (or the OS's): «ПК», «телефон».
    pub device: String,
    /// The save's display name (the player's own text; empty for a session slot).
    pub name: String,
    /// Real time of saving (seconds since 1970).
    pub saved_at: u64,
    pub razdor: String,
    /// The file's length in bytes, after zstd on the client; must equal the body's.
    pub size: u64,
    pub sha256: String,
    pub pinned: bool,
}

impl Validate for SaveMeta {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        check::key("game", &self.game, limits::MAX_KEY)?;
        check::key("map", &self.map, limits::MAX_KEY)?;
        check::text("device", &self.device, limits::MAX_TEXT)?;
        check::opt_text("name", &self.name, limits::MAX_SAVE_NAME)?;
        check::number("saved_at", self.saved_at, 4_102_444_800)?;
        check::text("razdor", &self.razdor, limits::MAX_VERSION)?;
        check::number("size", self.size, limits::MAX_SAVE_BODY as u64)?;
        check::sha256("sha256", &self.sha256)
    }
}

/// One save as the server lists it: its id (server-issued) and its metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveEntry {
    pub id: String,
    pub meta: SaveMeta,
}

impl Validate for SaveEntry {
    fn validate(&self) -> WireResult {
        check::key("id", &self.id, limits::MAX_KEY)?;
        self.meta.validate()
    }
}

/// The answer to `GET SAVES`: every save of the account, and the quota's use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveList {
    pub saves: Vec<SaveEntry>,
    pub used_bytes: u64,
    pub quota_bytes: u64,
}

impl Validate for SaveList {
    fn validate(&self) -> WireResult {
        check::count("saves", &self.saves, limits::MAX_SAVES_LISTED)?;
        self.saves.iter().try_for_each(Validate::validate)
    }
}

/// The answer to a put: the save's id. A put over the per-map limit or the quota is refused by the
/// server with the cloud's error, never by deleting another save.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavePut {
    pub id: String,
    pub used_bytes: u64,
}

impl Validate for SavePut {
    fn validate(&self) -> WireResult {
        check::key("id", &self.id, limits::MAX_KEY)
    }
}

/// A map's completed-game level (`marks.rs`'s level, snake case on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkLevel {
    Green,
    Grey,
    Yellow,
    Red,
}

/// One map's record: its level, the date, and the game's own mark JSON (`RunMark`), opaque here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarkRecord {
    pub level: MarkLevel,
    pub date: String,
    pub mark: serde_json::Value,
}

impl Validate for MarkRecord {
    fn validate(&self) -> WireResult {
        check::opt_text("date", &self.date, 16)?;
        let len = serde_json::to_vec(&self.mark).map_err(|e| WireError::Json(e.to_string()))?.len();
        if len > limits::MAX_MARK_VALUE_BYTES {
            return Err(WireError::TooLong { field: "mark", max: limits::MAX_MARK_VALUE_BYTES, len });
        }
        Ok(())
    }
}

/// `marks.json` in the cloud: the best record per map. Merged, never overwritten: a map completed
/// on any device stays completed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marks {
    pub schema: u32,
    /// When the document was last changed (seconds since 1970).
    pub updated_at: u64,
    pub maps: BTreeMap<String, MarkRecord>,
}

impl Validate for Marks {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        check::number("updated_at", self.updated_at, 4_102_444_800)?;
        check::count("maps", &self.maps.keys().collect::<Vec<_>>(), limits::MAX_MARK_MAPS)?;
        for (name, rec) in &self.maps {
            check::key("map", name, limits::MAX_KEY)?;
            rec.validate()?;
        }
        Ok(())
    }
}

/// One synced setting: its value and when it was last changed (the newest change wins).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingValue {
    pub value: String,
    pub changed_at: u64,
}

/// The settings that do not depend on the device (language, speed, hints, hotkeys, …).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedSettings {
    pub schema: u32,
    pub values: BTreeMap<String, SettingValue>,
}

impl Validate for SyncedSettings {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        check::count("values", &self.values.keys().collect::<Vec<_>>(), limits::MAX_SETTINGS)?;
        for (name, v) in &self.values {
            check::key("setting", name, limits::MAX_KEY)?;
            check::free("setting value", &v.value, limits::MAX_SETTING_VALUE)?;
            check::number("changed_at", v.changed_at, 4_102_444_800)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HubKind {
    Map,
    Mod,
}

/// One Hub map or mod installed on a device. Files come from the Hub, never from the cloud.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubItem {
    pub id: String,
    pub kind: HubKind,
    pub version: String,
}

/// The installed Hub items of an account (offered to another device: «Установить то же самое?»).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubList {
    pub schema: u32,
    pub updated_at: u64,
    pub items: Vec<HubItem>,
}

impl Validate for HubList {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        check::number("updated_at", self.updated_at, 4_102_444_800)?;
        check::count("items", &self.items, limits::MAX_HUB_ITEMS)?;
        for it in &self.items {
            check::key("hub id", &it.id, limits::MAX_KEY)?;
            check::text("hub version", &it.version, limits::MAX_VERSION)?;
        }
        Ok(())
    }
}

/// One scene of the performance test («Тест производительности»).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchScene {
    pub name: String,
    pub median_ms: f32,
    pub p99_ms: f32,
    pub frames: u32,
}

/// The result of the performance test, synced to the account.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchReport {
    pub schema: u32,
    pub razdor: String,
    pub at: u64,
    pub device: DeviceInfo,
    pub settings: BTreeMap<String, String>,
    pub scenes: Vec<BenchScene>,
}

impl Validate for BenchReport {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        check::text("razdor", &self.razdor, limits::MAX_VERSION)?;
        check::number("at", self.at, 4_102_444_800)?;
        self.device.validate()?;
        check::settings("settings", &self.settings)?;
        check::count("scenes", &self.scenes, limits::MAX_BENCH_SCENES)?;
        for s in &self.scenes {
            check::key("scene", &s.name, limits::MAX_KEY)?;
            check::float("median_ms", s.median_ms, 600_000.0)?;
            check::float("p99_ms", s.p99_ms, 600_000.0)?;
            check::number("frames", u64::from(s.frames), 10_000_000)?;
        }
        Ok(())
    }
}
