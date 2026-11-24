//! The dashboard's numbers, one struct for the HTML page and the `/admin/api/summary.json` API.
//! Raw rows and the per-day aggregates are read together, so the numbers hold across the retention
//! cut-off. Quarantined rows never count.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::params;
use serde::Serialize;

use crate::adoption::{self, AdoptionDay};
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
    /// The day of the group's first crash in the last 30 days.
    pub first_day: String,
    /// The razdor version of that first crash.
    pub first_version: String,
    /// True when the group began on the newest version seen in pings (`latest_version`).
    pub is_new: bool,
}

/// Distinct installs per day and razdor version (raw pings only, so it is empty beyond their retention).
#[derive(Debug, Serialize, PartialEq)]
pub struct VersionDay {
    pub day: String,
    pub version: String,
    pub installs: i64,
}

/// A version's first day in the pings (raw rows and their daily aggregates).
#[derive(Debug, Serialize, PartialEq)]
pub struct Release {
    pub version: String,
    pub first_day: String,
}

/// The crashes of one razdor version, and the installs that pinged with it, in the last 30 days.
#[derive(Debug, Serialize, PartialEq)]
pub struct CrashRate {
    pub version: String,
    pub crashes: i64,
    pub installs: i64,
}

/// The headline numbers: the last 7 days against the 7 days before them.
#[derive(Debug, Serialize, Default, PartialEq)]
pub struct Overview {
    /// Distinct installs that pinged in the last 7 days (today and the 6 days before).
    pub players_7d: i64,
    /// Distinct installs that pinged in the 7 days before those.
    pub players_prev_7d: i64,
    /// Installs whose first ping fell in the last 7 days.
    pub new_installs_7d: i64,
    /// Installs whose first ping fell in the 7 days before those.
    pub new_installs_prev_7d: i64,
    /// Crash reports in the last 7 days.
    pub crashes_7d: i64,
    /// Crash reports in the 7 days before those.
    pub crashes_prev_7d: i64,
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
    /// The chart range in days: 7, 30 or 90 (anything else asked for is 30).
    pub range_days: i64,
    pub overview: Overview,
    /// Installs that pinged, per day (the chart range).
    pub players_per_day: Vec<Named>,
    /// Installs per day on each razdor version and platform, from the daily ping (raw pings and the
    /// per-day aggregates, so it reaches past 90 days).
    pub adoption: Vec<AdoptionDay>,
    /// Distinct installs per day and razdor version (the chart range).
    pub versions_per_day: Vec<VersionDay>,
    /// The versions whose first day is in the chart range, by day.
    pub releases: Vec<Release>,
    /// The newest version seen in pings in the last 30 days ("" when there is none).
    pub latest_version: String,
    /// The next older version in `crash_rates` ("" when there is none).
    pub previous_version: String,
    /// Crashes and installs per version (last 30 days), newest first.
    pub crash_rates: Vec<CrashRate>,
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
    /// Installs per country over the chart range (install-days; `downloads::countries`).
    pub countries: Vec<Named>,
    /// The download counters of the chart range (`downloads::downloads`).
    pub downloads: crate::downloads::Downloads,
}

fn named(rows: Vec<(String, i64)>) -> Vec<Named> {
    rows.into_iter().map(|(name, value)| Named { name, value }).collect()
}

/// The chart ranges the dashboard offers; any other number of days is 30.
pub fn clamp_range(days: i64) -> i64 {
    match days {
        7 | 30 | 90 => days,
        _ => 30,
    }
}

/// One dot-separated part of a version: a number compares as a number and sorts before any text.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Part<'a> {
    Num(u64),
    Text(&'a str),
}

fn version_key(s: &str) -> Vec<Part<'_>> {
    s.split('.').map(|p| p.parse::<u64>().map_or(Part::Text(p), Part::Num)).collect()
}

/// Orders razdor versions part by part, numbers as numbers: "1.10.0" is newer than "1.9.3".
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    version_key(a).cmp(&version_key(b)).then_with(|| a.cmp(b))
}

pub async fn build(st: &AppState, range_days: i64) -> Result<Summary, ApiError> {
    let now = st.clock.now();
    let today = day_of(now);
    let range = clamp_range(range_days);
    let from = today - (range - 1);
    let month = today - 29;
    let week = today - 6;
    let adoption = st.stats.read(move |c| Ok(adoption::days(c, from)?)).await?;
    let stats = st
        .stats
        .read(move |c| {
            let players_per_day = c
                .prepare(
                    "SELECT day, SUM(n) FROM (
                       SELECT day, COUNT(DISTINCT install_id) AS n FROM pings WHERE quarantined = 0 AND day >= ?1 GROUP BY day
                       UNION ALL
                       SELECT day, installs AS n FROM daily_players WHERE day >= ?1
                     ) GROUP BY day ORDER BY day",
                )?
                .query_map(params![from], |r| Ok((ymd(r.get(0)?), r.get::<_, i64>(1)?)))?
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

            // The 30-day figures: crashes and installs per version, and the newest version in pings.
            let by_version = |sql: &str| -> rusqlite::Result<BTreeMap<String, i64>> {
                c.prepare(sql)?
                    .query_map(params![month], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                    .collect()
            };
            let crash_counts = by_version(
                "SELECT razdor, COUNT(*) FROM crashes WHERE quarantined = 0 AND day >= ?1 GROUP BY razdor",
            )?;
            let install_counts = by_version(
                "SELECT razdor, COUNT(DISTINCT install_id) FROM pings WHERE quarantined = 0 AND day >= ?1 GROUP BY razdor",
            )?;
            let latest_version = install_counts.keys().max_by(|a, b| version_cmp(a, b)).cloned().unwrap_or_default();
            let names: BTreeSet<&String> = crash_counts.keys().chain(install_counts.keys()).collect();
            let mut crash_rates: Vec<CrashRate> = names
                .into_iter()
                .map(|v| CrashRate {
                    version: v.clone(),
                    crashes: crash_counts.get(v).copied().unwrap_or(0),
                    installs: install_counts.get(v).copied().unwrap_or(0),
                })
                .collect();
            crash_rates.sort_by(|a, b| version_cmp(&b.version, &a.version));
            let previous_version = crash_rates
                .iter()
                .map(|r| r.version.as_str())
                .filter(|v| !latest_version.is_empty() && version_cmp(v, &latest_version) == Ordering::Less)
                .max_by(|a, b| version_cmp(a, b))
                .unwrap_or("")
                .to_string();

            let mut crashes = c
                .prepare(
                    "SELECT c.panic_loc, c.panic_msg, COUNT(*), MAX(c.day), MAX(c.razdor), MIN(c.day),
                       (SELECT e.razdor FROM crashes e WHERE e.quarantined = 0 AND e.day >= ?1
                          AND e.panic_loc = c.panic_loc AND e.panic_msg = c.panic_msg
                          ORDER BY e.day, e.received_at LIMIT 1)
                     FROM crashes c WHERE c.quarantined = 0 AND c.day >= ?1
                     GROUP BY c.panic_loc, c.panic_msg ORDER BY 3 DESC LIMIT 30",
                )?
                .query_map(params![month], |r| {
                    Ok(CrashRow {
                        location: r.get(0)?,
                        message: r.get(1)?,
                        count: r.get(2)?,
                        last_day: ymd(r.get(3)?),
                        last_version: r.get(4)?,
                        first_day: ymd(r.get(5)?),
                        first_version: r.get(6)?,
                        is_new: false,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            for row in &mut crashes {
                row.is_new = !latest_version.is_empty() && row.first_version == latest_version;
            }

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
                .query_map(params![from], |r| Ok((ymd(r.get(0)?), r.get::<_, i64>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?,
            );

            let players_in = |lo: i64, hi: i64| -> rusqlite::Result<i64> {
                c.query_row(
                    "SELECT COUNT(DISTINCT install_id) FROM pings WHERE quarantined = 0 AND day BETWEEN ?1 AND ?2",
                    params![lo, hi],
                    |r| r.get(0),
                )
            };
            let new_in = |lo: i64, hi: i64| -> rusqlite::Result<i64> {
                c.query_row(
                    "WITH firsts AS (SELECT install_id, MIN(day) AS first FROM pings WHERE quarantined = 0 GROUP BY install_id)
                     SELECT COUNT(*) FROM firsts WHERE first BETWEEN ?1 AND ?2",
                    params![lo, hi],
                    |r| r.get(0),
                )
            };
            let crashes_in = |lo: i64, hi: i64| -> rusqlite::Result<i64> {
                c.query_row(
                    "SELECT COUNT(*) FROM crashes WHERE quarantined = 0 AND day BETWEEN ?1 AND ?2",
                    params![lo, hi],
                    |r| r.get(0),
                )
            };
            let (w7, p7) = ((today - 6, today), (today - 13, today - 7));
            let overview = Overview {
                players_7d: players_in(w7.0, w7.1)?,
                players_prev_7d: players_in(p7.0, p7.1)?,
                new_installs_7d: new_in(w7.0, w7.1)?,
                new_installs_prev_7d: new_in(p7.0, p7.1)?,
                crashes_7d: crashes_in(w7.0, w7.1)?,
                crashes_prev_7d: crashes_in(p7.0, p7.1)?,
            };

            let versions_per_day = c
                .prepare(
                    "SELECT day, razdor, COUNT(DISTINCT install_id) FROM pings
                     WHERE quarantined = 0 AND day >= ?1 GROUP BY day, razdor ORDER BY day, razdor",
                )?
                .query_map(params![from], |r| {
                    Ok(VersionDay { day: ymd(r.get(0)?), version: r.get(1)?, installs: r.get(2)? })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let releases = c
                .prepare(
                    "SELECT razdor, MIN(day) FROM (
                       SELECT razdor, day FROM pings WHERE quarantined = 0
                       UNION ALL
                       SELECT razdor, day FROM daily_players
                     ) GROUP BY razdor HAVING MIN(day) >= ?1 ORDER BY MIN(day), razdor",
                )?
                .query_map(params![from], |r| Ok(Release { version: r.get(0)?, first_day: ymd(r.get(1)?) }))?
                .collect::<Result<Vec<_>, _>>()?;

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
                overview,
                versions_per_day,
                releases,
                crash_rates,
                latest_version,
                previous_version,
            ))
        })
        .await?;
    let (
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
        overview,
        versions_per_day,
        releases,
        crash_rates,
        latest_version,
        previous_version,
    ) = stats;
    let cloud = st
        .cloud
        .read(move |c| {
            let accounts: i64 = c.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
            let used_bytes: i64 = c.query_row("SELECT COALESCE(SUM(size), 0) FROM saves", [], |r| r.get(0))?;
            Ok(CloudStats { accounts, used_bytes, syncs_per_day })
        })
        .await?;
    let countries = crate::downloads::countries(st, from).await?;
    let downloads = crate::downloads::downloads(st, from).await?;
    Ok(Summary {
        generated_at: now,
        range_days: range,
        overview,
        players_per_day,
        adoption,
        versions_per_day,
        releases,
        latest_version,
        previous_version,
        crash_rates,
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
        countries,
        downloads,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numbers_as_numbers() {
        assert_eq!(version_cmp("1.10.0", "1.9.3"), Ordering::Greater);
        assert_ne!(version_cmp("1.5.2", "1.5.10"), Ordering::Greater);
        assert_eq!(version_cmp("1.5.2", "1.5.10"), Ordering::Less);
        assert_eq!(version_cmp("1.5", "1.5.0"), Ordering::Less);
        assert_eq!(version_cmp("1.5.4", "1.5.4"), Ordering::Equal);
    }

    #[test]
    fn range_is_one_of_three() {
        assert_eq!(clamp_range(7), 7);
        assert_eq!(clamp_range(90), 90);
        assert_eq!(clamp_range(45), 30);
        assert_eq!(clamp_range(0), 30);
    }
}
