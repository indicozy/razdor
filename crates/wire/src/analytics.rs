//! The analytics bodies (`2026-11-22-analytics-design.md`, What is sent): the daily ping, play
//! events, performance, crash reports, battle records and the verifier's verdicts.
//!
//! Battle records and crash saves are framed ([`crate::frame`]): the header is [`BattleUpload`] or
//! [`CrashSaveHeader`], the body the raw bytes. Everything else is plain JSON.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::check;
use crate::ids::InstallId;
use crate::limits;
use crate::{Validate, WireResult};

/// The daily ping. Carries nothing about the person, so it is sent with or without consent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ping {
    pub schema: u32,
    pub install_id: InstallId,
    /// Razdor's version (`CARGO_PKG_VERSION`).
    pub razdor: String,
    /// `os arch bits`, as the replay header writes it.
    pub platform: String,
    /// `dt`, `community-update`, `evolution`, or a mod's id.
    pub game: String,
    /// The interface language code.
    pub lang: String,
    /// The UTC day the ping is for (days since 1970). A client sends its queued days late, so the
    /// server counts a ping by this day, not by when it arrives. Optional: old clients omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<i64>,
}

impl Validate for Ping {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        self.install_id.validate()?;
        check::text("razdor", &self.razdor, limits::MAX_VERSION)?;
        check::text("platform", &self.platform, limits::MAX_TEXT)?;
        check::key("game", &self.game, limits::MAX_KEY)?;
        check::key("lang", &self.lang, limits::MAX_VERSION)
    }
}

/// «Удалить мои данные»: the server deletes everything of the install id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallDelete {
    pub schema: u32,
    pub install_id: InstallId,
}

impl Validate for InstallDelete {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        self.install_id.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Win,
    Loss,
    Abandoned,
}

/// Where a loaded save came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveSource {
    Manual,
    Auto,
    Quick,
    Session,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Player,
    Foe,
    Draw,
    None,
}

/// What a battle record is tagged with: the player let the autobattle play, or reloaded before the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BattleTag {
    Autobattle,
    ReloadBeforeEnd,
}

/// The end of a battle: who won, how many turns, the losses of each side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BattleOutcome {
    pub winner: Side,
    pub turns: u32,
    pub player_losses: u32,
    pub foe_losses: u32,
}

impl Validate for BattleOutcome {
    fn validate(&self) -> WireResult {
        check::number("turns", u64::from(self.turns), 100_000)?;
        check::number("player_losses", u64::from(self.player_losses), 1_000_000)?;
        check::number("foe_losses", u64::from(self.foe_losses), 1_000_000)
    }
}

/// A game setting that differs from the default: its catalog name and its value's catalog key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setting {
    pub name: String,
    pub value: String,
}

/// The seven kinds of play event of the spec, section 4 (the session counts as start and end).
/// Serialized internally tagged: `{"kind": "map_end", ...}`. An unknown kind fails to decode, so a
/// server must read a batch's events one by one and drop the unknown ones.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    SessionStart { screen_w: u32, screen_h: u32, graphics: String },
    SessionEnd { seconds: u64 },
    MapStart { map: String, map_hash: String, difficulty: String, ai_strength: String, experiments: Vec<String> },
    Progress { day: u32, hero_level: u32, army_size: u32 },
    MapEnd {
        /// The map's file name, as its `MapStart` named it (the server groups the ends by map).
        map: String,
        outcome: Outcome,
        last_day: u32,
        last_screen: String,
        last_battle: Option<BattleOutcome>,
        seconds_in_map: u64,
    },
    OlderSaveLoaded { source: SaveSource, save_version: u32 },
    SettingsChanged { changed: Vec<Setting> },
    UiCounters { windows: BTreeMap<String, u32>, hints: u32 },
}

/// One event: its time (seconds since 1970) and its kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub at: u64,
    #[serde(flatten)]
    pub kind: EventKind,
}

/// A batch of play events, sent in the background (at most a few per minute).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBatch {
    pub schema: u32,
    pub install_id: InstallId,
    pub razdor: String,
    pub events: Vec<Event>,
}

impl Validate for EventBatch {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        self.install_id.validate()?;
        check::text("razdor", &self.razdor, limits::MAX_VERSION)?;
        check::count("events", &self.events, limits::MAX_EVENTS_PER_BATCH)?;
        self.events.iter().try_for_each(Validate::validate)
    }
}

impl Validate for Event {
    fn validate(&self) -> WireResult {
        if self.at == 0 {
            return Err(crate::WireError::Invalid { field: "at", reason: "no time" });
        }
        check::number("at", self.at, 4_102_444_800)?;
        self.kind.validate()
    }
}

fn names(field: &'static str, list: &[String]) -> WireResult {
    check::count(field, list, limits::MAX_LIST)?;
    list.iter().try_for_each(|s| check::key(field, s, limits::MAX_KEY))
}

impl Validate for EventKind {
    fn validate(&self) -> WireResult {
        match self {
            EventKind::SessionStart { screen_w, screen_h, graphics } => {
                check::number("screen_w", u64::from(*screen_w), 16_384)?;
                check::number("screen_h", u64::from(*screen_h), 16_384)?;
                check::key("graphics", graphics, limits::MAX_KEY)
            }
            EventKind::SessionEnd { seconds } => check::number("seconds", *seconds, 10_000_000),
            EventKind::MapStart { map, map_hash, difficulty, ai_strength, experiments } => {
                check::key("map", map, limits::MAX_KEY)?;
                check::hex("map_hash", map_hash, 16, 16)?;
                check::key("difficulty", difficulty, limits::MAX_KEY)?;
                check::key("ai_strength", ai_strength, limits::MAX_KEY)?;
                names("experiments", experiments)
            }
            EventKind::Progress { day, hero_level, army_size } => {
                check::number("day", u64::from(*day), 1_000_000)?;
                check::number("hero_level", u64::from(*hero_level), 1000)?;
                check::number("army_size", u64::from(*army_size), 100_000)
            }
            EventKind::MapEnd { map, last_day, last_screen, last_battle, seconds_in_map, .. } => {
                check::key("map", map, limits::MAX_KEY)?;
                check::number("last_day", u64::from(*last_day), 1_000_000)?;
                check::key("last_screen", last_screen, limits::MAX_KEY)?;
                if let Some(battle) = last_battle {
                    battle.validate()?;
                }
                check::number("seconds_in_map", *seconds_in_map, 10_000_000)
            }
            EventKind::OlderSaveLoaded { save_version, .. } => {
                check::number("save_version", u64::from(*save_version), 10_000)
            }
            EventKind::SettingsChanged { changed } => {
                check::count("changed", changed, limits::MAX_LIST)?;
                changed.iter().try_for_each(|s| {
                    check::key("setting", &s.name, limits::MAX_KEY)?;
                    check::key("setting value", &s.value, limits::MAX_KEY)
                })
            }
            EventKind::UiCounters { windows, hints } => {
                check::count("windows", &windows.keys().collect::<Vec<_>>(), limits::MAX_SETTINGS)?;
                for (name, n) in windows {
                    check::key("window", name, limits::MAX_KEY)?;
                    check::number("window count", u64::from(*n), 1_000_000)?;
                }
                check::number("hints", u64::from(*hints), 1_000_000)
            }
        }
    }
}

/// The device, once a session (`PerfReport`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub os: String,
    pub os_version: String,
    pub gpu: String,
    pub gpu_driver: String,
    pub cpu_cores: u32,
    pub ram_mb: u32,
    pub screen_w: u32,
    pub screen_h: u32,
    pub scale_pct: u32,
    /// The phone's model, on Android and iOS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
}

impl Validate for DeviceInfo {
    fn validate(&self) -> WireResult {
        check::text("os", &self.os, limits::MAX_TEXT)?;
        check::opt_text("os_version", &self.os_version, limits::MAX_TEXT)?;
        check::opt_text("gpu", &self.gpu, limits::MAX_TEXT)?;
        check::opt_text("gpu_driver", &self.gpu_driver, limits::MAX_TEXT)?;
        check::number("cpu_cores", u64::from(self.cpu_cores), 1024)?;
        check::number("ram_mb", u64::from(self.ram_mb), 4_000_000)?;
        check::number("screen_w", u64::from(self.screen_w), 16_384)?;
        check::number("screen_h", u64::from(self.screen_h), 16_384)?;
        check::number("scale_pct", u64::from(self.scale_pct), 1000)?;
        match &self.phone {
            Some(p) => check::opt_text("phone", p, limits::MAX_TEXT),
            None => Ok(()),
        }
    }
}

/// Frame statistics of one screen (map, battle, menu, a window), over the minutes spent there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScreenStats {
    pub screen: String,
    pub median_ms: f32,
    pub p99_ms: f32,
    pub p999_ms: f32,
    pub minutes: f32,
}

/// One load time: `start`, `map`, `battle_entry`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadTime {
    pub what: String,
    pub ms: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_ram_mb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_vram_mb: Option<u32>,
    /// The Android low-memory warnings of the session.
    pub low_memory_warnings: u32,
}

/// One of the profiler's top parts of a session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfilePart {
    pub part: String,
    pub ms: f32,
}

/// Performance of one session: the device, the quality settings, the frames per screen, the loads,
/// the memory and the profiler's top five.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PerfReport {
    pub schema: u32,
    pub install_id: InstallId,
    pub razdor: String,
    pub device: DeviceInfo,
    /// The quality settings (`quality` in the spec's words: the graphics preset and its parts).
    pub quality: BTreeMap<String, String>,
    pub screens: Vec<ScreenStats>,
    pub loads: Vec<LoadTime>,
    pub memory: MemoryInfo,
    pub profile: Vec<ProfilePart>,
}

impl Validate for PerfReport {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        self.install_id.validate()?;
        check::text("razdor", &self.razdor, limits::MAX_VERSION)?;
        self.device.validate()?;
        check::settings("quality", &self.quality)?;
        check::count("screens", &self.screens, limits::MAX_SCREENS)?;
        for s in &self.screens {
            check::key("screen", &s.screen, limits::MAX_KEY)?;
            check::float("median_ms", s.median_ms, 600_000.0)?;
            check::float("p99_ms", s.p99_ms, 600_000.0)?;
            check::float("p999_ms", s.p999_ms, 600_000.0)?;
            check::float("minutes", s.minutes, 100_000.0)?;
        }
        check::count("loads", &self.loads, limits::MAX_LOADS)?;
        for l in &self.loads {
            check::key("load", &l.what, limits::MAX_KEY)?;
            check::number("load ms", u64::from(l.ms), 3_600_000)?;
        }
        check::number("low_memory_warnings", u64::from(self.memory.low_memory_warnings), 100_000)?;
        check::count("profile", &self.profile, limits::MAX_PROFILE_PARTS)?;
        for p in &self.profile {
            check::key("profile part", &p.part, limits::MAX_KEY)?;
            check::float("profile ms", p.ms, 600_000.0)?;
        }
        Ok(())
    }
}

/// A crash report, sent on the next start: the panic with its backtrace, the log's last lines, the
/// open screen and the settings. Paths are already cleaned by the game (`<home>`, `<dt>`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashReport {
    pub schema: u32,
    pub install_id: InstallId,
    pub razdor: String,
    pub platform: String,
    pub game: String,
    pub screen: String,
    /// When the crash happened (seconds since 1970).
    pub at: u64,
    pub settings: BTreeMap<String, String>,
    pub panic_text: String,
    pub log: Vec<String>,
    /// Whether a save follows (`CRASH_SAVE`); the player chooses «Отправить» for it.
    pub has_save: bool,
}

impl Validate for CrashReport {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        self.install_id.validate()?;
        check::text("razdor", &self.razdor, limits::MAX_VERSION)?;
        check::text("platform", &self.platform, limits::MAX_TEXT)?;
        check::key("game", &self.game, limits::MAX_KEY)?;
        check::key("screen", &self.screen, limits::MAX_KEY)?;
        check::number("at", self.at, 4_102_444_800)?;
        check::settings("settings", &self.settings)?;
        check::free("panic_text", &self.panic_text, limits::MAX_PANIC_CHARS)?;
        check::count("log", &self.log, limits::MAX_LOG_LINES)?;
        self.log.iter().try_for_each(|l| check::free("log line", l, limits::MAX_LOG_LINE_CHARS))
    }
}

/// The header of a crash's save upload (`CRASH_SAVE`, framed): the crash it belongs to and the file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashSaveHeader {
    pub schema: u32,
    pub install_id: InstallId,
    /// The id the server gave the crash report ([`crate::Ack::id`]).
    pub crash_id: String,
    pub sha256: String,
    /// The save's length in bytes; must equal the body's.
    pub len: u64,
}

impl Validate for CrashSaveHeader {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        self.install_id.validate()?;
        check::key("crash_id", &self.crash_id, limits::MAX_KEY)?;
        check::sha256("sha256", &self.sha256)?;
        check::number("len", self.len, limits::MAX_SAVE_BODY as u64)
    }
}

/// The header of a battle record upload (`BATTLES`, framed: the body is the record file's bytes).
/// The record holds the battle's start (armies, items, terrain, rules, experiments), every move in
/// order and the result; this header is the metadata the server indexes and the verifier reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BattleUpload {
    pub schema: u32,
    pub install_id: InstallId,
    pub razdor: String,
    pub platform: String,
    pub game: String,
    /// The map's file name and the hash of its bytes (`ReplayHeader` writes the same).
    pub map: String,
    pub map_hash: String,
    /// The hub mod's hash, when a mod is played; none for the base game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_hash: Option<String>,
    /// The rules fingerprint (`recording::header::rules_fingerprint`).
    pub rules: u64,
    pub experiments: Vec<String>,
    pub tags: Vec<BattleTag>,
    pub outcome: BattleOutcome,
    pub started_at: u64,
    pub ended_at: u64,
    pub record_sha256: String,
    /// The record's length in bytes; must equal the body's.
    pub record_len: u64,
}

impl Validate for BattleUpload {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        self.install_id.validate()?;
        check::text("razdor", &self.razdor, limits::MAX_VERSION)?;
        check::text("platform", &self.platform, limits::MAX_TEXT)?;
        check::key("game", &self.game, limits::MAX_KEY)?;
        check::key("map", &self.map, limits::MAX_KEY)?;
        check::hex("map_hash", &self.map_hash, 16, 16)?;
        if let Some(h) = &self.mod_hash {
            check::hex("mod_hash", h, 16, 16)?;
        }
        names("experiments", &self.experiments)?;
        check::count("tags", &self.tags, limits::MAX_BATTLE_TAGS)?;
        self.outcome.validate()?;
        check::number("started_at", self.started_at, 4_102_444_800)?;
        check::number("ended_at", self.ended_at, 4_102_444_800)?;
        if self.ended_at < self.started_at {
            return Err(crate::WireError::Invalid { field: "ended_at", reason: "before the start" });
        }
        check::sha256("record_sha256", &self.record_sha256)?;
        check::number("record_len", self.record_len, limits::MAX_BATTLE_BODY as u64)
    }
}

/// The verifier's verdict on a battle record: `ok` or `suspicious`, with the reason for the latter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictKind {
    Ok,
    Suspicious,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub schema: u32,
    /// The battle's id, as the server issued it.
    pub battle_id: String,
    pub kind: VerdictKind,
    /// Required for `suspicious`: what the replay did not match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Validate for Verdict {
    fn validate(&self) -> WireResult {
        check::schema(self.schema)?;
        check::key("battle_id", &self.battle_id, limits::MAX_KEY)?;
        match (&self.kind, &self.reason) {
            (VerdictKind::Suspicious, None) => {
                Err(crate::WireError::Invalid { field: "reason", reason: "a suspicious verdict needs a reason" })
            }
            (_, Some(r)) => check::text("reason", r, limits::MAX_TEXT),
            (VerdictKind::Ok, None) => Ok(()),
        }
    }
}
