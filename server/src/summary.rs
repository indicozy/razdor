//! The dashboard's numbers, one struct for the HTML page and the `/admin/api/summary.json` API.
//! Raw rows and the per-day aggregates are read together, so the numbers hold across the retention
//! cut-off. Quarantined rows never count.

use std::collections::BTreeMap;

use rusqlite::params;
use serde::Serialize;

use crate::budget::Figures;
use crate::civil::{day_of, ymd};
use crate::http::ApiError;
use crate::state::AppState;

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct Named {
    pub name: String,
    pub value: i64,
}

#[derive(Debug, Serialize, Default, PartialEq)]
pub struct Retention {
    /// Installs whose first ping was 14 to 20 days ago, and those that pinged again a week later.
    pub week1_cohort: i64,
    pub week1_returned: i64,
    /// Installs whose first ping was 34 to 40 days ago, and those that pinged again four weeks later.
    pub week4_cohort: i64,
    pub week4_returned: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MapRow {
    pub map: String,
    pub starts: i64,
    pub wins: i64,
    pub losses: i64,
    pub abandoned: i64,
    /// Where the abandoned games stopped: the last screen, with how many.
    pub abandoned_at: Vec<Named>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct CrashRow {
    pub location: String,
    pub message: String,
    pub count: i64,
    pub last_day: String,
    pub last_version: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FpsRow {
    pub gpu: String,
    pub phone: String,
    pub sessions: i64,
    pub median_ms: f64,
    pub p99_ms: f64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct BanRow {
    pub kind: String,
    pub key: String,
    pub strikes: i64,
    pub active: bool,
    pub reason: String,
    pub created_day: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct CloudStats {
    pub accounts: i64,
    pub used_bytes: i64,
    pub syncs_per_day: Vec<Named>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Summary {
    pub generated_at: i64,
    /// Installs that pinged, per day (last 30 days).
    pub players_per_day: Vec<Named>,
    pub week_by_version: Vec<Named>,
    pub week_by_platform: Vec<Named>,
    pub week_by_game: Vec<Named>,
    pub retention: Retention,
    pub maps: Vec<MapRow>,
    pub crashes: Vec<CrashRow>,
    pub fps: Vec<FpsRow>,
    pub experiments: Vec<Named>,
    pub bans: Vec<BanRow>,
    pub cloud: CloudStats,
    /// The stored files against the storage budget (see `budget`).
    pub storage: Figures,
}

fn named(rows: Vec<(String, i64)>) -> Vec<Named> {
    rows.into_iter().map(|(name, value)| Named { name, value }).collect()
}

pub async fn build(st: &AppState) -> Result<Summary, ApiError> {
    let now = st.clock.now();
    let today = day_of(now);
    let month = today - 29;
    let week = today - 6;
    let stats = st
        .stats
        .call(move |c| {
            let players_per_day = c
                .prepare(
                    "SELECT day, SUM(n) FROM (
                       SELECT day, COUNT(DISTINCT install_id) AS n FROM pings WHERE quarantined = 0 AND day >= ?1 GROUP BY day
                       UNION ALL
                       SELECT day, installs AS n FROM daily_players WHERE day >= ?1
                     ) GROUP BY day ORDER BY day",
                )?
                .query_map(params![month], |r| Ok((ymd(r.get(0)?), r.get::<_, i64>(1)?)))?
                .map(|r| r.map(|(d, v)| Named { name: d, value: v }))
                .collect::<Result<Vec<_>, _>>()?;

            let by = |column: &str| -> rusqlite::Result<Vec<(String, i64)>> {
                c.prepare(&format!(
                    "SELECT {column}, COUNT(DISTINCT install_id) FROM pings WHERE quarantined = 0 AND day >= ?1
                     GROUP BY {column} ORDER BY 2 DESC LIMIT 20"
                ))?
                .query_map(params![week], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .collect()
            };
            let week_by_version = named(by("razdor")?);
            let week_by_platform = named(by("platform")?);
            let week_by_game = named(by("game")?);

            // Retention: the cohort's installs (by their first ping in the raw window) that pinged again
            // inside the return window.
            let retention_of = |lo: i64, hi: i64, from: i64, to: i64| -> rusqlite::Result<(i64, i64)> {
                c.query_row(
                    "WITH firsts AS (SELECT install_id, MIN(day) AS first FROM pings WHERE quarantined = 0 GROUP BY install_id)
                     SELECT COUNT(*), COALESCE(SUM(EXISTS (SELECT 1 FROM pings p WHERE p.install_id = f.install_id
                       AND p.quarantined = 0 AND p.day BETWEEN f.first + ?3 AND f.first + ?4)), 0)
                     FROM firsts f WHERE f.first BETWEEN ?1 AND ?2",
                    params![lo, hi, from, to],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            };
            let (w1c, w1r) = retention_of(today - 20, today - 14, 7, 13)?;
            let (w4c, w4r) = retention_of(today - 40, today - 34, 28, 34)?;
            let retention = Retention { week1_cohort: w1c, week1_returned: w1r, week4_cohort: w4c, week4_returned: w4r };

            let mut maps: BTreeMap<String, MapRow> = BTreeMap::new();
            let rows = c
                .prepare(
                    "SELECT map, kind, outcome, last_screen, SUM(n) FROM (
                       SELECT map, kind, outcome, last_screen, 1 AS n FROM events
                         WHERE quarantined = 0 AND kind IN ('map_start', 'map_end') AND map <> ''
                       UNION ALL
                       SELECT map, kind, outcome, last_screen, count FROM daily_events
                         WHERE kind IN ('map_start', 'map_end') AND map <> ''
                     ) GROUP BY map, kind, outcome, last_screen",
                )?
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            for (map, kind, outcome, screen, n) in rows {
                let row = maps.entry(map.clone()).or_insert_with(|| MapRow {
                    map,
                    starts: 0,
                    wins: 0,
                    losses: 0,
                    abandoned: 0,
                    abandoned_at: Vec::new(),
                });
                match (kind.as_str(), outcome.as_str()) {
                    ("map_start", _) => row.starts += n,
                    ("map_end", "win") => row.wins += n,
                    ("map_end", "loss") => row.losses += n,
                    ("map_end", "abandoned") => {
                        row.abandoned += n;
                        match row.abandoned_at.iter_mut().find(|a| a.name == screen) {
                            Some(a) => a.value += n,
                            None => row.abandoned_at.push(Named { name: screen, value: n }),
                        }
                    }
                    _ => {}
                }
            }
            let mut maps: Vec<MapRow> = maps.into_values().collect();
            for m in &mut maps {
                m.abandoned_at.sort_by(|a, b| b.value.cmp(&a.value));
                m.abandoned_at.truncate(3);
            }
            maps.sort_by(|a, b| b.starts.cmp(&a.starts).then(a.map.cmp(&b.map)));
            maps.truncate(30);

            let crashes = c
                .prepare(
                    "SELECT panic_loc, panic_msg, COUNT(*), MAX(day), MAX(razdor) FROM crashes
                     WHERE quarantined = 0 AND day >= ?1 GROUP BY panic_loc, panic_msg ORDER BY 3 DESC LIMIT 30",
                )?
                .query_map(params![month], |r| {
                    Ok(CrashRow {
                        location: r.get(0)?,
                        message: r.get(1)?,
                        count: r.get(2)?,
                        last_day: ymd(r.get(3)?),
                        last_version: r.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let fps = c
                .prepare(
                    "SELECT gpu, phone, SUM(n), SUM(med) / SUM(n), SUM(p) / SUM(n) FROM (
                       SELECT gpu, phone, 1 AS n, median_ms AS med, p99_ms AS p FROM perf WHERE quarantined = 0
                       UNION ALL
                       SELECT gpu, phone, sessions, median_sum, p99_sum FROM daily_perf
                     ) GROUP BY gpu, phone ORDER BY SUM(n) DESC LIMIT 30",
                )?
                .query_map([], |r| {
                    Ok(FpsRow {
                        gpu: r.get(0)?,
                        phone: r.get(1)?,
                        sessions: r.get(2)?,
                        median_ms: r.get(3)?,
                        p99_ms: r.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let experiments = named(
                c.prepare(
                    "SELECT name, SUM(n) FROM (
                       SELECT j.value AS name, 1 AS n FROM events e, json_each(e.payload, '$.experiments') j
                         WHERE e.kind = 'map_start' AND e.quarantined = 0
                       UNION ALL
                       SELECT outcome, count FROM daily_events WHERE kind = 'experiment'
                     ) GROUP BY name ORDER BY 2 DESC LIMIT 50",
                )?
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?,
            );

            let bans = c
                .prepare("SELECT kind, key, strikes, until, reason, created_at FROM bans ORDER BY created_at DESC LIMIT 200")?
                .query_map([], |r| {
                    let until: i64 = r.get(3)?;
                    Ok(BanRow {
                        kind: r.get(0)?,
                        key: r.get(1)?,
                        strikes: r.get(2)?,
                        active: until > now,
                        reason: r.get(4)?,
                        created_day: ymd(day_of(r.get(5)?)),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let syncs_per_day = named(
                c.prepare(
                    "SELECT day, SUM(count) FROM daily_cloud WHERE day >= ?1 GROUP BY day ORDER BY day",
                )?
                .query_map(params![month], |r| Ok((ymd(r.get(0)?), r.get::<_, i64>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?,
            );

            Ok((
                players_per_day,
                week_by_version,
                week_by_platform,
                week_by_game,
                retention,
                maps,
                crashes,
                fps,
                experiments,
                bans,
                syncs_per_day,
            ))
        })
        .await?;
    let (players_per_day, week_by_version, week_by_platform, week_by_game, retention, maps, crashes, fps, experiments, bans, syncs_per_day) =
        stats;
    let cloud = st
        .cloud
        .call(move |c| {
            let accounts: i64 = c.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
            let used_bytes: i64 = c.query_row("SELECT COALESCE(SUM(size), 0) FROM saves", [], |r| r.get(0))?;
            Ok(CloudStats { accounts, used_bytes, syncs_per_day })
        })
        .await?;
    Ok(Summary {
        generated_at: now,
        players_per_day,
        week_by_version,
        week_by_platform,
        week_by_game,
        retention,
        maps,
        crashes,
        fps,
        experiments,
        bans,
        cloud,
        storage: st.budget.figures(),
    })
}
