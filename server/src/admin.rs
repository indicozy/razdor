//! The admin dashboard: a password (argon2) for the browser, a cookie session of 12 hours, and the
//! admin token (its SHA-256 in the config) for the API. Server-rendered HTML with inline SVG charts;
//! every value from a player (a panic text, a GPU name, a map name) is escaped.

use std::fmt::Write as _;

use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;

use crate::abuse;
use crate::analytics::ip_of;
use crate::civil::day_of;
use crate::http::{json, ApiError, Peer};
use crate::state::{sha256_hex, AppState};
use crate::summary::{self, Named, Summary};

pub const COOKIE: &str = "razdor_admin";
const SESSION_SECS: i64 = 12 * 3600;

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The admin token, from `x-razdor-admin` (the verifier and the digest).
pub fn token_ok(st: &AppState, headers: &HeaderMap) -> bool {
    let Some(expected) = st.cfg.admin_token_sha256.as_deref() else {
        return false;
    };
    let Some(given) = headers.get(razdor_wire::paths::ADMIN_HEADER).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    constant_eq(sha256_hex(&[given.as_bytes()]).as_bytes(), expected.as_bytes())
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == COOKIE).then(|| value.to_string())
    })
}

/// The browser's session cookie, if it is a live session.
pub fn session_ok(st: &AppState, headers: &HeaderMap) -> bool {
    let Some(token) = cookie_token(headers) else {
        return false;
    };
    let now = st.clock.now();
    let key = sha256_hex(&[token.as_bytes()]);
    st.sessions.lock().unwrap_or_else(|e| e.into_inner()).get(&key).is_some_and(|&exp| exp > now)
}

fn verify_password(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
        .unwrap_or(false)
}

/// `razdor-server hash-password`: the argon2 hash for the config's `admin_password_hash`.
pub fn hash_password(password: &str) -> Result<String, String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| format!("random: {e}"))?;
    let salt = SaltString::encode_b64(&b).map_err(|e| e.to_string())?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

fn page(status: StatusCode, title: &str, body: &str) -> Response {
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"robots\" content=\"noindex\">\
         <title>{title}</title><style>{CSS}</style></head><body>{body}</body></html>"
    );
    (status, Html(html)).into_response()
}

const CSS: &str = "body{font:14px/1.45 system-ui,sans-serif;margin:0 auto;max-width:1100px;padding:16px;color:#1d1f23;background:#fafaf8}\
h1{font-size:20px}h2{font-size:16px;margin-top:28px;border-bottom:1px solid #ddd;padding-bottom:4px}\
table{border-collapse:collapse;width:100%;margin:6px 0}td,th{text-align:left;padding:4px 6px;border-bottom:1px solid #e5e5e0;vertical-align:top}\
th{font-weight:600;background:#f0f0ea}td.n,th.n{text-align:right;font-variant-numeric:tabular-nums}\
.chart{width:100%;height:auto;background:#fff;border:1px solid #e5e5e0}.muted{color:#6b6e75}\
code{font-size:12px;word-break:break-all}button{font:inherit;padding:2px 8px}\
.warn{color:#9a3412}.ok{color:#166534}form.inline{display:inline}";

/// Escapes text for HTML.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A bar chart as inline SVG; the labels are shown every few bars.
pub fn bars(items: &[Named]) -> String {
    let (w, h) = (900.0_f64, 180.0_f64);
    if items.is_empty() {
        return "<p class=\"muted\">No data yet.</p>".to_string();
    }
    let max = items.iter().map(|i| i.value).max().unwrap_or(1).max(1) as f64;
    let bw = w / items.len() as f64;
    let step = (items.len() / 8).max(1);
    let mut s = format!("<svg class=\"chart\" viewBox=\"0 0 {w} {h}\" role=\"img\">");
    for (i, it) in items.iter().enumerate() {
        let bh = it.value as f64 / max * (h - 24.0);
        let x = i as f64 * bw + bw * 0.15;
        let y = h - 18.0 - bh;
        let _ = write!(
            s,
            "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{:.1}\" height=\"{bh:.1}\" fill=\"#3b6e8f\"><title>{} {}</title></rect>",
            bw * 0.7,
            esc(&it.name),
            it.value
        );
        if i % step == 0 {
            let _ = write!(
                s,
                "<text x=\"{:.1}\" y=\"{}\" font-size=\"10\" fill=\"#6b6e75\">{}</text>",
                x,
                h - 4.0,
                esc(it.name.get(5..).unwrap_or(&it.name))
            );
        }
    }
    s.push_str("</svg>");
    s
}

fn named_table(title: &str, rows: &[Named]) -> String {
    let mut s = format!("<h3>{}</h3><table><tr><th>{}</th><th class=\"n\">installs</th></tr>", esc(title), esc(title));
    for r in rows {
        let _ = write!(s, "<tr><td>{}</td><td class=\"n\">{}</td></tr>", esc(&r.name), r.value);
    }
    s.push_str("</table>");
    s
}

fn pct(part: i64, whole: i64) -> String {
    if whole == 0 {
        "-".to_string()
    } else {
        format!("{:.0}%", part as f64 * 100.0 / whole as f64)
    }
}

/// The dashboard's HTML, from the same numbers as the JSON.
pub fn render(sm: &Summary) -> String {
    let mut b = String::new();
    let _ = write!(
        b,
        "<h1>Razdor stats</h1><p class=\"muted\">Generated day {}. Raw rows: pings 90 days, events and performance 180 days, \
         crash reports 1 year (saves 90 days), battles forever. Quarantined rows are not counted.</p>",
        esc(&crate::civil::ymd(day_of(sm.generated_at)))
    );

    b.push_str("<h2>Players per day (last 30 days)</h2>");
    b.push_str(&bars(&sm.players_per_day));

    b.push_str("<h2>The last 7 days</h2><div style=\"display:flex;gap:24px;flex-wrap:wrap\"><div style=\"flex:1;min-width:240px\">");
    b.push_str(&named_table("version", &sm.week_by_version));
    b.push_str("</div><div style=\"flex:1;min-width:240px\">");
    b.push_str(&named_table("platform", &sm.week_by_platform));
    b.push_str("</div><div style=\"flex:1;min-width:240px\">");
    b.push_str(&named_table("game", &sm.week_by_game));
    b.push_str("</div></div>");

    let r = &sm.retention;
    let _ = write!(
        b,
        "<h2>Retention</h2><table><tr><th>cohort</th><th class=\"n\">installs</th><th class=\"n\">came back</th><th class=\"n\">rate</th></tr>\
         <tr><td>week 1 (first ping 14-20 days ago, back 7-13 days later)</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>\
         <tr><td>week 4 (first ping 34-40 days ago, back 28-34 days later)</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr></table>",
        r.week1_cohort,
        r.week1_returned,
        pct(r.week1_returned, r.week1_cohort),
        r.week4_cohort,
        r.week4_returned,
        pct(r.week4_returned, r.week4_cohort)
    );

    b.push_str("<h2>Maps: starts, ends and where games are abandoned (last 180 days)</h2><table><tr><th>map</th><th class=\"n\">starts</th><th class=\"n\">wins</th><th class=\"n\">losses</th><th class=\"n\">abandoned</th><th>abandoned at (top screens)</th></tr>");
    for m in &sm.maps {
        let at: Vec<String> = m.abandoned_at.iter().map(|a| format!("{} ({})", esc(&a.name), a.value)).collect();
        let _ = write!(
            b,
            "<tr><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td>{}</td></tr>",
            esc(&m.map),
            m.starts,
            m.wins,
            m.losses,
            m.abandoned,
            at.join(", ")
        );
    }
    b.push_str("</table>");

    b.push_str("<h2>Crashes (last 30 days, grouped by location and message)</h2><table><tr><th>count</th><th>location</th><th>message</th><th>last</th><th>version</th></tr>");
    for c in &sm.crashes {
        let _ = write!(
            b,
            "<tr><td class=\"n\">{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td></tr>",
            c.count,
            esc(&c.location),
            esc(&c.message),
            esc(&c.last_day),
            esc(&c.last_version)
        );
    }
    b.push_str("</table>");

    b.push_str("<h2>Frames by GPU and phone</h2><table><tr><th>GPU</th><th>phone</th><th class=\"n\">sessions</th><th class=\"n\">median ms</th><th class=\"n\">p99 ms</th><th class=\"n\">FPS (median)</th></tr>");
    for f in &sm.fps {
        let fps = if f.median_ms > 0.0 { 1000.0 / f.median_ms } else { 0.0 };
        let _ = write!(
            b,
            "<tr><td>{}</td><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{:.1}</td><td class=\"n\">{:.1}</td><td class=\"n\">{:.0}</td></tr>",
            esc(&f.gpu),
            esc(&f.phone),
            f.sessions,
            f.median_ms,
            f.p99_ms,
            fps
        );
    }
    b.push_str("</table>");

    b.push_str("<h2>Experiments (map starts with each switch on)</h2>");
    b.push_str(&named_table("experiment", &sm.experiments));

    b.push_str("<h2>Cloud</h2>");
    let _ = write!(
        b,
        "<p>Accounts: <b>{}</b>. Saved on the server: <b>{:.2} MB</b>.</p>",
        sm.cloud.accounts,
        sm.cloud.used_bytes as f64 / (1024.0 * 1024.0)
    );
    b.push_str("<p class=\"muted\">Syncs per day (saves, marks, settings, hub, bench, links, accounts):</p>");
    b.push_str(&bars(&sm.cloud.syncs_per_day));

    b.push_str("<h2>Storage</h2>");
    let s = &sm.storage;
    let state = if s.over { "<span class=\"warn\">over the budget: no new uploads</span>" } else { "<span class=\"muted\">within the budget</span>" };
    let _ = write!(
        b,
        "<p>Stored files: <b>{:.2} GB</b> of <b>{:.2} GB</b>. Free on the disk: <b>{:.2} GB</b> (floor {:.2} GB). {}</p>",
        s.used_bytes as f64 / 1e9,
        s.max_bytes as f64 / 1e9,
        s.free_bytes as f64 / 1e9,
        s.min_free_bytes as f64 / 1e9,
        state
    );

    b.push_str("<h2>Bans</h2><table><tr><th>kind</th><th>key</th><th class=\"n\">strikes</th><th>since</th><th>reason</th><th>state</th><th></th></tr>");
    for ban in &sm.bans {
        let state = if ban.active { "<span class=\"warn\">active</span>" } else { "<span class=\"muted\">lifted</span>" };
        let action = if ban.active {
            format!(
                "<form class=\"inline\" method=\"post\" action=\"/admin/unban\"><input type=\"hidden\" name=\"kind\" value=\"{}\">\
                 <input type=\"hidden\" name=\"key\" value=\"{}\"><button>Unban</button></form>",
                esc(&ban.kind),
                esc(&ban.key)
            )
        } else {
            String::new()
        };
        let _ = write!(
            b,
            "<tr><td>{}</td><td><code>{}</code></td><td class=\"n\">{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(&ban.kind),
            esc(&ban.key),
            ban.strikes,
            esc(&ban.created_day),
            esc(&ban.reason),
            state,
            action
        );
    }
    b.push_str("</table>");
    b.push_str("<p class=\"muted\"><a href=\"/admin/api/summary.json\">summary.json</a> (the same numbers; admin token or this session).</p>");
    b
}

pub async fn page_get(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if !session_ok(&st, &headers) {
        return Ok(page(
            StatusCode::OK,
            "Razdor stats",
            "<h1>Razdor stats</h1><form method=\"post\" action=\"/admin/login\">\
             <label>Password <input type=\"password\" name=\"password\" autofocus></label> <button>Enter</button></form>",
        ));
    }
    let sm = summary::build(&st).await?;
    Ok(page(StatusCode::OK, "Razdor stats", &render(&sm)))
}

#[derive(Deserialize)]
pub struct LoginForm {
    password: String,
}

pub async fn login(
    State(st): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Result<Response, ApiError> {
    let ip = ip_of(&st, &headers, peer);
    abuse::login_gate(&st, ip)?;
    let Some(hash) = st.cfg.admin_password_hash.clone() else {
        return Ok(page(StatusCode::SERVICE_UNAVAILABLE, "Razdor stats", "<p>No admin password is set.</p>"));
    };
    // The argon2 check runs on the blocking pool, one at a time (`hash_gate`): a flood of logins
    // waits in async and cannot take every thread or all the memory at once.
    let ok = {
        let _permit = st
            .hash_gate
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| ApiError::Internal("hash gate closed".into()))?;
        let password = form.password;
        tokio::task::spawn_blocking(move || verify_password(&hash, &password))
            .await
            .map_err(|e| ApiError::Internal(format!("argon2 task: {e}")))?
    };
    if !ok {
        return Ok(page(StatusCode::UNAUTHORIZED, "Razdor stats", "<p>Wrong password.</p>"));
    }
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| ApiError::Internal(format!("random: {e}")))?;
    let token = crate::files::hex(&raw);
    let now = st.clock.now();
    {
        let mut sessions = st.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.retain(|_, exp| *exp > now);
        sessions.insert(sha256_hex(&[token.as_bytes()]), now + SESSION_SECS);
    }
    let mut r = Redirect::to("/admin").into_response();
    r.headers_mut().insert(header::SET_COOKIE, set_cookie(&st, &token, SESSION_SECS));
    Ok(r)
}

fn set_cookie(st: &AppState, value: &str, max_age: i64) -> axum::http::HeaderValue {
    let secure = if st.cfg.secure_cookies { "; Secure" } else { "" };
    format!("{COOKIE}={value}; Path=/admin; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}")
        .parse()
        .expect("a cookie header")
}

pub async fn logout(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(token) = cookie_token(&headers) {
        let key = sha256_hex(&[token.as_bytes()]);
        st.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
    }
    let mut r = Redirect::to("/admin").into_response();
    r.headers_mut().insert(header::SET_COOKIE, set_cookie(&st, "", 0));
    Ok(r)
}

#[derive(Deserialize)]
pub struct UnbanForm {
    kind: String,
    key: String,
}

pub async fn unban(State(st): State<AppState>, headers: HeaderMap, Form(form): Form<UnbanForm>) -> Result<Response, ApiError> {
    if !session_ok(&st, &headers) {
        return Err(ApiError::Unauthorized("log in first"));
    }
    let kind = match form.kind.as_str() {
        "install" => "install",
        "ip" => "ip",
        _ => return Err(ApiError::BadRequest("unknown ban kind".into())),
    };
    abuse::unban(&st, kind, form.key).await?;
    Ok(Redirect::to("/admin").into_response())
}

pub async fn summary_json(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if !token_ok(&st, &headers) && !session_ok(&st, &headers) {
        return Err(ApiError::Unauthorized("admin token or dashboard session"));
    }
    let sm = summary::build(&st).await?;
    Ok(json(&sm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_player_text() {
        assert_eq!(esc("<b>\"x\" & 'y'</b>"), "&lt;b&gt;&quot;x&quot; &amp; &#39;y&#39;&lt;/b&gt;");
    }

    #[test]
    fn password_round_trip() {
        let h = hash_password("correct horse").expect("hashes");
        assert!(verify_password(&h, "correct horse"));
        assert!(!verify_password(&h, "wrong"));
    }

    #[test]
    fn constant_compare() {
        assert!(constant_eq(b"abc", b"abc"));
        assert!(!constant_eq(b"abc", b"abd"));
        assert!(!constant_eq(b"ab", b"abc"));
    }
}
