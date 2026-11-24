//! Update adoption: per day, the installs on each razdor version and on each platform. It reads the
//! daily ping only (sent without consent, so it is not a self-selected sample): the raw pings of the
//! last 90 days, and the per-day aggregates `daily_players` kept forever after them. Quarantined raw
//! rows never count.

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::admin::esc;
use crate::civil::ymd;
use crate::summary::{version_cmp, Named};

/// One day's installs per version and per platform (newest version first, platforms by name).
#[derive(Debug, Serialize, PartialEq)]
pub struct AdoptionDay {
    pub day: String,
    pub versions: Vec<Named>,
    pub platforms: Vec<Named>,
}

/// The days from `from` (a day number) on, oldest first. A day with no ping is left out.
pub fn days(c: &Connection, from: i64) -> rusqlite::Result<Vec<AdoptionDay>> {
    let rows = c
        .prepare(
            "SELECT day, kind, name, SUM(n) FROM (
               SELECT day, 'version' AS kind, razdor AS name, COUNT(DISTINCT install_id) AS n FROM pings
                 WHERE quarantined = 0 AND day >= ?1 GROUP BY day, razdor
               UNION ALL
               SELECT day, 'version', razdor, installs FROM daily_players WHERE day >= ?1
               UNION ALL
               SELECT day, 'platform', platform, COUNT(DISTINCT install_id) FROM pings
                 WHERE quarantined = 0 AND day >= ?1 GROUP BY day, platform
               UNION ALL
               SELECT day, 'platform', platform, installs FROM daily_players WHERE day >= ?1
             ) GROUP BY day, kind, name ORDER BY day, kind, name",
        )?
        .query_map(params![from], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(fold(rows))
}

/// Groups the query's rows (day, kind, name, installs; sorted by day) into one entry per day.
fn fold(rows: Vec<(i64, String, String, i64)>) -> Vec<AdoptionDay> {
    let mut out: Vec<(i64, AdoptionDay)> = Vec::new();
    for (day, kind, name, installs) in rows {
        if out.last().map(|(d, _)| *d) != Some(day) {
            out.push((day, AdoptionDay { day: ymd(day), versions: Vec::new(), platforms: Vec::new() }));
        }
        let entry = &mut out.last_mut().expect("pushed above").1;
        let list = if kind == "version" { &mut entry.versions } else { &mut entry.platforms };
        list.push(Named { name, value: installs });
    }
    for (_, d) in &mut out {
        d.versions.sort_by(|a, b| version_cmp(&b.name, &a.name));
        d.platforms.sort_by(|a, b| a.name.cmp(&b.name));
    }
    out.into_iter().map(|(_, d)| d).collect()
}

/// The share of `value` in the day's total, as a whole percent ("-" when the day has no installs).
fn share(value: i64, total: i64) -> String {
    if total <= 0 {
        "-".to_string()
    } else {
        format!("{}%", (value * 100 + total / 2) / total)
    }
}

/// The dashboard's adoption section: one table of versions per day, one of platforms per day. The
/// newest day is first; each cell shows the installs and the share of that day's installs.
pub fn section(days: &[AdoptionDay]) -> String {
    let mut b = String::from("<section id=\"adoption\"><h2>Update adoption (installs per day)</h2>");
    b.push_str("<p class=\"muted\">From the daily ping (no consent needed), so it covers every install that pinged. \
                 Each cell: installs and their share of the day.</p>");
    if days.is_empty() {
        b.push_str("<p class=\"muted\">No data yet.</p></section>");
        return b;
    }
    let mut versions: Vec<&str> = days.iter().flat_map(|d| d.versions.iter().map(|v| v.name.as_str())).collect();
    versions.sort_by(|a, b| version_cmp(b, a));
    versions.dedup();
    let mut platforms: Vec<&str> = days.iter().flat_map(|d| d.platforms.iter().map(|v| v.name.as_str())).collect();
    platforms.sort();
    platforms.dedup();
    b.push_str("<h3>By version</h3>");
    b.push_str(&table(days, &versions, |d| &d.versions));
    b.push_str("<h3>By platform</h3>");
    b.push_str(&table(days, &platforms, |d| &d.platforms));
    b.push_str("</section>");
    b
}

fn table<'a>(days: &'a [AdoptionDay], names: &[&str], pick: impl Fn(&'a AdoptionDay) -> &'a Vec<Named>) -> String {
    let mut b = String::from("<div class=\"scroll\"><table class=\"wide\"><thead><tr><th>day</th>");
    for n in names {
        b.push_str(&format!("<th class=\"n\">{}</th>", esc(n)));
    }
    b.push_str("</tr></thead><tbody>");
    for d in days.iter().rev() {
        let list = pick(d);
        let total: i64 = list.iter().map(|v| v.value).sum();
        b.push_str(&format!("<tr><td>{}</td>", esc(&d.day)));
        for n in names {
            match list.iter().find(|v| v.name == *n) {
                Some(v) => b.push_str(&format!(
                    "<td class=\"n\">{} <span class=\"muted\">{}</span></td>",
                    v.value,
                    share(v.value, total)
                )),
                None => b.push_str("<td class=\"n\">-</td>"),
            }
        }
        b.push_str("</tr>");
    }
    b.push_str("</tbody></table></div>");
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ping(c: &Connection, install: &str, day: i64, razdor: &str, platform: &str, quarantined: i64) {
        c.execute(
            "INSERT INTO pings (install_id, day, received_at, razdor, platform, game, lang, quarantined)
             VALUES (?1, ?2, 0, ?3, ?4, 'base', 'en', ?5)",
            params![install, day, razdor, platform, quarantined],
        )
        .expect("inserts a ping");
    }

    fn db() -> Connection {
        let c = Connection::open_in_memory().expect("opens");
        c.execute_batch(crate::schema::STATS).expect("schema");
        c
    }

    #[test]
    fn counts_installs_per_version_and_platform_per_day() {
        let c = db();
        // Day 100: installs a and b on 1.5.4 (a also pings on 1.5.5), c is quarantined. Platforms count installs.
        ping(&c, "a", 100, "1.5.4", "linux", 0);
        ping(&c, "b", 100, "1.5.4", "windows", 0);
        ping(&c, "a", 100, "1.5.5", "linux", 0);
        ping(&c, "c", 100, "1.5.5", "linux", 1); // quarantined: not counted
        ping(&c, "a", 101, "1.5.5", "linux", 0);
        let days = days(&c, 0).expect("aggregates");
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].day, ymd(100));
        assert_eq!(
            days[0].versions,
            vec![Named { name: "1.5.5".into(), value: 1 }, Named { name: "1.5.4".into(), value: 2 }]
        );
        assert_eq!(
            days[0].platforms,
            vec![Named { name: "linux".into(), value: 1 }, Named { name: "windows".into(), value: 1 }]
        );
        assert_eq!(days[1].versions, vec![Named { name: "1.5.5".into(), value: 1 }]);
    }

    #[test]
    fn daily_aggregates_extend_the_series_and_the_range_cuts_old_days() {
        let c = db();
        c.execute(
            "INSERT INTO daily_players (day, razdor, platform, game, lang, installs) VALUES
               (50, '1.5.3', 'linux', 'base', 'en', 7),
               (50, '1.5.3', 'linux', 'base', 'ru', 2),
               (50, '1.5.4', 'windows', 'base', 'ru', 1)",
            [],
        )
        .expect("aggregates");
        ping(&c, "a", 200, "1.5.5", "linux", 0);
        let all = days(&c, 0).expect("all");
        assert_eq!(all.iter().map(|d| d.day.clone()).collect::<Vec<_>>(), vec![ymd(50), ymd(200)]);
        // Two rows of one day and version (two languages) add up.
        assert_eq!(all[0].versions, vec![Named { name: "1.5.4".into(), value: 1 }, Named { name: "1.5.3".into(), value: 9 }]);
        assert_eq!(all[0].platforms, vec![Named { name: "linux".into(), value: 9 }, Named { name: "windows".into(), value: 1 }]);
        let recent = days(&c, 150).expect("range");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].day, ymd(200));
    }

    #[test]
    fn section_escapes_player_text_and_shows_shares() {
        let days = vec![AdoptionDay {
            day: "2026-11-20".into(),
            versions: vec![
                Named { name: "<img src=x>".into(), value: 3 },
                Named { name: "1.5.4".into(), value: 1 },
            ],
            platforms: vec![Named { name: "linux".into(), value: 4 }],
        }];
        let html = section(&days);
        assert!(!html.contains("<img src=x>"));
        assert!(html.contains("&lt;img src=x&gt;"));
        assert!(html.contains("75%"));
        assert!(html.contains("id=\"adoption\""));
        assert!(section(&[]).contains("No data yet."));
    }
}
