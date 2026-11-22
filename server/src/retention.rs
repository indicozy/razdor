//! The retention job (daily): raw pings older than 90 days and raw events and performance older than
//! 180 days are folded into per-day aggregates and deleted; crash saves go after 90 days, crash
//! reports after a year; expired link codes go at once. Battles and verdicts are kept forever.
//! Cloud accounts idle for `account_expiry_days` (90; 0 is off) go with their save files.
//! The clock is injected, so the tests run it on any day.

use std::time::Duration;

use rusqlite::params;

use crate::budget;
use crate::civil::{day_of, DAY};
use crate::cloud;
use crate::files::{self, CRASHES};
use crate::http::ApiError;
use crate::state::AppState;

pub const PINGS_DAYS: i64 = 90;
pub const EVENTS_DAYS: i64 = 180;
pub const CRASH_SAVE_DAYS: i64 = 90;
pub const CRASH_REPORT_DAYS: i64 = 365;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub pings: usize,
    pub events: usize,
    pub perf: usize,
    pub crash_saves: usize,
    pub crash_reports: usize,
    pub links: usize,
    pub accounts: usize,
}

/// Deletes the accounts idle past `account_expiry_days` (0: never), with their save files.
async fn expire_accounts(st: &AppState) -> Result<usize, ApiError> {
    let days = i64::from(st.cfg.limits.account_expiry_days);
    if days == 0 {
        return Ok(0);
    }
    let before = st.clock.now() - days * DAY;
    let idle: Vec<String> = st
        .cloud
        .call(move |c| {
            let mut s = c.prepare("SELECT id FROM accounts WHERE last_active < ?1")?;
            let rows = s.query_map(params![before], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    for id in &idle {
        cloud::remove_account(st, id).await?;
    }
    Ok(idle.len())
}

/// One pass of the job, at the clock's time.
pub async fn run_once(st: &AppState) -> Result<Report, ApiError> {
    let now = st.clock.now();
    let today = day_of(now);
    let pings_before = today - PINGS_DAYS;
    let events_before = today - EVENTS_DAYS;
    let save_before = now - CRASH_SAVE_DAYS * DAY;
    let report_before = now - CRASH_REPORT_DAYS * DAY;
    let root = st.root.clone();
    let counted = st.clone();
    let mut report = st
        .stats
        .call(move |c| {
            let tx = c.transaction()?;
            let mut report = Report::default();

            tx.execute(
                "INSERT INTO daily_players (day, razdor, platform, game, lang, installs)
                 SELECT day, razdor, platform, game, lang, COUNT(DISTINCT install_id) FROM pings
                 WHERE day < ?1 AND quarantined = 0 GROUP BY day, razdor, platform, game, lang
                 ON CONFLICT (day, razdor, platform, game, lang) DO UPDATE SET installs = installs + excluded.installs",
                params![pings_before],
            )?;
            report.pings = tx.execute("DELETE FROM pings WHERE day < ?1", params![pings_before])?;

            tx.execute(
                "INSERT INTO daily_events (day, kind, map, outcome, last_screen, count)
                 SELECT day, kind, map, outcome, last_screen, COUNT(*) FROM events
                 WHERE day < ?1 AND quarantined = 0 GROUP BY day, kind, map, outcome, last_screen
                 ON CONFLICT (day, kind, map, outcome, last_screen) DO UPDATE SET count = count + excluded.count",
                params![events_before],
            )?;
            tx.execute(
                "INSERT INTO daily_events (day, kind, map, outcome, last_screen, count)
                 SELECT e.day, 'experiment', '', j.value, '', COUNT(*) FROM events e, json_each(e.payload, '$.experiments') j
                 WHERE e.kind = 'map_start' AND e.day < ?1 AND e.quarantined = 0 GROUP BY e.day, j.value
                 ON CONFLICT (day, kind, map, outcome, last_screen) DO UPDATE SET count = count + excluded.count",
                params![events_before],
            )?;
            report.events = tx.execute("DELETE FROM events WHERE day < ?1", params![events_before])?;

            tx.execute(
                "INSERT INTO daily_perf (day, gpu, phone, screen, sessions, median_sum, p99_sum)
                 SELECT day, gpu, phone, screen, COUNT(*), SUM(median_ms), SUM(p99_ms) FROM perf
                 WHERE day < ?1 AND quarantined = 0 GROUP BY day, gpu, phone, screen
                 ON CONFLICT (day, gpu, phone, screen) DO UPDATE SET sessions = sessions + excluded.sessions,
                 median_sum = median_sum + excluded.median_sum, p99_sum = p99_sum + excluded.p99_sum",
                params![events_before],
            )?;
            report.perf = tx.execute("DELETE FROM perf WHERE day < ?1", params![events_before])?;

            let mut doomed: Vec<(&'static str, String)> = Vec::new();
            {
                let mut s = tx.prepare("SELECT id, save_file FROM crashes WHERE save_file IS NOT NULL AND save_at < ?1")?;
                let rows = s
                    .query_map(params![save_before], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                for (id, file) in rows {
                    doomed.push((CRASHES, file));
                    tx.execute("UPDATE crashes SET save_file = NULL WHERE id = ?1", params![id])?;
                    report.crash_saves += 1;
                }
            }
            {
                let mut s = tx.prepare("SELECT report_file, save_file FROM crashes WHERE received_at < ?1")?;
                let rows = s
                    .query_map(params![report_before], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                for (report_file, save_file) in rows {
                    doomed.push((CRASHES, report_file));
                    if let Some(save) = save_file {
                        doomed.push((CRASHES, save));
                    }
                    report.crash_reports += 1;
                }
            }
            tx.execute("DELETE FROM crashes WHERE received_at < ?1", params![report_before])?;
            tx.commit()?;
            for (folder, name) in doomed {
                budget::remove(&counted.budget, &files::path(&root, folder, &name));
            }
            Ok(report)
        })
        .await?;

    report.links = cloud::sweep_links(st).await?;
    report.accounts = expire_accounts(st).await?;
    st.abuse().prune(now);
    Ok(report)
}

/// Every minute, whether or not requests come: the expired link codes go (their keys are not kept
/// longer than the code), and the abuse windows are pruned (the request path only does it under traffic).
pub fn spawn_minutely(st: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            match cloud::sweep_links(&st).await {
                Ok(0) => {}
                Ok(n) => tracing::info!("links: {n} expired codes removed"),
                Err(e) => tracing::error!("link sweep failed: {e:?}"),
            }
            st.abuse().maybe_prune(st.clock.now());
        }
    });
}

/// Runs the job once a day, starting five minutes after the service starts.
pub fn spawn(st: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(300)).await;
        loop {
            match run_once(&st).await {
                Ok(r) => tracing::info!(
                    "retention: {} pings, {} events, {} perf rows folded; {} crash saves, {} crash reports, {} links; {} idle accounts removed",
                    r.pings,
                    r.events,
                    r.perf,
                    r.crash_saves,
                    r.crash_reports,
                    r.links,
                    r.accounts
                ),
                Err(e) => tracing::error!("retention failed: {e:?}"),
            }
            tokio::time::sleep(Duration::from_secs(DAY as u64)).await;
        }
    });
}
