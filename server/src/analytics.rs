//! The analytics endpoints (`2026-11-22-analytics-design.md`): the daily ping, play events, the
//! performance report, crash reports and their saves, battle records, the verifier's verdicts and
//! «Удалить мои данные». Every ingest goes through [`admit`] (IP gate, JSON or frame, the install's
//! gate), then [`typed`] (decode and validate, refusals counted), then its own limits, then the store.

use std::net::IpAddr;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use razdor_wire::analytics::{
    BattleUpload, CrashReport, CrashSaveHeader, Event, EventBatch, EventKind, InstallDelete, Outcome, Ping,
    PerfReport, Verdict,
};
use razdor_wire::{frame, limits, Ack, InstallId, Validate};
use rusqlite::{params, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

use crate::abuse;
use crate::admin;
use crate::civil;
use crate::files::{self, BATTLES, CRASHES};
use crate::http::{client_ip, json, release_ok, ApiError, Peer};
use crate::state::{sha256_hex, AppState};

/// The event kinds this server knows. An unknown kind from a newer client is skipped and counted.
const KNOWN_KINDS: [&str; 8] = [
    "session_start",
    "session_end",
    "map_start",
    "progress",
    "map_end",
    "older_save_loaded",
    "settings_changed",
    "ui_counters",
];

/// A request that passed the gates: its JSON (or frame header), its install, and a framed body's bytes.
pub struct Admitted<'a> {
    pub value: Value,
    pub install: String,
    pub rest: &'a [u8],
}

pub fn ip_of(st: &AppState, headers: &HeaderMap, peer: Peer) -> IpAddr {
    client_ip(st, headers, peer.0)
}

/// The gates every analytics write passes: the IP's, the body's shape, the install's.
/// `allow_banned` is for «Удалить мои данные» only.
pub async fn admit<'a>(
    st: &AppState,
    ip: IpAddr,
    body: &'a [u8],
    framed: bool,
    allow_banned: bool,
) -> Result<Admitted<'a>, ApiError> {
    abuse::ip_gate(st, ip).await?;
    let parsed: Option<(Value, &'a [u8])> = if framed {
        frame::decode::<Value>(body).ok()
    } else {
        serde_json::from_slice::<Value>(body).ok().map(|v| (v, &body[body.len()..]))
    };
    let Some((value, rest)) = parsed else {
        let msg = if framed { "not a framed body" } else { "not JSON" };
        return Err(abuse::junk(st, ip, msg).await);
    };
    let install = value
        .get("install_id")
        .and_then(Value::as_str)
        .filter(|s| InstallId::parse(s).is_ok())
        .map(str::to_string);
    let Some(install) = install else {
        return Err(abuse::junk(st, ip, "no valid install id").await);
    };
    abuse::install_gate(st, &install, allow_banned).await?;
    Ok(Admitted { value, install, rest })
}

/// Decodes and validates. A refusal is counted against the install (and may ban it).
pub async fn typed<T>(st: &AppState, install: &str, value: Value, items: u32) -> Result<T, ApiError>
where
    T: DeserializeOwned + Validate,
{
    let parsed = serde_json::from_value::<T>(value).map_err(|e| e.to_string()).and_then(|t| {
        t.validate().map(|()| t).map_err(|e| e.to_string())
    });
    match parsed {
        Ok(t) => Ok(t),
        Err(msg) => Err(abuse::reject(st, install, items, msg).await),
    }
}

/// The release check: `X.Y.Z` (or the configured list).
pub async fn release(st: &AppState, install: &str, version: &str) -> Result<(), ApiError> {
    if release_ok(version, &st.cfg.allowed_versions) {
        Ok(())
    } else {
        Err(abuse::reject(st, install, 1, format!("not a release version: {version}")).await)
    }
}

/// The answer of a stored write: its id, when it made one.
fn stored(id: Option<String>) -> Response {
    json(&Ack { id })
}

fn name_of<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn outcome_name(o: Outcome) -> &'static str {
    match o {
        Outcome::Win => "win",
        Outcome::Loss => "loss",
        Outcome::Abandoned => "abandoned",
    }
}

/// The denormalized columns of one event: its kind, map, outcome and last screen.
fn columns(kind: &EventKind) -> (&'static str, String, String, String) {
    let none = String::new;
    match kind {
        EventKind::SessionStart { .. } => ("session_start", none(), none(), none()),
        EventKind::SessionEnd { .. } => ("session_end", none(), none(), none()),
        EventKind::MapStart { map, .. } => ("map_start", map.clone(), none(), none()),
        EventKind::Progress { .. } => ("progress", none(), none(), none()),
        EventKind::MapEnd { map, outcome, last_screen, .. } => {
            ("map_end", map.clone(), outcome_name(*outcome).to_string(), last_screen.clone())
        }
        EventKind::OlderSaveLoaded { .. } => ("older_save_loaded", none(), none(), none()),
        EventKind::SettingsChanged { .. } => ("settings_changed", none(), none(), none()),
        EventKind::UiCounters { .. } => ("ui_counters", none(), none(), none()),
    }
}

#[derive(Deserialize)]
struct RawBatch {
    schema: u32,
    razdor: String,
    events: Vec<Value>,
}

/// `POST /v1/ping`: the daily ping (3 a day per install at most).
pub async fn ping(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let adm = admit(&st, ip_of(&st, &headers, peer), &body, false, false).await?;
    let p: Ping = typed(&st, &adm.install, adm.value, 1).await?;
    release(&st, &adm.install, &p.razdor).await?;
    let now = st.clock.now();
    let day = civil::day_of(now);
    let limit = i64::from(st.cfg.limits.pings_per_day);
    let install = adm.install.clone();
    let over = st
        .stats
        .call(move |c| {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM pings WHERE install_id = ?1 AND day = ?2 AND quarantined = 0",
                params![install, day],
                |r| r.get(0),
            )?;
            if n >= limit {
                return Ok(true);
            }
            c.execute(
                "INSERT INTO pings (install_id, day, received_at, razdor, platform, game, lang)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![install, day, now, p.razdor, p.platform, p.game, p.lang],
            )?;
            Ok(false)
        })
        .await?;
    if over {
        abuse::ban_install(&st, &adm.install, "more pings in a day than a player can send", civil::day_start(day))
            .await?;
        return Err(ApiError::Silent);
    }
    abuse::tally(&st, &adm.install, 1, 0, false).await?;
    Ok(stored(None))
}

/// `POST /v1/events`: a batch of play events. Each event is decoded alone: an unknown kind is
/// skipped (and counted in the answer), an invalid one is dropped and counted against the install.
pub async fn events(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let adm = admit(&st, ip_of(&st, &headers, peer), &body, false, false).await?;
    let raw: RawBatch = match serde_json::from_value(adm.value) {
        Ok(r) => r,
        Err(e) => return Err(abuse::reject(&st, &adm.install, 1, format!("json: {e}")).await),
    };
    if raw.events.len() > limits::MAX_EVENTS_PER_BATCH {
        return Err(abuse::reject(&st, &adm.install, 1, "more events than a batch takes".into()).await);
    }
    let install_id = InstallId::parse(&adm.install).map_err(|e| ApiError::Internal(e.to_string()))?;
    let header = EventBatch { schema: raw.schema, install_id, razdor: raw.razdor.clone(), events: Vec::new() };
    if let Err(e) = header.validate() {
        return Err(abuse::reject(&st, &adm.install, 1, e.to_string()).await);
    }
    release(&st, &adm.install, &raw.razdor).await?;

    let mut events: Vec<Event> = Vec::with_capacity(raw.events.len());
    let mut skipped = 0u32;
    let mut invalid = 0u32;
    for v in raw.events {
        let unknown = v.get("kind").and_then(Value::as_str).is_some_and(|k| !KNOWN_KINDS.contains(&k));
        if unknown {
            skipped += 1;
            continue;
        }
        match serde_json::from_value::<Event>(v) {
            Ok(e) if e.validate().is_ok() => events.push(e),
            _ => invalid += 1,
        }
    }
    let stored_n = events.len() as u32;
    let now = st.clock.now();
    let limit = i64::from(st.cfg.limits.events_per_hour);
    let install = adm.install.clone();
    let room = st
        .stats
        .call(move |c| {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM events WHERE install_id = ?1 AND received_at > ?2 AND quarantined = 0",
                params![install, now - 3600],
                |r| r.get(0),
            )?;
            Ok(n + i64::from(stored_n) <= limit)
        })
        .await?;
    if !room {
        abuse::ban_install(&st, &adm.install, "more events in an hour than a player can play", now - 3600).await?;
        return Err(ApiError::Silent);
    }
    abuse::tally(&st, &adm.install, stored_n, invalid, false).await?;

    let install = adm.install.clone();
    let razdor = raw.razdor;
    st.stats
        .call(move |c| {
            let tx = c.transaction()?;
            for e in &events {
                let (kind, map, outcome, last_screen) = columns(&e.kind);
                let payload = serde_json::to_string(e).map_err(|err| ApiError::Internal(err.to_string()))?;
                tx.execute(
                    "INSERT INTO events (install_id, received_at, day, razdor, kind, map, outcome, last_screen, payload)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![install, now, civil::day_of(now), razdor, kind, map, outcome, last_screen, payload],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(json(&serde_json::json!({ "stored": stored_n, "skipped": skipped, "invalid": invalid })))
}

/// `POST /v1/perf`: the device, the settings and the frames of one session.
pub async fn perf(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let adm = admit(&st, ip_of(&st, &headers, peer), &body, false, false).await?;
    let p: PerfReport = typed(&st, &adm.install, adm.value, 1).await?;
    release(&st, &adm.install, &p.razdor).await?;
    let n = p.screens.len().max(1) as f32;
    let median = p.screens.iter().map(|s| s.median_ms).sum::<f32>() / n;
    let p99 = p.screens.iter().map(|s| s.p99_ms).sum::<f32>() / n;
    let screen = format!("{}x{}", p.device.screen_w, p.device.screen_h);
    let gpu = p.device.gpu.clone();
    let phone = p.device.phone.clone().unwrap_or_default();
    let payload = serde_json::to_string(&p).map_err(|e| ApiError::Internal(e.to_string()))?;
    let now = st.clock.now();
    let install = adm.install.clone();
    st.stats
        .call(move |c| {
            c.execute(
                "INSERT INTO perf (install_id, received_at, day, razdor, gpu, phone, screen, median_ms, p99_ms, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![install, now, civil::day_of(now), p.razdor, gpu, phone, screen, median, p99, payload],
            )?;
            Ok(())
        })
        .await?;
    abuse::tally(&st, &adm.install, 1, 0, false).await?;
    Ok(stored(None))
}

/// The panic's location and message, for grouping: `panicked at <loc>` and the line after it.
pub fn panic_key(text: &str) -> (String, String) {
    let lines: Vec<&str> = text.lines().collect();
    let (loc, msg) = match lines.iter().position(|l| l.contains("panicked at")) {
        Some(i) => {
            let loc = lines[i].split("panicked at").nth(1).unwrap_or("").trim().trim_end_matches(':');
            let msg = lines.get(i + 1).map(|s| s.trim()).unwrap_or("");
            (loc.to_string(), msg.to_string())
        }
        None => (String::new(), lines.first().map(|s| s.trim()).unwrap_or("").to_string()),
    };
    (loc.chars().take(200).collect(), msg.chars().take(300).collect())
}

/// `POST /v1/crash`: a crash report. The server keeps its JSON as a zstd file and answers the id
/// the save upload refers to.
pub async fn crash(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let adm = admit(&st, ip_of(&st, &headers, peer), &body, false, false).await?;
    let c: CrashReport = typed(&st, &adm.install, adm.value, 1).await?;
    release(&st, &adm.install, &c.razdor).await?;
    let (loc, msg) = panic_key(&c.panic_text);
    let id = files::new_id()?;
    let report = serde_json::to_vec(&c).map_err(|e| ApiError::Internal(e.to_string()))?;
    let zst = files::zstd_blocking(report).await?;
    let file = format!("{id}.json.zst");
    write_blocking(st.file(CRASHES, &file), zst).await?;
    let now = st.clock.now();
    let row = id.clone();
    let install = adm.install.clone();
    st.stats
        .call(move |conn| {
            conn.execute(
                "INSERT INTO crashes (id, install_id, received_at, day, razdor, platform, game, screen, panic_loc,
                 panic_msg, has_save, report_file) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    row,
                    install,
                    now,
                    civil::day_of(now),
                    c.razdor,
                    c.platform,
                    c.game,
                    c.screen,
                    loc,
                    msg,
                    c.has_save,
                    file
                ],
            )?;
            Ok(())
        })
        .await?;
    abuse::tally(&st, &adm.install, 1, 0, false).await?;
    Ok(stored(Some(id)))
}

/// `POST /v1/crash/save`: the save of a crash the player chose to send (framed).
pub async fn crash_save(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let adm = admit(&st, ip_of(&st, &headers, peer), &body, true, false).await?;
    let h: CrashSaveHeader = typed(&st, &adm.install, adm.value, 1).await?;
    if adm.rest.len() as u64 != h.len {
        return Err(abuse::reject(&st, &adm.install, 1, "save length differs from its header".into()).await);
    }
    if sha256_hex(&[adm.rest]) != h.sha256 {
        return Err(abuse::reject(&st, &adm.install, 1, "save sha256 differs from its header".into()).await);
    }
    let install = adm.install.clone();
    let crash = h.crash_id.clone();
    let owned = st
        .stats
        .call({
            let (install, crash) = (install.clone(), crash.clone());
            move |c| {
                let found: Option<i64> = c
                    .query_row(
                        "SELECT 1 FROM crashes WHERE id = ?1 AND install_id = ?2",
                        params![crash, install],
                        |r| r.get(0),
                    )
                    .optional()?;
                Ok(found.is_some())
            }
        })
        .await?;
    if !owned {
        return Err(ApiError::NotFound("no such crash"));
    }
    let zst = files::zstd_blocking(adm.rest.to_vec()).await?;
    let file = format!("{crash}.save.zst");
    write_blocking(st.file(CRASHES, &file), zst).await?;
    let now = st.clock.now();
    st.stats
        .call(move |c| {
            c.execute(
                "UPDATE crashes SET has_save = 1, save_file = ?1, save_at = ?2 WHERE id = ?3",
                params![file, now, crash],
            )?;
            Ok(())
        })
        .await?;
    abuse::tally(&st, &adm.install, 1, 0, false).await?;
    Ok(stored(None))
}

/// `POST /v1/battles`: a battle record (framed: the header, then the record's bytes).
pub async fn battles(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let adm = admit(&st, ip_of(&st, &headers, peer), &body, true, false).await?;
    let b: BattleUpload = typed(&st, &adm.install, adm.value, 1).await?;
    release(&st, &adm.install, &b.razdor).await?;
    if adm.rest.len() as u64 != b.record_len {
        return Err(abuse::reject(&st, &adm.install, 1, "record length differs from its header".into()).await);
    }
    if sha256_hex(&[adm.rest]) != b.record_sha256 {
        return Err(abuse::reject(&st, &adm.install, 1, "record sha256 differs from its header".into()).await);
    }
    let now = st.clock.now();
    let (started, ended) = (b.started_at as i64, b.ended_at as i64);
    let limit = i64::from(st.cfg.limits.battles_per_hour);
    let install = adm.install.clone();
    let (recent, overlaps): (i64, i64) = st
        .stats
        .call({
            let install = install.clone();
            move |c| {
                let recent = c.query_row(
                    "SELECT COUNT(*) FROM battles WHERE install_id = ?1 AND received_at > ?2 AND quarantined = 0",
                    params![install, now - 3600],
                    |r| r.get(0),
                )?;
                let overlaps = c.query_row(
                    "SELECT COUNT(*) FROM battles WHERE install_id = ?1 AND quarantined = 0
                     AND started_at < ?2 AND ended_at > ?3",
                    params![install, ended, started],
                    |r| r.get(0),
                )?;
                Ok((recent, overlaps))
            }
        })
        .await?;
    if recent + 1 > limit {
        abuse::ban_install(&st, &install, "more battles in an hour than a player can fight", now - 3600).await?;
        return Err(ApiError::Silent);
    }
    if overlaps > 0 {
        abuse::ban_install(&st, &install, "overlapping battles", now - 3600).await?;
        return Err(ApiError::Silent);
    }
    let id = files::new_id()?;
    let zst = files::zstd_blocking(adm.rest.to_vec()).await?;
    let file = format!("{id}.rec.zst");
    write_blocking(st.file(BATTLES, &file), zst).await?;
    let row = id.clone();
    let inserted = st
        .stats
        .call({
            let file = file.clone();
            move |c| {
                c.execute(
                    "INSERT INTO battles (id, install_id, received_at, day, started_at, ended_at, razdor, platform, game,
                     map, map_hash, mod_hash, rules, experiments, tags, winner, turns, player_losses, foe_losses,
                     record_sha256, record_len, file)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19,
                     ?20, ?21, ?22)",
                    params![
                        row,
                        install,
                        now,
                        civil::day_of(now),
                        started,
                        ended,
                        b.razdor,
                        b.platform,
                        b.game,
                        b.map,
                        b.map_hash,
                        b.mod_hash,
                        b.rules as i64,
                        serde_json::to_string(&b.experiments).unwrap_or_default(),
                        serde_json::to_string(&b.tags).unwrap_or_default(),
                        name_of(&b.outcome.winner),
                        b.outcome.turns,
                        b.outcome.player_losses,
                        b.outcome.foe_losses,
                        b.record_sha256,
                        b.record_len as i64,
                        file
                    ],
                )?;
                Ok(())
            }
        })
        .await;
    if let Err(e) = inserted {
        files::remove_quiet(&st.file(BATTLES, &file));
        return Err(e);
    }
    abuse::tally(&st, &adm.install, 1, 0, false).await?;
    Ok(stored(Some(id)))
}

/// `POST /v1/verdicts` (admin token): the verifier's verdict on a battle.
pub async fn verdict(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    if !admin::token_ok(&st, &headers) {
        return Err(ApiError::Unauthorized("admin token"));
    }
    let v: Verdict = serde_json::from_slice(&body).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    v.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let kind = name_of(&v.kind);
    let reason = v.reason.clone();
    let battle = v.battle_id.clone();
    let n = st
        .stats
        .call(move |c| {
            Ok(c.execute(
                "UPDATE battles SET verdict = ?1, verdict_reason = ?2 WHERE id = ?3",
                params![kind, reason, battle],
            )?)
        })
        .await?;
    if n == 0 {
        return Err(ApiError::NotFound("no such battle"));
    }
    Ok(stored(None))
}

fn battle_meta(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "game": row.get::<_, String>(1)?,
        "map": row.get::<_, String>(2)?,
        "map_hash": row.get::<_, String>(3)?,
        "mod_hash": row.get::<_, Option<String>>(4)?,
        "rules": row.get::<_, i64>(5)? as u64,
        "experiments": serde_json::from_str::<Value>(&row.get::<_, String>(6)?).unwrap_or(Value::Null),
        "tags": serde_json::from_str::<Value>(&row.get::<_, String>(7)?).unwrap_or(Value::Null),
        "winner": row.get::<_, String>(8)?,
        "turns": row.get::<_, u32>(9)?,
        "player_losses": row.get::<_, u32>(10)?,
        "foe_losses": row.get::<_, u32>(11)?,
        "started_at": row.get::<_, i64>(12)?,
        "ended_at": row.get::<_, i64>(13)?,
        "received_at": row.get::<_, i64>(14)?,
    }))
}

const BATTLE_COLUMNS: &str = "id, game, map, map_hash, mod_hash, rules, experiments, tags, winner, turns,
     player_losses, foe_losses, started_at, ended_at, received_at";

/// `GET /v1/battles/pending` (admin token): the battles the verifier has not judged yet.
pub async fn battles_pending(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if !admin::token_ok(&st, &headers) {
        return Err(ApiError::Unauthorized("admin token"));
    }
    let rows = st
        .stats
        .call(|c| {
            let mut s = c.prepare(&format!(
                "SELECT {BATTLE_COLUMNS} FROM battles WHERE verdict IS NULL AND quarantined = 0
                 ORDER BY received_at LIMIT 200"
            ))?;
            let rows = s.query_map([], battle_meta)?.collect::<Result<Vec<Value>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(json(&rows))
}

/// `GET /v1/battles/{id}` (admin token): the battle's metadata and its record, framed.
pub async fn battle_get(
    State(st): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if !admin::token_ok(&st, &headers) {
        return Err(ApiError::Unauthorized("admin token"));
    }
    let (meta, file) = st
        .stats
        .call({
            let id = id.clone();
            move |c| {
                let found = c
                    .query_row(
                        &format!("SELECT {BATTLE_COLUMNS}, file FROM battles WHERE id = ?1"),
                        params![id],
                        |r| Ok((battle_meta(r)?, r.get::<_, String>(15)?)),
                    )
                    .optional()?;
                Ok(found)
            }
        })
        .await?
        .ok_or(ApiError::NotFound("no such battle"))?;
    let zst = tokio::fs::read(st.file(BATTLES, &file))
        .await
        .map_err(|e| ApiError::Internal(format!("battle file: {e}")))?;
    let record = tokio::task::spawn_blocking(move || files::unzstd(&zst))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(|e| ApiError::Internal(format!("battle file: {e}")))?;
    let framed = frame::encode(&meta, &record).map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok((StatusCode::OK, [(header::CONTENT_TYPE, "application/octet-stream")], framed).into_response())
}

/// `POST /v1/install/delete`: «Удалить мои данные». Everything of the install id goes, its files too.
/// The ban record stays (the strikes of an id are not reset by deleting it).
pub async fn install_delete(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let adm = admit(&st, ip_of(&st, &headers, peer), &body, false, true).await?;
    let d: InstallDelete = typed(&st, &adm.install, adm.value, 1).await?;
    let install = d.install_id.as_str().to_string();
    let root = st.root.clone();
    st.stats
        .call(move |c| {
            let tx = c.transaction()?;
            let mut doomed: Vec<(&'static str, String)> = Vec::new();
            {
                let mut s = tx.prepare("SELECT file FROM battles WHERE install_id = ?1")?;
                for f in s.query_map(params![install], |r| r.get::<_, String>(0))? {
                    doomed.push((BATTLES, f?));
                }
            }
            {
                let mut s = tx.prepare("SELECT report_file, save_file FROM crashes WHERE install_id = ?1")?;
                for row in s.query_map(params![install], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                })? {
                    let (report, save) = row?;
                    doomed.push((CRASHES, report));
                    if let Some(save) = save {
                        doomed.push((CRASHES, save));
                    }
                }
            }
            for table in ["pings", "events", "perf", "battles", "crashes"] {
                tx.execute(&format!("DELETE FROM {table} WHERE install_id = ?1"), params![install])?;
            }
            tx.commit()?;
            for (folder, name) in doomed {
                files::remove_quiet(&files::path(&root, folder, &name));
            }
            Ok(())
        })
        .await?;
    Ok(stored(None))
}

/// Writes a file on the blocking pool.
pub async fn write_blocking(path: std::path::PathBuf, bytes: Vec<u8>) -> Result<(), ApiError> {
    tokio::task::spawn_blocking(move || files::write_atomic(&path, &bytes))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(|e| ApiError::Internal(format!("write: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_grouping_takes_location_and_message() {
        let text = "thread 'main' panicked at src/rules/battle.rs:12:5:\nindex out of bounds\nstack...";
        let (loc, msg) = panic_key(text);
        assert_eq!(loc, "src/rules/battle.rs:12:5");
        assert_eq!(msg, "index out of bounds");
        let (loc, msg) = panic_key("plain text");
        assert_eq!(loc, "");
        assert_eq!(msg, "plain text");
    }
}
