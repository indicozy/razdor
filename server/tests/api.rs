//! The service end to end, through the router (axum's test client, no socket): every endpoint's
//! happy path and refusals, the bans and the quarantine, the cloud's limits, the link codes, the
//! deletion, the retention job (time injected) and the dashboard.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::{to_bytes, Body};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::Router;
use razdor_server::clock::Clock;
use razdor_server::config::{Config, Limits};
use razdor_server::state::{sha256_hex, AppState};
use razdor_server::{admin, retention, router, summary};
use razdor_wire::frame;
use serde_json::{json, Value};
use tower::ServiceExt;

const NOW: i64 = 1_769_000_000;
const TOKEN: &str = "test-admin-token";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A data directory that is removed when the test ends.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Harness {
    st: AppState,
    _dir: Dir,
}

fn harness(name: &str, limits: Limits) -> Harness {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cfg = Config {
        data_dir: dir.clone(),
        trust_proxy: true,
        secure_cookies: false,
        admin_token_sha256: Some(sha256_hex(&[TOKEN.as_bytes()])),
        admin_password_hash: Some(admin::hash_password("pw").expect("hashes")),
        limits,
        ..Config::default()
    };
    let st = AppState::open(cfg, Clock::fixed(NOW)).expect("opens");
    Harness { st, _dir: Dir(dir) }
}

struct Resp {
    status: StatusCode,
    body: Vec<u8>,
    headers: HeaderMap,
}

impl Resp {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

async fn call(app: &Router, method: &str, uri: &str, headers: &[(&str, String)], body: Vec<u8>) -> Resp {
    let mut req = Request::builder().method(method).uri(uri).header("x-real-ip", "203.0.113.7");
    for (k, v) in headers {
        req = req.header(*k, v.as_str());
    }
    let resp = app.clone().oneshot(req.body(Body::from(body)).expect("request")).await.expect("served");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body").to_vec();
    Resp { status, body, headers }
}

async fn post_json(h: &Harness, uri: &str, v: &Value) -> Resp {
    call(&router(h.st.clone()), "POST", uri, &[("content-type", "application/json".into())], v.to_string().into_bytes())
        .await
}

fn install(n: u32) -> String {
    format!("{:032x}", 0xabcdef00u64 << 32 | u64::from(n))
}

fn ping_body(id: &str) -> Value {
    json!({ "schema": 1, "install_id": id, "razdor": "1.5.4", "platform": "linux x86_64 64", "game": "dt", "lang": "ru" })
}

fn progress_event(at: i64) -> Value {
    json!({ "at": at, "kind": "progress", "day": 3, "hero_level": 2, "army_size": 9 })
}

async fn count(h: &Harness, sql: &'static str) -> i64 {
    h.st.stats.call(move |c| Ok(c.query_row(sql, [], |r| r.get::<_, i64>(0))?)).await.expect("count")
}

async fn cloud_count(h: &Harness, sql: &'static str) -> i64 {
    h.st.cloud.call(move |c| Ok(c.query_row(sql, [], |r| r.get::<_, i64>(0))?)).await.expect("count")
}

fn battle_frame(id: &str, started: u64, ended: u64, record: &[u8]) -> Vec<u8> {
    let header = json!({
        "schema": 1, "install_id": id, "razdor": "1.5.4", "platform": "linux x86_64 64", "game": "dt",
        "map": "map01.dtm", "map_hash": "0123456789abcdef", "rules": 42, "experiments": ["six-cells"], "tags": [],
        "outcome": { "winner": "player", "turns": 3, "player_losses": 1, "foe_losses": 2 },
        "started_at": started, "ended_at": ended,
        "record_sha256": sha256_hex(&[record]), "record_len": record.len()
    });
    frame::encode(&header, record).expect("frames")
}

async fn account(h: &Harness) -> String {
    let r = post_json(h, "/v1/account", &json!({ "schema": 1 })).await;
    assert_eq!(r.status, StatusCode::OK);
    r.json()["key"].as_str().expect("key").to_string()
}

fn key_header(key: &str) -> Vec<(&'static str, String)> {
    vec![("x-razdor-key", key.to_string())]
}

fn save_frame(game: &str, map: &str, kind: &str, bytes: &[u8]) -> Vec<u8> {
    let meta = json!({
        "schema": 1, "game": game, "map": map, "kind": kind, "device": "PC", "name": "slot",
        "saved_at": NOW, "razdor": "1.5.4", "size": bytes.len(), "sha256": sha256_hex(&[bytes]), "pinned": false
    });
    frame::encode(&meta, bytes).expect("frames")
}

// Analytics.

#[tokio::test]
async fn ping_is_stored_and_answered() {
    let h = harness("ping", Limits::default());
    let r = post_json(&h, "/v1/ping", &ping_body(&install(1))).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(count(&h, "SELECT COUNT(*) FROM pings").await, 1);
}

#[tokio::test]
async fn a_ping_refusal_is_a_400_with_the_reason() {
    let h = harness("ping-bad", Limits::default());
    let mut body = ping_body(&install(1));
    body["schema"] = json!(9);
    let r = post_json(&h, "/v1/ping", &body).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.json()["error"].as_str().unwrap_or("").contains("schema"));
    assert_eq!(count(&h, "SELECT COUNT(*) FROM pings").await, 0);
}

#[tokio::test]
async fn a_release_version_must_look_like_one() {
    let h = harness("version", Limits::default());
    let mut body = ping_body(&install(1));
    body["razdor"] = json!("dev-build");
    assert_eq!(post_json(&h, "/v1/ping", &body).await.status, StatusCode::BAD_REQUEST);
    body["razdor"] = json!("1.5");
    assert_eq!(post_json(&h, "/v1/ping", &body).await.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_oversize_body_is_refused_before_it_is_read() {
    let h = harness("oversize", Limits::default());
    let big = "x".repeat(8 * 1024);
    let mut body = ping_body(&install(1));
    body["lang"] = json!(big);
    let r = post_json(&h, "/v1/ping", &body).await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn junk_that_is_not_json_is_a_400() {
    let h = harness("junk", Limits::default());
    let r = call(&router(h.st.clone()), "POST", "/v1/ping", &[], b"not json".to_vec()).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn ten_bad_requests_in_a_row_ban_the_install_silently() {
    let h = harness("bad-run", Limits::default());
    let id = install(2);
    let mut bad = ping_body(&id);
    bad["schema"] = json!(9);
    for _ in 0..9 {
        assert_eq!(post_json(&h, "/v1/ping", &bad).await.status, StatusCode::BAD_REQUEST);
    }
    // The tenth refusal brings the ban: a silent 200.
    let tenth = post_json(&h, "/v1/ping", &bad).await;
    assert_eq!(tenth.status, StatusCode::OK);
    assert_eq!(tenth.json(), json!({}));
    // A valid ping now goes nowhere, and says nothing.
    let valid = post_json(&h, "/v1/ping", &ping_body(&id)).await;
    assert_eq!(valid.status, StatusCode::OK);
    assert_eq!(count(&h, "SELECT COUNT(*) FROM pings").await, 0);
}

#[tokio::test]
async fn a_fourth_ping_in_a_day_bans_and_quarantines_the_day() {
    let h = harness("pings-day", Limits::default());
    let id = install(3);
    for _ in 0..3 {
        assert_eq!(post_json(&h, "/v1/ping", &ping_body(&id)).await.status, StatusCode::OK);
    }
    assert_eq!(count(&h, "SELECT COUNT(*) FROM pings WHERE quarantined = 0").await, 3);
    let fourth = post_json(&h, "/v1/ping", &ping_body(&id)).await;
    assert_eq!(fourth.json(), json!({}));
    // The banned day's rows are kept but not counted.
    assert_eq!(count(&h, "SELECT COUNT(*) FROM pings WHERE quarantined = 1").await, 3);
    let (strikes, active): (i64, bool) = h
        .st
        .stats
        .call(|c| {
            Ok(c.query_row("SELECT strikes, until > 0 FROM bans WHERE kind = 'install'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?)
        })
        .await
        .expect("ban row");
    assert_eq!((strikes, active), (1, true));
}

#[tokio::test]
async fn a_second_ban_lasts_seven_days_and_a_third_is_for_good() {
    let h = harness("escalate", Limits::default());
    let id = install(4);
    razdor_server::abuse::ban_install(&h.st, &id, "test", NOW).await.expect("bans");
    razdor_server::abuse::ban_install(&h.st, &id, "test", NOW).await.expect("bans");
    let until: i64 = h
        .st
        .stats
        .call(|c| Ok(c.query_row("SELECT until FROM bans WHERE kind = 'install'", [], |r| r.get(0))?))
        .await
        .expect("row");
    assert_eq!(until, NOW + 7 * 86_400);
    razdor_server::abuse::ban_install(&h.st, &id, "test", NOW).await.expect("bans");
    let until: i64 = h
        .st
        .stats
        .call(|c| Ok(c.query_row("SELECT until FROM bans WHERE kind = 'install'", [], |r| r.get(0))?))
        .await
        .expect("row");
    assert_eq!(until, i64::MAX);
}

#[tokio::test]
async fn events_skip_unknown_kinds_and_store_the_rest() {
    let h = harness("events", Limits::default());
    let id = install(5);
    let batch = json!({
        "schema": 1, "install_id": id, "razdor": "1.5.4",
        "events": [
            progress_event(NOW),
            { "at": NOW, "kind": "from_the_future", "x": 1 },
            { "at": NOW, "kind": "map_start", "map": "map01.dtm", "map_hash": "0123456789abcdef",
              "difficulty": "normal", "ai_strength": "standard", "experiments": ["six-cells"] },
            { "at": NOW, "kind": "map_end", "map": "map01.dtm", "outcome": "abandoned", "last_day": 4,
              "last_screen": "map", "last_battle": null, "seconds_in_map": 600 }
        ]
    });
    let r = post_json(&h, "/v1/events", &batch).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json(), json!({ "stored": 3, "skipped": 1, "invalid": 0 }));
    assert_eq!(count(&h, "SELECT COUNT(*) FROM events WHERE map = 'map01.dtm'").await, 2);
    assert_eq!(count(&h, "SELECT COUNT(*) FROM events WHERE outcome = 'abandoned' AND last_screen = 'map'").await, 1);
}

#[tokio::test]
async fn an_hourly_event_limit_bans_the_install() {
    let limits = Limits { events_per_hour: 5, ..Limits::default() };
    let h = harness("events-hour", limits);
    let id = install(6);
    let batch = |n: usize| json!({ "schema": 1, "install_id": id, "razdor": "1.5.4",
        "events": (0..n).map(|_| progress_event(NOW)).collect::<Vec<_>>() });
    assert_eq!(post_json(&h, "/v1/events", &batch(3)).await.json()["stored"], 3);
    assert_eq!(post_json(&h, "/v1/events", &batch(3)).await.json(), json!({}));
    assert_eq!(count(&h, "SELECT COUNT(*) FROM events").await, 3);
}

#[tokio::test]
async fn performance_is_stored_with_its_device() {
    let h = harness("perf", Limits::default());
    let body = json!({
        "schema": 1, "install_id": install(7), "razdor": "1.5.4",
        "device": { "os": "linux", "os_version": "6", "gpu": "RTX 4060 Ti", "gpu_driver": "550", "cpu_cores": 8,
                    "ram_mb": 32000, "screen_w": 1920, "screen_h": 1080, "scale_pct": 100 },
        "quality": { "preset": "high" },
        "screens": [ { "screen": "map", "median_ms": 16.0, "p99_ms": 25.0, "p999_ms": 40.0, "minutes": 2.0 } ],
        "loads": [], "memory": { "low_memory_warnings": 0 }, "profile": []
    });
    assert_eq!(post_json(&h, "/v1/perf", &body).await.status, StatusCode::OK);
    assert_eq!(count(&h, "SELECT COUNT(*) FROM perf WHERE gpu = 'RTX 4060 Ti' AND median_ms = 16.0").await, 1);
}

#[tokio::test]
async fn a_crash_and_its_save_are_kept_as_files() {
    let h = harness("crash", Limits::default());
    let id = install(8);
    let report = json!({
        "schema": 1, "install_id": id, "razdor": "1.5.4", "platform": "linux", "game": "dt", "screen": "map",
        "at": NOW, "settings": { "speed": "fast" },
        "panic_text": "thread 'main' panicked at src/rules/battle.rs:12:5:\nindex out of bounds",
        "log": ["started", "battle"], "has_save": true
    });
    let r = post_json(&h, "/v1/crash", &report).await;
    assert_eq!(r.status, StatusCode::OK);
    let crash = r.json()["id"].as_str().expect("an id").to_string();
    assert_eq!(count(&h, "SELECT COUNT(*) FROM crashes WHERE panic_loc = 'src/rules/battle.rs:12:5'").await, 1);

    let save = b"zstd-or-not, the server keeps the bytes".to_vec();
    let header = json!({ "schema": 1, "install_id": id, "crash_id": crash, "sha256": sha256_hex(&[&save]), "len": save.len() });
    let framed = frame::encode(&header, &save).expect("frames");
    let r = call(&router(h.st.clone()), "POST", "/v1/crash/save", &[], framed.clone()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(count(&h, "SELECT COUNT(*) FROM crashes WHERE save_file IS NOT NULL").await, 1);

    // Another install cannot attach a save to this crash.
    let mut other = header.clone();
    other["install_id"] = json!(install(9));
    let framed_other = frame::encode(&other, &save).expect("frames");
    assert_eq!(call(&router(h.st.clone()), "POST", "/v1/crash/save", &[], framed_other).await.status, StatusCode::NOT_FOUND);

    // A save whose sha256 does not match is refused.
    let mut wrong = header.clone();
    wrong["sha256"] = json!("0".repeat(64));
    let framed_wrong = frame::encode(&wrong, &save).expect("frames");
    assert_eq!(call(&router(h.st.clone()), "POST", "/v1/crash/save", &[], framed_wrong).await.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_battle_is_stored_checked_and_judged() {
    let h = harness("battle", Limits::default());
    let id = install(10);
    let record = b"the record's bytes".to_vec();
    let app = router(h.st.clone());
    let r = call(&app, "POST", "/v1/battles", &[], battle_frame(&id, 100, 200, &record)).await;
    assert_eq!(r.status, StatusCode::OK);
    let battle = r.json()["id"].as_str().expect("an id").to_string();

    // A record whose length differs from its header is refused.
    let mut short = record.clone();
    short.pop();
    let bad = battle_frame(&id, 300, 400, &record);
    let header_len = u32::from_le_bytes([bad[0], bad[1], bad[2], bad[3]]) as usize;
    let mut tampered = bad[..4 + header_len].to_vec();
    tampered.extend_from_slice(&short);
    assert_eq!(call(&app, "POST", "/v1/battles", &[], tampered).await.status, StatusCode::BAD_REQUEST);

    // Admin: no token, no access; the verifier's token lists and reads the battle.
    let pending = call(&app, "GET", "/v1/battles/pending", &[], vec![]).await;
    assert_eq!(pending.status, StatusCode::UNAUTHORIZED);
    let token = vec![("x-razdor-admin", TOKEN.to_string())];
    let pending = call(&app, "GET", "/v1/battles/pending", &token, vec![]).await;
    assert_eq!(pending.json().as_array().map(Vec::len), Some(1));
    let one = call(&app, "GET", &format!("/v1/battles/{battle}"), &token, vec![]).await;
    assert_eq!(one.status, StatusCode::OK);
    let (meta, rec): (Value, &[u8]) = frame::decode(&one.body).expect("framed");
    assert_eq!(meta["id"], json!(battle));
    assert_eq!(rec, &record[..]);

    let verdict = json!({ "schema": 1, "battle_id": battle, "kind": "suspicious", "reason": "turn 2 differs" });
    let r = call(&app, "POST", "/v1/verdicts", &[("x-razdor-admin", TOKEN.into())], verdict.to_string().into_bytes()).await;
    assert_eq!(r.status, StatusCode::OK);
    let pending = call(&app, "GET", "/v1/battles/pending", &token, vec![]).await;
    assert_eq!(pending.json().as_array().map(Vec::len), Some(0));
    let missing = json!({ "schema": 1, "battle_id": "nope", "kind": "ok" });
    let r = call(&app, "POST", "/v1/verdicts", &token, missing.to_string().into_bytes()).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn overlapping_battles_ban_the_install() {
    let h = harness("overlap", Limits::default());
    let id = install(11);
    let app = router(h.st.clone());
    assert_eq!(call(&app, "POST", "/v1/battles", &[], battle_frame(&id, 100, 200, b"a")).await.status, StatusCode::OK);
    let r = call(&app, "POST", "/v1/battles", &[], battle_frame(&id, 150, 250, b"b")).await;
    assert_eq!(r.json(), json!({}));
    assert_eq!(count(&h, "SELECT COUNT(*) FROM battles").await, 1);
}

#[tokio::test]
async fn install_delete_removes_the_install_and_its_files() {
    let h = harness("delete", Limits::default());
    let id = install(12);
    post_json(&h, "/v1/ping", &ping_body(&id)).await;
    let app = router(h.st.clone());
    let r = call(&app, "POST", "/v1/battles", &[], battle_frame(&id, 100, 200, b"rec")).await;
    let battle = r.json()["id"].as_str().expect("id").to_string();
    let file = h.st.root.join("battles").join(format!("{battle}.rec.zst"));
    assert!(file.exists());
    let r = post_json(&h, "/v1/install/delete", &json!({ "schema": 1, "install_id": id })).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(count(&h, "SELECT COUNT(*) FROM pings").await, 0);
    assert_eq!(count(&h, "SELECT COUNT(*) FROM battles").await, 0);
    assert!(!file.exists());
}

#[tokio::test]
async fn the_ip_flood_limit_refuses_then_bans_silently() {
    let limits = Limits { ip_per_minute: 2, ip_flood_strikes: 1, ..Limits::default() };
    let h = harness("flood", limits);
    assert_eq!(post_json(&h, "/v1/ping", &ping_body(&install(13))).await.status, StatusCode::OK);
    assert_eq!(post_json(&h, "/v1/ping", &ping_body(&install(13))).await.status, StatusCode::OK);
    // Third within the minute: a strike, and with one strike allowed, the IP is banned (silent).
    let third = post_json(&h, "/v1/ping", &ping_body(&install(14))).await;
    assert_eq!(third.json(), json!({}));
    let fourth = post_json(&h, "/v1/ping", &ping_body(&install(15))).await;
    assert_eq!(fourth.json(), json!({}));
    assert_eq!(count(&h, "SELECT COUNT(*) FROM bans WHERE kind = 'ip'").await, 1);
}

// Cloud saves.

#[tokio::test]
async fn an_account_saves_lists_fetches_and_deletes() {
    let h = harness("saves", Limits::default());
    let key = account(&h).await;
    assert_eq!(key.len(), 64);
    let app = router(h.st.clone());
    let saved = b"a save file, zstd in the real client".to_vec();
    let r = call(&app, "PUT", "/v1/saves", &key_header(&key), save_frame("dt", "map01.dtm", "manual", &saved)).await;
    assert_eq!(r.status, StatusCode::OK);
    let id = r.json()["id"].as_str().expect("id").to_string();
    assert_eq!(r.json()["used_bytes"], json!(saved.len()));

    let list = call(&app, "GET", "/v1/saves", &key_header(&key), vec![]).await.json();
    assert_eq!(list["saves"].as_array().map(Vec::len), Some(1));
    assert_eq!(list["quota_bytes"], json!(20 * 1024 * 1024));

    let got = call(&app, "GET", &format!("/v1/saves/{id}"), &key_header(&key), vec![]).await;
    assert_eq!(got.status, StatusCode::OK);
    let (meta, bytes): (Value, &[u8]) = frame::decode(&got.body).expect("framed");
    assert_eq!(meta["map"], json!("map01.dtm"));
    assert_eq!(bytes, &saved[..]);

    let del = call(&app, "DELETE", &format!("/v1/saves/{id}"), &key_header(&key), vec![]).await;
    assert_eq!(del.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", &format!("/v1/saves/{id}"), &key_header(&key), vec![]).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_map_takes_three_manual_saves_and_a_fourth_is_refused() {
    let h = harness("per-map", Limits::default());
    let key = account(&h).await;
    let app = router(h.st.clone());
    for _ in 0..3 {
        let r = call(&app, "PUT", "/v1/saves", &key_header(&key), save_frame("dt", "map02.dtm", "manual", b"s")).await;
        assert_eq!(r.status, StatusCode::OK);
    }
    let fourth = call(&app, "PUT", "/v1/saves", &key_header(&key), save_frame("dt", "map02.dtm", "manual", b"s")).await;
    assert_eq!(fourth.status, StatusCode::CONFLICT);
    // Another kind on the same map has its own room.
    let quick = call(&app, "PUT", "/v1/saves", &key_header(&key), save_frame("dt", "map02.dtm", "quick", b"s")).await;
    assert_eq!(quick.status, StatusCode::OK);
}

#[tokio::test]
async fn the_twenty_megabyte_quota_refuses_the_save_that_overflows_it() {
    let h = harness("quota", Limits::default());
    let key = account(&h).await;
    let app = router(h.st.clone());
    let big = vec![7u8; 7 * 1024 * 1024];
    for map in ["a.dtm", "b.dtm"] {
        let r = call(&app, "PUT", "/v1/saves", &key_header(&key), save_frame("dt", map, "manual", &big)).await;
        assert_eq!(r.status, StatusCode::OK);
    }
    let third = call(&app, "PUT", "/v1/saves", &key_header(&key), save_frame("dt", "c.dtm", "manual", &big)).await;
    assert_eq!(third.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(third.json()["error"].as_str().unwrap_or("").contains("Облако заполнено"));
    // Nothing was deleted to make room.
    assert_eq!(cloud_count(&h, "SELECT COUNT(*) FROM saves").await, 2);
}

#[tokio::test]
async fn a_save_whose_bytes_do_not_match_its_header_is_refused() {
    let h = harness("save-sha", Limits::default());
    let key = account(&h).await;
    let mut framed = save_frame("dt", "map01.dtm", "manual", b"abc");
    let last = framed.len() - 1;
    framed[last] ^= 1;
    let r = call(&router(h.st.clone()), "PUT", "/v1/saves", &key_header(&key), framed).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_wrong_key_is_unauthorized() {
    let h = harness("key", Limits::default());
    let _ = account(&h).await;
    let app = router(h.st.clone());
    let none = call(&app, "GET", "/v1/saves", &[], vec![]).await;
    assert_eq!(none.status, StatusCode::UNAUTHORIZED);
    let wrong = call(&app, "GET", "/v1/saves", &key_header(&"f".repeat(64)), vec![]).await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_link_code_gives_the_same_key_once_and_only_within_ten_minutes() {
    let h = harness("link", Limits::default());
    let key = account(&h).await;
    let app = router(h.st.clone());
    let issued = call(&app, "POST", "/v1/link", &key_header(&key), vec![]).await;
    assert_eq!(issued.status, StatusCode::OK);
    let code = issued.json()["code"].as_str().expect("code").to_string();
    assert_eq!(code.len(), 9);
    assert_eq!(&code[4..5], "-");
    assert_eq!(issued.json()["expires_at"], json!(NOW + 600));

    let typed = code.replace('-', "").to_lowercase();
    let body = json!({ "schema": 1, "code": typed }).to_string().into_bytes();
    let redeemed = call(&app, "POST", "/v1/link/redeem", &[], body.clone()).await;
    assert_eq!(redeemed.status, StatusCode::OK);
    assert_eq!(redeemed.json()["key"], json!(key));
    // Used: a second redeem finds nothing.
    assert_eq!(call(&app, "POST", "/v1/link/redeem", &[], body).await.status, StatusCode::BAD_REQUEST);

    // An expired code is refused.
    let again = call(&app, "POST", "/v1/link", &key_header(&key), vec![]).await.json();
    h.st.clock.set(NOW + 601);
    let late = json!({ "schema": 1, "code": again["code"] }).to_string().into_bytes();
    assert_eq!(call(&app, "POST", "/v1/link/redeem", &[], late).await.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn marks_merge_the_best_level_per_map() {
    let h = harness("marks", Limits::default());
    let key = account(&h).await;
    let app = router(h.st.clone());
    let pc = json!({ "schema": 1, "updated_at": 0, "maps": {
        "map01.dtm": { "level": "green", "date": "2026-01-02", "mark": { "won": true } } } });
    call(&app, "PUT", "/v1/marks", &key_header(&key), pc.to_string().into_bytes()).await;
    let phone = json!({ "schema": 1, "updated_at": 0, "maps": {
        "map01.dtm": { "level": "red", "date": "2026-01-09", "mark": { "cheat": true } },
        "map02.dtm": { "level": "grey", "date": "2026-01-09", "mark": {} } } });
    let merged = call(&app, "PUT", "/v1/marks", &key_header(&key), phone.to_string().into_bytes()).await.json();
    assert_eq!(merged["maps"]["map01.dtm"]["level"], json!("green"));
    assert_eq!(merged["maps"]["map02.dtm"]["level"], json!("grey"));
    let got = call(&app, "GET", "/v1/marks", &key_header(&key), vec![]).await.json();
    assert_eq!(got["maps"].as_object().map(|m| m.len()), Some(2));
}

#[tokio::test]
async fn settings_merge_per_key_and_the_newest_change_wins() {
    let h = harness("settings", Limits::default());
    let key = account(&h).await;
    let app = router(h.st.clone());
    let a = json!({ "schema": 1, "values": { "speed": { "value": "fast", "changed_at": 100 } } });
    call(&app, "PUT", "/v1/settings", &key_header(&key), a.to_string().into_bytes()).await;
    let b = json!({ "schema": 1, "values": {
        "speed": { "value": "slow", "changed_at": 50 }, "lang": { "value": "en", "changed_at": 60 } } });
    let merged = call(&app, "PUT", "/v1/settings", &key_header(&key), b.to_string().into_bytes()).await.json();
    assert_eq!(merged["values"]["speed"]["value"], json!("fast"));
    assert_eq!(merged["values"]["lang"]["value"], json!("en"));
}

#[tokio::test]
async fn hub_and_bench_are_stored_for_the_account() {
    let h = harness("hub-bench", Limits::default());
    let key = account(&h).await;
    let app = router(h.st.clone());
    let hub = json!({ "schema": 1, "updated_at": 10, "items": [ { "id": "community-pack", "kind": "map", "version": "1" } ] });
    call(&app, "PUT", "/v1/hub", &key_header(&key), hub.to_string().into_bytes()).await;
    let got = call(&app, "GET", "/v1/hub", &key_header(&key), vec![]).await.json();
    assert_eq!(got["items"].as_array().map(Vec::len), Some(1));
    let bench = json!({ "schema": 1, "razdor": "1.5.4", "at": NOW, "device": {
        "os": "linux", "os_version": "6", "gpu": "x", "gpu_driver": "1", "cpu_cores": 4, "ram_mb": 8000,
        "screen_w": 1280, "screen_h": 720, "scale_pct": 100 }, "settings": {}, "scenes": [
        { "name": "map", "median_ms": 12.0, "p99_ms": 20.0, "frames": 600 } ] });
    assert_eq!(call(&app, "POST", "/v1/bench", &key_header(&key), bench.to_string().into_bytes()).await.status, StatusCode::OK);
    let list = call(&app, "GET", "/v1/bench", &key_header(&key), vec![]).await.json();
    assert_eq!(list.as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn deleting_the_account_removes_its_files_and_its_key() {
    let h = harness("account-delete", Limits::default());
    let key = account(&h).await;
    let app = router(h.st.clone());
    let r = call(&app, "PUT", "/v1/saves", &key_header(&key), save_frame("dt", "map01.dtm", "manual", b"s")).await;
    assert_eq!(r.status, StatusCode::OK);
    let account_dir = h.st.root.join("saves");
    assert!(std::fs::read_dir(&account_dir).expect("dir").next().is_some());
    assert_eq!(call(&app, "DELETE", "/v1/account", &key_header(&key), vec![]).await.status, StatusCode::OK);
    assert_eq!(cloud_count(&h, "SELECT COUNT(*) FROM accounts").await, 0);
    assert_eq!(cloud_count(&h, "SELECT COUNT(*) FROM saves").await, 0);
    assert_eq!(std::fs::read_dir(&account_dir).expect("dir").count(), 0);
    assert_eq!(call(&app, "GET", "/v1/saves", &key_header(&key), vec![]).await.status, StatusCode::UNAUTHORIZED);
}

// Retention, the dashboard and the health check.

#[tokio::test]
async fn retention_folds_old_rows_into_daily_aggregates() {
    let h = harness("retention", Limits::default());
    let id = install(20);
    post_json(&h, "/v1/ping", &ping_body(&id)).await;
    let batch = json!({ "schema": 1, "install_id": id, "razdor": "1.5.4", "events": [
        progress_event(NOW),
        { "at": NOW, "kind": "map_start", "map": "map01.dtm", "map_hash": "0123456789abcdef",
          "difficulty": "normal", "ai_strength": "standard", "experiments": ["six-cells"] } ] });
    post_json(&h, "/v1/events", &batch).await;
    assert_eq!(count(&h, "SELECT COUNT(*) FROM pings").await, 1);

    h.st.clock.set(NOW + 200 * 86_400);
    let report = retention::run_once(&h.st).await.expect("runs");
    assert_eq!(report.pings, 1);
    assert_eq!(report.events, 2);
    assert_eq!(count(&h, "SELECT COUNT(*) FROM pings").await, 0);
    assert_eq!(count(&h, "SELECT COUNT(*) FROM events").await, 0);
    assert_eq!(count(&h, "SELECT SUM(installs) FROM daily_players").await, 1);
    assert_eq!(count(&h, "SELECT SUM(count) FROM daily_events WHERE kind = 'progress'").await, 1);
    assert_eq!(count(&h, "SELECT SUM(count) FROM daily_events WHERE kind = 'experiment' AND outcome = 'six-cells'").await, 1);
    // Running it again adds nothing.
    let again = retention::run_once(&h.st).await.expect("runs");
    assert_eq!(again.pings + again.events, 0);
    assert_eq!(count(&h, "SELECT SUM(installs) FROM daily_players").await, 1);
}

#[tokio::test]
async fn the_dashboard_needs_the_password_and_shows_the_numbers() {
    let h = harness("admin", Limits::default());
    post_json(&h, "/v1/ping", &ping_body(&install(21))).await;
    let app = router(h.st.clone());
    let form = |pw: &str| format!("password={pw}").into_bytes();
    let form_header = [("content-type", "application/x-www-form-urlencoded".to_string())];
    let wrong = call(&app, "POST", "/admin/login", &form_header, form("nope")).await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);

    let ok = call(&app, "POST", "/admin/login", &form_header, form("pw")).await;
    assert_eq!(ok.status, StatusCode::SEE_OTHER);
    let set = ok.headers.get(header::SET_COOKIE).and_then(|v| v.to_str().ok()).expect("cookie").to_string();
    let token = set.split(';').next().expect("pair").to_string();

    let page = call(&app, "GET", "/admin", &[("cookie", token.clone())], vec![]).await;
    assert_eq!(page.status, StatusCode::OK);
    let html = String::from_utf8_lossy(&page.body).to_string();
    assert!(html.contains("Players per day"));
    assert!(html.contains("1.5.4"));

    let anon = call(&app, "GET", "/admin", &[], vec![]).await;
    assert!(String::from_utf8_lossy(&anon.body).contains("type=\"password\""));

    let api = call(&app, "GET", "/admin/api/summary.json", &[("x-razdor-admin", TOKEN.into())], vec![]).await;
    assert_eq!(api.status, StatusCode::OK);
    assert_eq!(api.json()["players_per_day"][0]["value"], json!(1));
    let no_token = call(&app, "GET", "/admin/api/summary.json", &[], vec![]).await;
    assert_eq!(no_token.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_dashboard_escapes_player_text() {
    let h = harness("escape", Limits::default());
    let report = json!({
        "schema": 1, "install_id": install(22), "razdor": "1.5.4", "platform": "linux", "game": "dt", "screen": "map",
        "at": NOW, "settings": {}, "panic_text": "panicked at src/x.rs:1:1:\n<script>alert(1)</script>",
        "log": [], "has_save": false
    });
    post_json(&h, "/v1/crash", &report).await;
    let sm = summary::build(&h.st).await.expect("builds");
    assert_eq!(sm.crashes.len(), 1);
    let html = admin::render(&sm);
    assert!(!html.contains("<script>alert"));
    assert!(html.contains("&lt;script&gt;"));
}

#[tokio::test]
async fn healthz_answers_ok() {
    let h = harness("health", Limits::default());
    let r = call(&router(h.st.clone()), "GET", "/healthz", &[], vec![]).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body, b"ok");
}
