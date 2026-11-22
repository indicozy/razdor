//! The crate's tests: serde round trips, validation (good and refused), forward compatibility and
//! framing. No I/O.

use std::collections::BTreeMap;

use serde::{de::DeserializeOwned, Serialize};

use crate::analytics::*;
use crate::cloud::*;
use crate::frame;
use crate::ids::{AccountKey, InstallId, LinkCode};
use crate::limits;
use crate::{Validate, WireError, SCHEMA};

const ID: &str = "0123456789abcdef0123456789abcdef";
const SHA: &str = "ab00000000000000000000000000000000000000000000000000000000000cd1";
const HASH: &str = "0123456789abcdef";

fn round<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(v: &T) -> T {
    let text = serde_json::to_string(v).expect("encode");
    let back: T = serde_json::from_str(&text).expect("decode");
    assert_eq!(&back, v);
    back
}

fn install() -> InstallId {
    InstallId::parse(ID).unwrap()
}

fn ping() -> Ping {
    Ping {
        schema: SCHEMA,
        install_id: install(),
        razdor: "1.5.4".into(),
        platform: "linux x86_64 64".into(),
        game: "community-update".into(),
        lang: "ru".into(),
        day: Some(20_000),
    }
}

#[test]
fn a_ping_without_a_day_is_an_old_client_and_still_decodes() {
    // Old clients omit `day`; the server then counts the ping by its arrival day.
    let old = r#"{"schema":1,"install_id":"0123456789abcdef0123456789abcdef","razdor":"1.5.4",
        "platform":"linux x86_64 64","game":"dt","lang":"ru"}"#;
    let p: Ping = serde_json::from_str(old).expect("an old ping decodes");
    assert_eq!(p.day, None);
    let new = serde_json::to_string(&Ping { day: Some(20_000), ..p }).unwrap();
    assert!(new.contains("\"day\":20000"), "the claimed day goes on the wire: {new}");
}

fn meta() -> SaveMeta {
    SaveMeta {
        schema: SCHEMA,
        game: "dt".into(),
        map: "Пепельная Марка".into(),
        kind: SaveKind::Manual,
        device_id: "0123456789abcdef0123456789abcdef".into(),
        device: "ПК".into(),
        name: "перед боем".into(),
        saved_at: 1_700_000_000,
        razdor: "1.5.4".into(),
        size: 1000,
        sha256: SHA.into(),
        raw_sha256: SHA.into(),
        pinned: true,
    }
}

fn battle() -> BattleUpload {
    BattleUpload {
        schema: SCHEMA,
        install_id: install(),
        razdor: "1.5.4".into(),
        platform: "linux x86_64 64".into(),
        game: "dt".into(),
        map: "map01".into(),
        map_hash: HASH.into(),
        mod_hash: None,
        rules: 42,
        experiments: vec!["six-cells".into()],
        tags: vec![BattleTag::Autobattle],
        outcome: BattleOutcome { winner: Side::Player, turns: 12, player_losses: 3, foe_losses: 9 },
        started_at: 1_700_000_000,
        ended_at: 1_700_000_100,
        record_sha256: SHA.into(),
        record_len: 2048,
    }
}

fn device() -> DeviceInfo {
    DeviceInfo {
        os: "Linux".into(),
        os_version: "7.2".into(),
        gpu: "RTX 4060".into(),
        gpu_driver: "590".into(),
        cpu_cores: 12,
        ram_mb: 32_000,
        screen_w: 2560,
        screen_h: 1440,
        scale_pct: 100,
        phone: None,
    }
}

fn perf() -> PerfReport {
    PerfReport {
        schema: SCHEMA,
        install_id: install(),
        razdor: "1.5.4".into(),
        device: device(),
        quality: BTreeMap::from([("preset".to_string(), "high".to_string())]),
        screens: vec![ScreenStats { screen: "map".into(), median_ms: 8.1, p99_ms: 14.0, p999_ms: 22.5, minutes: 3.5 }],
        loads: vec![LoadTime { what: "map".into(), ms: 900 }],
        memory: MemoryInfo { peak_ram_mb: Some(900), peak_vram_mb: None, low_memory_warnings: 0 },
        profile: vec![ProfilePart { part: "draw".into(), ms: 3.2 }],
    }
}

// Round trips.

#[test]
fn round_trip_analytics_bodies() {
    round(&ping());
    round(&InstallDelete { schema: SCHEMA, install_id: install() });
    round(&perf());
    round(&battle());
    round(&CrashSaveHeader {
        schema: SCHEMA,
        install_id: install(),
        crash_id: "c1".into(),
        sha256: SHA.into(),
        len: 10,
    });
    round(&Verdict { schema: SCHEMA, battle_id: "b1".into(), kind: VerdictKind::Ok, reason: None });
    round(&crate::Ack { id: Some("x".into()), ok: false });
    round(&crate::Ack::confirmed());
}

#[test]
fn a_silent_answer_is_not_a_confirmed_one() {
    // The silent 200 is `{}`: it must decode as not confirmed, and a confirmed answer must not be `{}`.
    let silent: crate::Ack = serde_json::from_str("{}").unwrap();
    assert!(!silent.ok);
    let confirmed = serde_json::to_string(&crate::Ack::confirmed()).unwrap();
    assert_eq!(confirmed, r#"{"ok":true}"#);
    let old_client_shape: crate::Ack = serde_json::from_str(&confirmed).unwrap();
    assert!(old_client_shape.ok);
}

#[test]
fn round_trip_every_event_kind() {
    let kinds = vec![
        EventKind::SessionStart { screen_w: 1920, screen_h: 1080, graphics: "high".into() },
        EventKind::SessionEnd { seconds: 3600 },
        EventKind::MapStart {
            map: "map01".into(),
            map_hash: HASH.into(),
            difficulty: "normal".into(),
            ai_strength: "standard".into(),
            experiments: vec!["six-cells".into()],
        },
        EventKind::Progress { day: 40, hero_level: 5, army_size: 12 },
        EventKind::MapEnd {
            map: "map01".into(),
            outcome: Outcome::Abandoned,
            last_day: 41,
            last_screen: "map".into(),
            last_battle: Some(BattleOutcome { winner: Side::Foe, turns: 3, player_losses: 2, foe_losses: 0 }),
            seconds_in_map: 5400,
        },
        EventKind::OlderSaveLoaded { source: SaveSource::Auto, save_version: 9 },
        EventKind::SettingsChanged { changed: vec![Setting { name: "speed".into(), value: "fast".into() }] },
        EventKind::UiCounters { windows: BTreeMap::from([("settings".to_string(), 2)]), hints: 4 },
    ];
    assert_eq!(kinds.len(), 8);
    for k in kinds {
        let e = Event { at: 1_700_000_000, kind: k };
        round(&e);
        e.validate().unwrap();
    }
}

#[test]
fn round_trip_cloud_bodies() {
    let key = AccountKey::parse(&"f".repeat(64)).unwrap();
    round(&AccountCreated { key: key.clone() });
    round(&LinkIssued { code: LinkCode::parse("4F7K2Q9M").unwrap(), expires_at: 1_700_000_600 });
    round(&LinkRedeem { schema: SCHEMA, code: "4f7k-2q9m".into() });
    round(&LinkRedeemed { key });
    round(&meta());
    round(&SaveList { saves: vec![SaveEntry { id: "s1".into(), meta: meta() }], used_bytes: 1000, quota_bytes: 20_000_000 });
    round(&SavePut { id: "s1".into(), used_bytes: 1000 });
    let mut maps = BTreeMap::new();
    maps.insert("map01".to_string(), MarkRecord { level: MarkLevel::Yellow, date: "2026-11-22".into(), mark: serde_json::json!({"level": "Yellow"}) });
    round(&Marks { schema: SCHEMA, updated_at: 1, maps });
    let mut values = BTreeMap::new();
    values.insert("speed".to_string(), SettingValue { value: "fast".into(), changed_at: 5 });
    round(&SyncedSettings { schema: SCHEMA, values });
    round(&HubList { schema: SCHEMA, updated_at: 1, items: vec![HubItem { id: "evo".into(), kind: HubKind::Mod, version: "2".into() }] });
    round(&BenchReport {
        schema: SCHEMA,
        razdor: "1.5.4".into(),
        at: 1,
        device: device(),
        settings: BTreeMap::new(),
        scenes: vec![BenchScene { name: "map".into(), median_ms: 9.0, p99_ms: 12.0, frames: 600 }],
    });
}

// Validation: good bodies pass.

#[test]
fn good_bodies_validate() {
    ping().validate().unwrap();
    perf().validate().unwrap();
    battle().validate().unwrap();
    meta().validate().unwrap();
    EventBatch { schema: SCHEMA, install_id: install(), razdor: "1.5.4".into(), events: vec![] }
        .validate()
        .unwrap();
    LinkCode::parse("4f7k 2q9m").unwrap().validate().unwrap();
}

fn refused<T: Validate>(v: &T, field: &str) {
    match v.validate() {
        Err(e) => assert!(format!("{e}").contains(field), "wrong field in {e}"),
        Ok(()) => panic!("accepted a bad body ({field})"),
    }
}

#[test]
fn schema_zero_and_newer_are_refused() {
    let mut p = ping();
    p.schema = 0;
    refused(&p, "schema");
    p.schema = SCHEMA + 1;
    refused(&p, "schema");
}

#[test]
fn bad_identities_are_refused() {
    assert!(InstallId::parse("ABCDEF0123456789abcdef0123456789").is_err(), "upper case");
    assert!(InstallId::parse("abc").is_err(), "short");
    assert!(AccountKey::parse(&"a".repeat(31)).is_err(), "under 128 bits");
    assert!(AccountKey::parse(&"g".repeat(40)).is_err(), "not hex");
    assert!(LinkCode::parse("4F7K-2Q9").is_err(), "7 characters");
    assert!(LinkCode::parse("0OIL-1234").is_err(), "characters outside the alphabet");
}

#[test]
fn text_and_key_rules() {
    let mut p = ping();
    p.razdor = "1.5\n4".into();
    refused(&p, "control");
    let mut p = ping();
    p.game = "../etc".into();
    refused(&p, "path");
    let mut p = ping();
    p.razdor = "x".repeat(limits::MAX_VERSION + 1);
    refused(&p, "razdor");
    let mut p = ping();
    p.game = String::new();
    refused(&p, "game");
}

#[test]
fn event_batch_limits() {
    let event = Event { at: 1, kind: EventKind::SessionEnd { seconds: 1 } };
    let batch = |n: usize| EventBatch {
        schema: SCHEMA,
        install_id: install(),
        razdor: "1.5.4".into(),
        events: vec![event.clone(); n],
    };
    batch(limits::MAX_EVENTS_PER_BATCH).validate().unwrap();
    refused(&batch(limits::MAX_EVENTS_PER_BATCH + 1), "events");
    let mut e = event.clone();
    e.at = 0;
    let mut b = batch(1);
    b.events[0] = e;
    refused(&b, "at");
}

#[test]
fn unknown_event_kind_and_enum_values_fail_to_decode() {
    let bad = r#"{"at":1,"kind":"teleport","seconds":3}"#;
    assert!(serde_json::from_str::<Event>(bad).is_err());
    let bad_outcome = r#"{"at":1,"kind":"map_end","outcome":"draw","last_day":1,"last_screen":"map","last_battle":null,"seconds_in_map":1}"#;
    assert!(serde_json::from_str::<Event>(bad_outcome).is_err());
    let bad_side = r#"{"winner":"neutral","turns":1,"player_losses":0,"foe_losses":0}"#;
    assert!(serde_json::from_str::<BattleOutcome>(bad_side).is_err());
}

#[test]
fn unknown_fields_are_ignored() {
    let text = r#"{"schema":1,"install_id":"0123456789abcdef0123456789abcdef","razdor":"1.5.4","platform":"linux","game":"dt","lang":"ru","future_field":[1,2,3]}"#;
    let p: Ping = serde_json::from_str(text).expect("a newer peer's extra field");
    p.validate().unwrap();
}

#[test]
fn perf_and_crash_limits() {
    let mut p = perf();
    p.profile = vec![ProfilePart { part: "x".into(), ms: 1.0 }; limits::MAX_PROFILE_PARTS + 1];
    refused(&p, "profile");
    let mut p = perf();
    p.screens[0].median_ms = f32::NAN;
    refused(&p, "median_ms");
    let mut p = perf();
    p.device.cpu_cores = 100_000;
    refused(&p, "cpu_cores");

    let crash = CrashReport {
        schema: SCHEMA,
        install_id: install(),
        razdor: "1.5.4".into(),
        platform: "linux".into(),
        game: "dt".into(),
        screen: "map".into(),
        at: 1_700_000_000,
        settings: BTreeMap::new(),
        panic_text: "boom\n  at src/x.rs:1".into(),
        log: vec!["line\twith tab".into(); limits::MAX_LOG_LINES],
        has_save: true,
    };
    crash.validate().unwrap();
    let mut too_many = crash.clone();
    too_many.log.push("one more".into());
    refused(&too_many, "log");
    let mut long = crash.clone();
    long.log = vec!["x".repeat(limits::MAX_LOG_LINE_CHARS + 1)];
    refused(&long, "log line");
}

#[test]
fn battle_and_verdict_rules() {
    let mut b = battle();
    b.ended_at = b.started_at - 1;
    refused(&b, "ended_at");
    let mut b = battle();
    b.map_hash = "XYZ".into();
    refused(&b, "map_hash");
    let mut b = battle();
    b.record_len = limits::MAX_BATTLE_BODY as u64 + 1;
    refused(&b, "record_len");
    let mut b = battle();
    b.tags = vec![BattleTag::ReloadBeforeEnd; limits::MAX_BATTLE_TAGS + 1];
    refused(&b, "tags");

    let suspicious = Verdict { schema: SCHEMA, battle_id: "b1".into(), kind: VerdictKind::Suspicious, reason: None };
    refused(&suspicious, "reason");
    let suspicious = Verdict { reason: Some("units moved twice".into()), ..suspicious };
    suspicious.validate().unwrap();
}

#[test]
fn save_rules() {
    let mut m = meta();
    m.sha256 = "not-a-hash".into();
    refused(&m, "sha256");
    let mut m = meta();
    m.size = limits::MAX_SAVE_BODY as u64 + 1;
    refused(&m, "size");
    let mut m = meta();
    m.game = "a/b".into();
    refused(&m, "path");
    let mut m = meta();
    m.name = "x".repeat(limits::MAX_SAVE_NAME + 1);
    refused(&m, "name");
    let mut m = meta();
    m.raw_sha256 = "short".into();
    refused(&m, "raw_sha256");
    let mut m = meta();
    m.device_id = "not-hex".into();
    refused(&m, "device_id");
    // An old row (no device id, no raw hash) is listable; a current upload needs both (the server).
    let mut m = meta();
    m.device_id.clear();
    m.raw_sha256.clear();
    assert!(m.validate().is_ok(), "rows from before device ids stay listable");
    assert_eq!(limits::SAVES_PER_ACCOUNT, 4 * limits::SAVES_PER_DEVICE);
}

#[test]
fn marks_and_settings_rules() {
    let rec = MarkRecord { level: MarkLevel::Green, date: String::new(), mark: serde_json::json!("Honest") };
    let mut maps = BTreeMap::new();
    for i in 0..=limits::MAX_MARK_MAPS {
        maps.insert(format!("m{i}"), rec.clone());
    }
    refused(&Marks { schema: SCHEMA, updated_at: 1, maps }, "maps");

    let big = MarkRecord { level: MarkLevel::Red, date: String::new(), mark: serde_json::Value::String("x".repeat(limits::MAX_MARK_VALUE_BYTES)) };
    refused(&big, "mark");

    let mut values = BTreeMap::new();
    for i in 0..=limits::MAX_SETTINGS {
        values.insert(format!("k{i}"), SettingValue { value: "v".into(), changed_at: 1 });
    }
    refused(&SyncedSettings { schema: SCHEMA, values }, "values");
}

#[test]
fn link_code_normalizes_typed_input() {
    assert_eq!(LinkCode::parse(" 4f7k2q9m ").unwrap().as_str(), "4F7K-2Q9M");
    assert_eq!(LinkCode::parse("4F7K-2Q9M").unwrap().as_str(), "4F7K-2Q9M");
}

// Framing.

#[test]
fn framed_round_trip_keeps_the_body_bytes() {
    let body: Vec<u8> = (0u8..=255).collect();
    let bytes = frame::encode(&battle(), &body).unwrap();
    let (header, rest): (BattleUpload, &[u8]) = frame::decode(&bytes).unwrap();
    assert_eq!(header, battle());
    assert_eq!(rest, &body[..]);
}

#[test]
fn framed_refuses_cut_and_oversized_headers() {
    assert_eq!(frame::decode::<BattleUpload>(&[1u8, 0]).unwrap_err(), WireError::Frame("shorter than its length prefix"));
    let mut bytes = frame::encode(&meta(), b"data").unwrap();
    bytes.truncate(10);
    assert!(matches!(frame::decode::<SaveMeta>(&bytes), Err(WireError::Frame(_))));
    let mut huge = (limits::MAX_FRAME_HEADER as u32 + 1).to_le_bytes().to_vec();
    huge.extend_from_slice(b"{}");
    assert_eq!(frame::decode::<SaveMeta>(&huge).unwrap_err(), WireError::Frame("header longer than the limit"));
    let big_header = MarkRecord { level: MarkLevel::Green, date: String::new(), mark: serde_json::Value::String("x".repeat(limits::MAX_FRAME_HEADER)) };
    assert!(matches!(frame::encode(&big_header, b""), Err(WireError::TooLong { .. })));
}

#[test]
fn limits_are_consistent() {
    assert_eq!(limits::LINK_ALPHABET.len(), 31, "digits 2-9 and 23 letters");
}
