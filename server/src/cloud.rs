//! The cloud endpoints (`2026-11-22-cloud-saves-design.md`): the account (a random key, no login),
//! linking a second device with a code, the saves (per-map limits and the 20 MB quota, refusals
//! never delete), the completed-game marks (merged: the best level per map wins), the synced
//! settings (the newest change per key wins), the installed Hub items and the bench reports.
//! Keyed requests carry the account key in `x-razdor-key`; the server keeps only its SHA-256.


use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use razdor_wire::cloud::{
    per_map_limit, AccountCreated, AccountNew, BenchReport, HubList, LinkIssued, LinkRedeem, LinkRedeemed,
    MarkLevel, Marks, SaveEntry, SaveKind, SaveList, SaveMeta, SavePut, SyncedSettings,
};
use razdor_wire::ids::{AccountKey, LinkCode};
use razdor_wire::{frame, limits, paths, Ack, Validate, SCHEMA};
use rusqlite::{params, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::abuse;
use crate::analytics::ip_of;
use crate::civil;
use crate::files::{self, SAVES};
use crate::http::{json, ApiError, Peer};
use crate::state::{sha256_hex, AppState};

/// The saves a device may list per account: the list is the whole account (no paging needed).
const BENCH_KEPT: i64 = 50;

/// A cloud request is never answered with the silent 200 that a banned analytics sender gets: the
/// client would forget its key while the account stays. A ban is a 429 with Retry-After instead.
pub fn cloud_refusal(e: ApiError) -> ApiError {
    match e {
        ApiError::Silent => ApiError::Limited,
        other => other,
    }
}

/// The IP's gate, for every cloud request (a flood is refused for all of them). A request whose key
/// is a stored account skips the IP ban: a shared (CGNAT) address must not lock its own players out.
async fn gate(st: &AppState, headers: &HeaderMap, peer: Peer) -> Result<(), ApiError> {
    if let Some(key_hash) = key_hash_of(headers) {
        if find_account(st, key_hash).await?.is_some() {
            return Ok(());
        }
    }
    abuse::ip_gate(st, ip_of(st, headers, peer)).await.map_err(cloud_refusal)
}

/// The SHA-256 of the presented account key, when the header is a well-formed key.
fn key_hash_of(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(paths::KEY_HEADER).and_then(|v| v.to_str().ok())?;
    let key = AccountKey::parse(raw).ok()?;
    Some(sha256_hex(&[key.as_str().as_bytes()]))
}

async fn find_account(st: &AppState, key_hash: String) -> Result<Option<String>, ApiError> {
    st.cloud
        .call(move |c| {
            Ok(c.query_row("SELECT id FROM accounts WHERE key_hash = ?1", params![key_hash], |r| r.get::<_, String>(0))
                .optional()?)
        })
        .await
}

fn parse_valid<T: DeserializeOwned + Validate>(body: &[u8]) -> Result<T, ApiError> {
    let t: T = serde_json::from_slice(body).map_err(|e| ApiError::BadRequest(format!("json: {e}")))?;
    t.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(t)
}

/// The account of a keyed request: its key's SHA-256 must be a stored account.
async fn account_of(st: &AppState, headers: &HeaderMap) -> Result<String, ApiError> {
    let raw = headers
        .get(paths::KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::Unauthorized("no account key"))?;
    let key = AccountKey::parse(raw).map_err(|_| ApiError::Unauthorized("bad account key"))?;
    let key_hash = sha256_hex(&[key.as_str().as_bytes()]);
    // The rate window is opened only for a stored account: a made-up key costs no memory.
    let id = find_account(st, key_hash.clone()).await?.ok_or(ApiError::Unauthorized("no such account"))?;
    abuse::account_gate(st, &key_hash)?;
    touch_account(st, &id).await?;
    Ok(id)
}

/// Marks the account used now, for the expiry. One write an hour per account at most: the UPDATE
/// only moves a `last_active` that is over an hour old.
async fn touch_account(st: &AppState, id: &str) -> Result<(), ApiError> {
    let now = st.clock.now();
    let id = id.to_string();
    st.cloud
        .call(move |c| {
            c.execute(
                "UPDATE accounts SET last_active = ?1 WHERE id = ?2 AND last_active < ?1 - 3600",
                params![now, id],
            )?;
            Ok(())
        })
        .await
}

async fn count_op(st: &AppState, op: &'static str) -> Result<(), ApiError> {
    let day = civil::day_of(st.clock.now());
    st.stats
        .call(move |c| {
            c.execute(
                "INSERT INTO daily_cloud (day, op, count) VALUES (?1, ?2, 1)
                 ON CONFLICT (day, op) DO UPDATE SET count = count + 1",
                params![day, op],
            )?;
            Ok(())
        })
        .await
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn kind_name(kind: SaveKind) -> &'static str {
    match kind {
        SaveKind::Manual => "manual",
        SaveKind::Quick => "quick",
        SaveKind::Session => "session",
    }
}

fn kind_from(s: &str) -> rusqlite::Result<SaveKind> {
    serde_json::from_value(Value::String(s.to_string()))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
}

const SAVE_COLUMNS: &str = "id, game, map, kind, device, name, saved_at, razdor, size, sha256, pinned";

/// A stored save's id and metadata, read in the order of [`SAVE_COLUMNS`].
fn meta_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, SaveMeta)> {
    let kind: String = r.get(3)?;
    let meta = SaveMeta {
        schema: SCHEMA,
        game: r.get(1)?,
        map: r.get(2)?,
        kind: kind_from(&kind)?,
        device: r.get(4)?,
        name: r.get(5)?,
        saved_at: r.get::<_, i64>(6)? as u64,
        razdor: r.get(7)?,
        size: r.get::<_, i64>(8)? as u64,
        sha256: r.get(9)?,
        pinned: r.get::<_, i64>(10)? != 0,
    };
    Ok((r.get(0)?, meta))
}

/// `POST /v1/account`: a new account. The answer is its key: the only secret, kept by the game.
pub async fn new_account(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let ip = ip_of(&st, &headers, peer);
    abuse::ip_gate(&st, ip).await.map_err(cloud_refusal)?;
    let _req: AccountNew = match serde_json::from_slice::<AccountNew>(&body).map_err(|e| e.to_string()).and_then(|r| {
        r.validate().map(|()| r).map_err(|e| e.to_string())
    }) {
        Ok(r) => r,
        Err(msg) => return Err(cloud_refusal(abuse::junk(&st, ip, &msg).await)),
    };
    // Accounts per IP per day (CGNAT-tolerant: a bucket of many players, not one player).
    abuse::account_create_gate(&st, ip).await?;
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| ApiError::Internal(format!("random: {e}")))?;
    let key_text = files::hex(&raw);
    let key = AccountKey::parse(&key_text).map_err(|e| ApiError::Internal(e.to_string()))?;
    let id = files::new_id()?;
    let key_hash = sha256_hex(&[key_text.as_bytes()]);
    let now = st.clock.now();
    st.cloud
        .call(move |c| {
            c.execute(
                "INSERT INTO accounts (id, key_hash, created_at, last_active) VALUES (?1, ?2, ?3, ?3)",
                params![id, key_hash, now],
            )?;
            Ok(())
        })
        .await?;
    count_op(&st, "account_new").await?;
    Ok(json(&AccountCreated { key }))
}

/// `DELETE /v1/account`: deletes the account and every file of it.
pub async fn delete_account(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let id = account_of(&st, &headers).await?;
    remove_account(&st, &id).await?;
    count_op(&st, "account_delete").await?;
    Ok(json(&Ack::confirmed()))
}

/// Deletes an account: its row (the links, saves, marks and the rest go with it) and its save files.
/// Used by the owner's delete and by the retention job's expiry.
pub async fn remove_account(st: &AppState, id: &str) -> Result<(), ApiError> {
    let dir = st.root.join(SAVES).join(id);
    let id = id.to_string();
    st.cloud
        .call(move |c| {
            c.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await?;
    tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?
    .map_err(|e| ApiError::Internal(format!("account files: {e}")))
}

/// Deletes the link codes past their expiry (their keys go with them). Run every minute.
pub async fn sweep_links(st: &AppState) -> Result<usize, ApiError> {
    let now = st.clock.now();
    st.cloud.call(move |c| Ok(c.execute("DELETE FROM links WHERE expires_at <= ?1", params![now])?)).await
}

/// `POST /v1/link`: a code another device types within ten minutes, once.
pub async fn link_issue(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let raw_key = headers
        .get(paths::KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::Unauthorized("no account key"))?
        .to_string();
    let id = account_of(&st, &headers).await?;
    let now = st.clock.now();
    let expires_at = now + limits::LINK_TTL_SECS as i64;
    let code = new_link_code()?;
    let stored_code = code.clone();
    st.cloud
        .call(move |c| {
            c.execute(
                "INSERT INTO links (code, account_id, key, expires_at) VALUES (?1, ?2, ?3, ?4)",
                params![stored_code, id, raw_key, expires_at],
            )?;
            Ok(())
        })
        .await?;
    count_op(&st, "link_issue").await?;
    let code = LinkCode::parse(&code).map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(json(&LinkIssued { code, expires_at: expires_at as u64 }))
}

/// Eight characters of the link alphabet, as `XXXX-XXXX`. Bytes outside the largest multiple of 31
/// are redrawn, so every character is equally likely.
fn new_link_code() -> Result<String, ApiError> {
    let alphabet = limits::LINK_ALPHABET.as_bytes();
    let mut chars: Vec<char> = Vec::with_capacity(8);
    while chars.len() < 8 {
        let mut b = [0u8; 16];
        getrandom::fill(&mut b).map_err(|e| ApiError::Internal(format!("random: {e}")))?;
        for x in b {
            if (x as usize) < 31 * 8 && chars.len() < 8 {
                chars.push(alphabet[(x as usize) % 31] as char);
            }
        }
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[4..].iter().collect();
    Ok(format!("{head}-{tail}"))
}

/// `POST /v1/link/redeem`: the second device gives the code and gets the account's key. A wrong or
/// used code counts as junk from its IP.
pub async fn link_redeem(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let ip = ip_of(&st, &headers, peer);
    abuse::ip_gate(&st, ip).await.map_err(cloud_refusal)?;
    // Every attempt, right or wrong, is rate-limited per IP (429): a wrong code is no junk strike.
    abuse::redeem_gate(&st, ip)?;
    let req: LinkRedeem = match serde_json::from_slice::<LinkRedeem>(&body)
        .map_err(|e| e.to_string())
        .and_then(|r| r.validate().map(|()| r).map_err(|e| e.to_string()))
    {
        Ok(r) => r,
        Err(msg) => return Err(cloud_refusal(abuse::junk(&st, ip, &msg).await)),
    };
    let code = LinkCode::parse(&req.code).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let now = st.clock.now();
    let found = st
        .cloud
        .call(move |c| {
            let tx = c.transaction()?;
            let row: Option<(String, i64)> = tx
                .query_row("SELECT key, expires_at FROM links WHERE code = ?1", params![code.as_str()], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?;
            // A code is used up by its first redeem, and an expired one is dropped with its key.
            let key = match row {
                Some((key, expires_at)) => {
                    tx.execute("DELETE FROM links WHERE code = ?1", params![code.as_str()])?;
                    (expires_at > now).then_some(key)
                }
                None => None,
            };
            tx.commit()?;
            Ok(key)
        })
        .await?;
    let Some(key) = found else {
        return Err(ApiError::BadRequest("no such code, or it has expired".into()));
    };
    let key = AccountKey::parse(&key).map_err(|e| ApiError::Internal(e.to_string()))?;
    count_op(&st, "link_redeem").await?;
    Ok(json(&LinkRedeemed { key }))
}

/// `GET /v1/saves`: every save of the account and the quota's use.
pub async fn list_saves(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let id = account_of(&st, &headers).await?;
    let (saves, used) = st
        .cloud
        .call(move |c| {
            let mut s = c.prepare(&format!("SELECT {SAVE_COLUMNS} FROM saves WHERE account_id = ?1 ORDER BY saved_at"))?;
            let saves = s
                .query_map(params![id], meta_of)?
                .map(|r| r.map(|(id, meta)| SaveEntry { id, meta }))
                .collect::<Result<Vec<_>, _>>()?;
            let used: i64 = c.query_row(
                "SELECT COALESCE(SUM(size), 0) FROM saves WHERE account_id = ?1",
                params![id],
                |r| r.get(0),
            )?;
            Ok((saves, used as u64))
        })
        .await?;
    Ok(json(&SaveList { saves, used_bytes: used, quota_bytes: limits::CLOUD_BYTES_PER_ACCOUNT }))
}

/// `PUT /v1/saves`: a save (framed: its metadata, then the file). Refused over the map's limit or the
/// quota; nothing is deleted to make room.
pub async fn put_save(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let id = account_of(&st, &headers).await?;
    let (meta, bytes): (SaveMeta, &[u8]) = frame::decode(&body).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    meta.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if bytes.len() as u64 != meta.size {
        return Err(ApiError::BadRequest("save length differs from its header".into()));
    }
    if sha256_hex(&[bytes]) != meta.sha256 {
        return Err(ApiError::BadRequest("save sha256 differs from its header".into()));
    }
    if meta.size == 0 {
        return Err(ApiError::BadRequest("an empty save is refused".into()));
    }
    let save_id = files::new_id()?;
    let bytes = bytes.to_vec();
    let rows_cap = st.cfg.limits.saves_per_account as i64;
    let limit = per_map_limit(meta.kind) as i64;
    let kind = kind_name(meta.kind);
    let quota = limits::CLOUD_BYTES_PER_ACCOUNT as i64;
    let size = meta.size as i64;
    let now = st.clock.now();
    let root = st.root.clone();
    let row_id = save_id.clone();
    let account = id.clone();
    let used = st
        .cloud
        .call(move |c| {
            let tx = c.transaction()?;
            let on_map: i64 = tx.query_row(
                "SELECT COUNT(*) FROM saves WHERE account_id = ?1 AND game = ?2 AND map = ?3 AND kind = ?4",
                params![account, meta.game, meta.map, kind],
                |r| r.get(0),
            )?;
            if on_map >= limit {
                return Err(ApiError::Conflict(format!(
                    "На этой карте уже максимум сохранений этого вида ({limit}). Удалите старое."
                )));
            }
            // The account's row cap: the same refusal as a full cloud, so the client shows it alike.
            let rows: i64 = tx.query_row("SELECT COUNT(*) FROM saves WHERE account_id = ?1", params![account], |r| r.get(0))?;
            if rows >= rows_cap {
                return Err(ApiError::TooLarge("Облако заполнено, удалите старые сохранения".into()));
            }
            let used: i64 = tx.query_row(
                "SELECT COALESCE(SUM(size), 0) FROM saves WHERE account_id = ?1",
                params![account],
                |r| r.get(0),
            )?;
            if used + size > quota {
                return Err(ApiError::TooLarge("Облако заполнено, удалите старые сохранения".into()));
            }
            let path = root.join(SAVES).join(&account).join(format!("{row_id}.bin"));
            files::write_atomic(&path, &bytes).map_err(|e| ApiError::Internal(format!("save file: {e}")))?;
            let inserted = tx.execute(
                "INSERT INTO saves (id, account_id, game, map, kind, device, name, saved_at, razdor, size, sha256,
                 pinned, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    row_id,
                    account,
                    meta.game,
                    meta.map,
                    kind,
                    meta.device,
                    meta.name,
                    meta.saved_at as i64,
                    meta.razdor,
                    size,
                    meta.sha256,
                    meta.pinned,
                    now
                ],
            );
            if let Err(e) = inserted.and_then(|_| tx.commit()) {
                files::remove_quiet(&path);
                return Err(e.into());
            }
            Ok((used + size) as u64)
        })
        .await?;
    count_op(&st, "save_put").await?;
    Ok(json(&SavePut { id: save_id, used_bytes: used }))
}

/// `GET /v1/saves/{id}`: one save, framed (its metadata, then the file).
pub async fn get_save(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    Path(save): Path<String>,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let account = account_of(&st, &headers).await?;
    if !valid_id(&save) {
        return Err(ApiError::NotFound("no such save"));
    }
    let found = st
        .cloud
        .call({
            let (save, account) = (save.clone(), account.clone());
            move |c| {
                Ok(c.query_row(
                    &format!("SELECT {SAVE_COLUMNS} FROM saves WHERE id = ?1 AND account_id = ?2"),
                    params![save, account],
                    meta_of,
                )
                .optional()?)
            }
        })
        .await?;
    let (_, meta) = found.ok_or(ApiError::NotFound("no such save"))?;
    let path = st.root.join(SAVES).join(&account).join(format!("{save}.bin"));
    let bytes = tokio::fs::read(&path).await.map_err(|e| ApiError::Internal(format!("save file: {e}")))?;
    let framed = frame::encode(&meta, &bytes).map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok((StatusCode::OK, [(header::CONTENT_TYPE, "application/octet-stream")], framed).into_response())
}

/// `DELETE /v1/saves/{id}`: removes one save and its file.
pub async fn delete_save(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    Path(save): Path<String>,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let account = account_of(&st, &headers).await?;
    if !valid_id(&save) {
        return Err(ApiError::NotFound("no such save"));
    }
    let root = st.root.clone();
    st.cloud
        .call(move |c| {
            let tx = c.transaction()?;
            let n = tx.execute("DELETE FROM saves WHERE id = ?1 AND account_id = ?2", params![save, account])?;
            if n == 0 {
                return Err(ApiError::NotFound("no such save"));
            }
            tx.commit()?;
            files::remove_quiet(&root.join(SAVES).join(&account).join(format!("{save}.bin")));
            Ok(())
        })
        .await?;
    count_op(&st, "save_delete").await?;
    Ok(json(&Ack::confirmed()))
}

/// The rank of a mark's level: the game's own order, Green (best) first.
fn rank(level: MarkLevel) -> u8 {
    match level {
        MarkLevel::Green => 0,
        MarkLevel::Fixes => 1,
        MarkLevel::Grey => 2,
        MarkLevel::Yellow => 3,
        MarkLevel::Red => 4,
    }
}

/// Merges two marks documents: per map, the better level wins (a tie: the later date). A map
/// completed on any device stays completed.
pub fn merge_marks(existing: Option<Marks>, incoming: Marks, now: u64) -> Marks {
    let mut maps = existing.map(|m| m.maps).unwrap_or_default();
    for (name, rec) in incoming.maps {
        let better = match maps.get(&name) {
            None => true,
            Some(old) => {
                rank(rec.level) < rank(old.level) || (rank(rec.level) == rank(old.level) && rec.date > old.date)
            }
        };
        if better {
            maps.insert(name, rec);
        }
    }
    Marks { schema: SCHEMA, updated_at: now, maps }
}

/// Merges synced settings: per key, the newest change wins.
pub fn merge_settings(existing: Option<SyncedSettings>, incoming: SyncedSettings) -> SyncedSettings {
    let mut values = existing.map(|s| s.values).unwrap_or_default();
    for (name, v) in incoming.values {
        let newer = values.get(&name).is_none_or(|old| v.changed_at > old.changed_at);
        if newer {
            values.insert(name, v);
        }
    }
    SyncedSettings { schema: SCHEMA, values }
}

/// `GET /v1/marks`.
pub async fn get_marks(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let account = account_of(&st, &headers).await?;
    let doc = st
        .cloud
        .call(move |c| load_doc::<Marks>(c, "marks", &account))
        .await?
        .unwrap_or(Marks { schema: SCHEMA, updated_at: 0, maps: Default::default() });
    Ok(json(&doc))
}

/// `PUT /v1/marks`: merged into the account's marks; answers the merged document.
pub async fn put_marks(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let account = account_of(&st, &headers).await?;
    let incoming: Marks = parse_valid(&body)?;
    let now = st.clock.now() as u64;
    let merged = st
        .cloud
        .call(move |c| {
            let existing = load_doc::<Marks>(c, "marks", &account)?;
            let merged = merge_marks(existing, incoming, now);
            save_doc(c, "marks", &account, &merged)?;
            Ok(merged)
        })
        .await?;
    count_op(&st, "marks").await?;
    Ok(json(&merged))
}

/// `GET /v1/settings`.
pub async fn get_settings(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let account = account_of(&st, &headers).await?;
    let doc = st
        .cloud
        .call(move |c| load_doc::<SyncedSettings>(c, "settings", &account))
        .await?
        .unwrap_or(SyncedSettings { schema: SCHEMA, values: Default::default() });
    Ok(json(&doc))
}

/// `PUT /v1/settings`: merged per key (the newest change wins); answers the merged settings.
pub async fn put_settings(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let account = account_of(&st, &headers).await?;
    let incoming: SyncedSettings = parse_valid(&body)?;
    let merged = st
        .cloud
        .call(move |c| {
            let existing = load_doc::<SyncedSettings>(c, "settings", &account)?;
            let merged = merge_settings(existing, incoming);
            save_doc(c, "settings", &account, &merged)?;
            Ok(merged)
        })
        .await?;
    count_op(&st, "settings").await?;
    Ok(json(&merged))
}

/// `GET /v1/hub`: the Hub items installed on the account's devices.
pub async fn get_hub(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let account = account_of(&st, &headers).await?;
    let doc = st
        .cloud
        .call(move |c| load_doc::<HubList>(c, "hub", &account))
        .await?
        .unwrap_or(HubList { schema: SCHEMA, updated_at: 0, items: Vec::new() });
    Ok(json(&doc))
}

/// `PUT /v1/hub`: replaces the list when it is not older than the stored one.
pub async fn put_hub(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let account = account_of(&st, &headers).await?;
    let incoming: HubList = parse_valid(&body)?;
    let doc = st
        .cloud
        .call(move |c| {
            let existing = load_doc::<HubList>(c, "hub", &account)?;
            let doc = match existing {
                Some(old) if old.updated_at > incoming.updated_at => old,
                _ => {
                    save_doc(c, "hub", &account, &incoming)?;
                    incoming
                }
            };
            Ok(doc)
        })
        .await?;
    count_op(&st, "hub").await?;
    Ok(json(&doc))
}

/// `POST /v1/bench`: a performance test's report. The last fifty are kept.
pub async fn post_bench(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let account = account_of(&st, &headers).await?;
    let report: BenchReport = parse_valid(&body)?;
    let json_text = serde_json::to_string(&report).map_err(|e| ApiError::Internal(e.to_string()))?;
    let at = report.at as i64;
    st.cloud
        .call(move |c| {
            let tx = c.transaction()?;
            tx.execute(
                "INSERT INTO bench (account_id, at, json) VALUES (?1, ?2, ?3)",
                params![account, at, json_text],
            )?;
            tx.execute(
                "DELETE FROM bench WHERE account_id = ?1 AND id NOT IN
                 (SELECT id FROM bench WHERE account_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2)",
                params![account, BENCH_KEPT],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    count_op(&st, "bench").await?;
    Ok(json(&Ack::confirmed()))
}

/// `GET /v1/bench`: the account's last reports, newest first.
pub async fn get_bench(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    gate(&st, &headers, peer).await?;
    let account = account_of(&st, &headers).await?;
    let reports = st
        .cloud
        .call(move |c| {
            let mut s = c.prepare("SELECT json FROM bench WHERE account_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2")?;
            let texts = s
                .query_map(params![account, BENCH_KEPT], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(texts.iter().filter_map(|t| serde_json::from_str::<Value>(t).ok()).collect::<Vec<Value>>())
        })
        .await?;
    Ok(json(&reports))
}

/// Reads an account's stored document (`marks`, `settings` or `hub`) as JSON.
fn load_doc<T: DeserializeOwned>(c: &rusqlite::Connection, table: &str, account: &str) -> Result<Option<T>, ApiError> {
    let text: Option<String> = c
        .query_row(&format!("SELECT json FROM {table} WHERE account_id = ?1"), params![account], |r| r.get(0))
        .optional()?;
    match text {
        None => Ok(None),
        Some(t) => serde_json::from_str(&t).map(Some).map_err(|e| ApiError::Internal(format!("stored {table}: {e}"))),
    }
}

fn save_doc<T: serde::Serialize>(c: &rusqlite::Connection, table: &str, account: &str, doc: &T) -> Result<(), ApiError> {
    let text = serde_json::to_string(doc).map_err(|e| ApiError::Internal(e.to_string()))?;
    c.execute(
        &format!(
            "INSERT INTO {table} (account_id, json) VALUES (?1, ?2)
             ON CONFLICT (account_id) DO UPDATE SET json = excluded.json"
        ),
        params![account, text],
    )?;
    Ok(())
}
