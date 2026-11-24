//! The crash detail page: the reports of one crash group (the same location and message), each with
//! its time, version, platform, screen, settings and the stored log tail. Every report is joined with
//! the same install's latest perf report for the device (phone model and GPU), shown as "unknown" when
//! the install has none. Only what the server stores: the crash rows, their report files and the perf
//! rows. Every player-sent text goes through `esc`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use rusqlite::{params, Connection};

use razdor_wire::analytics::CrashReport;

use crate::admin::{esc, page, session_ok};
use crate::civil::{day_of, ymd, DAY};
use crate::files::{self, CRASHES};
use crate::http::ApiError;
use crate::state::AppState;

#[cfg(test)]
mod tests;

/// The crash table's window: the reports of the last 30 days, like the dashboard's group counts.
pub const WINDOW_DAYS: i64 = 30;
/// Reports listed per page; the rest are counted, not shown.
pub const MAX_REPORTS: i64 = 100;
/// The last log lines shown per report.
pub const LOG_TAIL_LINES: usize = 60;

/// The device of a report's install, from its latest perf report. `None` when the install sent none.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Device {
    pub phone: Option<String>,
    pub gpu: Option<String>,
}

/// One stored crash row of the group, with the device joined in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashItem {
    pub id: String,
    pub received_at: i64,
    pub razdor: String,
    pub platform: String,
    pub game: String,
    pub screen: String,
    pub report_file: String,
    pub has_save: bool,
    pub device: Device,
}

/// The crash rows of one group (`panic_loc`, `panic_msg`) received since `since`, newest first, at
/// most `MAX_REPORTS`, each with its install's latest perf report (`perf` rows, quarantined ones
/// skipped). An empty `phone` or `gpu` column (the client sent none) reads as `None`.
pub fn group_items(c: &Connection, loc: &str, msg: &str, since: i64) -> rusqlite::Result<(Vec<CrashItem>, i64)> {
    let total: i64 = c.query_row(
        "SELECT COUNT(*) FROM crashes WHERE quarantined = 0 AND day >= ?3 AND panic_loc = ?1 AND panic_msg = ?2",
        params![loc, msg, since],
        |r| r.get(0),
    )?;
    let mut stmt = c.prepare(
        "SELECT c.id, c.received_at, c.razdor, c.platform, c.game, c.screen, c.report_file, c.has_save,
           (SELECT NULLIF(p.phone, '') FROM perf p WHERE p.install_id = c.install_id AND p.quarantined = 0
              ORDER BY p.received_at DESC, p.id DESC LIMIT 1),
           (SELECT NULLIF(p.gpu, '') FROM perf p WHERE p.install_id = c.install_id AND p.quarantined = 0
              ORDER BY p.received_at DESC, p.id DESC LIMIT 1)
         FROM crashes c
         WHERE c.quarantined = 0 AND c.day >= ?3 AND c.panic_loc = ?1 AND c.panic_msg = ?2
         ORDER BY c.received_at DESC, c.id DESC LIMIT ?4",
    )?;
    let items = stmt
        .query_map(params![loc, msg, since, MAX_REPORTS], |r| {
            Ok(CrashItem {
                id: r.get(0)?,
                received_at: r.get(1)?,
                razdor: r.get(2)?,
                platform: r.get(3)?,
                game: r.get(4)?,
                screen: r.get(5)?,
                report_file: r.get(6)?,
                has_save: r.get::<_, i64>(7)? != 0,
                device: Device { phone: r.get(8)?, gpu: r.get(9)? },
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok((items, total))
}

/// A crash row with its stored report, when the file can be read.
pub struct Detail {
    pub item: CrashItem,
    pub report: Option<CrashReport>,
}

/// Reads a report file (zstd JSON). `None` when it is missing or does not parse.
pub fn read_report(path: &std::path::Path) -> Option<CrashReport> {
    let zst = std::fs::read(path).ok()?;
    let json = files::unzstd(&zst).ok()?;
    serde_json::from_slice(&json).ok()
}

/// The link to a group's page, with the location and message percent-encoded.
pub fn group_href(loc: &str, msg: &str) -> String {
    format!("/admin/crash?loc={}&msg={}", query_escape(loc), query_escape(msg))
}

/// Percent-encodes everything but the unreserved characters (RFC 3986).
pub fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `YYYY-MM-DD HH:MM:SS` (UTC) of a Unix time.
pub fn utc_time(t: i64) -> String {
    let secs = t.rem_euclid(DAY);
    format!("{} {:02}:{:02}:{:02}", ymd(day_of(t)), secs / 3600, secs % 3600 / 60, secs % 60)
}

fn device_cell(value: &Option<String>) -> String {
    match value {
        Some(v) => esc(v),
        None => "<span class=\"muted\">unknown</span>".to_string(),
    }
}

/// The last `n` lines of a log, in order.
fn tail(lines: &[String], n: usize) -> &[String] {
    &lines[lines.len().saturating_sub(n)..]
}

/// The page's body: the group's heading, its reports and each report's settings and log tail.
pub fn render(loc: &str, msg: &str, shown: &[Detail], total: i64) -> String {
    let mut b = String::new();
    b.push_str("<p class=\"range\"><a href=\"/admin?days=30#crashes\">Back to the dashboard</a></p>");
    b.push_str(&format!("<h1>Crash group</h1><p><code>{}</code></p><p>{}</p>", esc(loc), esc(msg)));
    let note = if total > shown.len() as i64 {
        format!(", the newest {} shown", shown.len())
    } else {
        String::new()
    };
    b.push_str(&format!(
        "<p class=\"muted\">{total} reports in the last {WINDOW_DAYS} days{note}. Times are UTC. Device: the \
         install's latest performance report; \"unknown\" when it sent none.</p>"
    ));
    if shown.is_empty() {
        b.push_str("<p class=\"muted\">No reports in this window.</p>");
        return b;
    }
    b.push_str("<div class=\"scroll\"><table class=\"wide\"><thead><tr><th>time</th><th>version</th><th>platform</th>\
         <th>game</th><th>screen</th><th>phone</th><th>GPU</th><th>save</th></tr></thead><tbody>");
    for d in shown {
        let it = &d.item;
        b.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            utc_time(it.received_at),
            esc(&it.razdor),
            esc(&it.platform),
            esc(&it.game),
            esc(&it.screen),
            device_cell(&it.device.phone),
            device_cell(&it.device.gpu),
            if it.has_save { "yes" } else { "no" },
        ));
    }
    b.push_str("</tbody></table></div>");
    for d in shown {
        b.push_str(&render_report(d));
    }
    b
}

fn render_report(d: &Detail) -> String {
    let it = &d.item;
    let mut b = format!("<section><h3>{} · {}</h3>", utc_time(it.received_at), esc(&it.razdor));
    let Some(report) = &d.report else {
        b.push_str("<p class=\"muted\">The stored report is missing or unreadable.</p></section>");
        return b;
    };
    b.push_str("<details><summary>Settings and log tail</summary>");
    if report.settings.is_empty() {
        b.push_str("<p class=\"muted\">No settings sent.</p>");
    } else {
        let rows: BTreeMap<&String, &String> = report.settings.iter().collect();
        b.push_str("<table><tbody>");
        for (k, v) in rows {
            b.push_str(&format!("<tr><td><code>{}</code></td><td>{}</td></tr>", esc(k), esc(v)));
        }
        b.push_str("</tbody></table>");
    }
    let log = tail(&report.log, LOG_TAIL_LINES).join("\n");
    b.push_str(&format!(
        "<h3>Log tail ({} of {} lines)</h3><pre>{}</pre>",
        report.log.len().min(LOG_TAIL_LINES),
        report.log.len(),
        esc(&log)
    ));
    b.push_str(&format!("<h3>Panic</h3><pre>{}</pre></details></section>", esc(&report.panic_text)));
    b
}

/// The page's HTML body for a group, read from the stats database and the report files.
async fn detail(st: &AppState, loc: String, msg: String) -> Result<String, ApiError> {
    let since = st.clock.now().div_euclid(DAY) - (WINDOW_DAYS - 1);
    let (loc2, msg2) = (loc.clone(), msg.clone());
    let (items, total) = st.stats.read(move |c| Ok(group_items(c, &loc2, &msg2, since)?)).await?;
    let paths: Vec<PathBuf> = items.iter().map(|it| st.file(CRASHES, &it.report_file)).collect();
    let details = tokio::task::spawn_blocking(move || {
        items
            .into_iter()
            .zip(paths)
            .map(|(item, path)| {
                let report = read_report(&path);
                Detail { item, report }
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| ApiError::Internal(format!("report task: {e}")))?;
    Ok(render(&loc, &msg, &details, total))
}

/// `GET /admin/crash?loc=&msg=`: the reports of one crash group. Needs the dashboard session, like
/// the dashboard itself.
pub async fn page_get(
    State(st): State<AppState>,
    Query(q): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if !session_ok(&st, &headers) {
        return Ok(crate::admin::login_page());
    }
    let loc = q.get("loc").cloned().unwrap_or_default();
    let msg = q.get("msg").cloned().unwrap_or_default();
    let body = detail(&st, loc, msg).await?;
    Ok(page(StatusCode::OK, "Razdor crash group", &body))
}
