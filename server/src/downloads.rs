//! Download counters (the stats plan, items 3 and 4; the owner's answers of 2026-11-24). Two sources, both
//! kept as daily totals in `stats.sqlite`:
//!
//! - the own `/hub/` log (`hub.log*` in `hub_log_dir`, written by nginx with no client address and no user
//!   agent): folded once a day into `daily_hub` (day, file, country), replacing the days it covers, so a
//!   re-run never counts twice. A download's size is whole files: bytes sent divided by the file's size
//!   (the log has the size: `Content-Range` or `Content-Length`);
//! - GitHub's release asset counters, polled once a day (two anonymous API calls): the difference from the
//!   last poll is the day's downloads (`daily_github`).
//!
//! The daily job runs inside the service (`spawn`), like the retention job.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::civil::{day_of, ymd};
use crate::geo;
use crate::http::ApiError;
use crate::state::AppState;
use crate::summary::Named;

/// The release repositories polled on GitHub (owner/name).
pub const REPOS: [&str; 2] = ["indicozy/razdor", "indicozy/razdor-community"];

/// The longest file path the fold keeps (a longer one is a bad request, not a file).
const MAX_FILE_LEN: usize = 300;

// ---------------------------------------------------------------------------------------------------------
// The own log
// ---------------------------------------------------------------------------------------------------------

/// One download line of the `/hub/` log: the file, its status, its bytes and its size, and the country.
#[derive(Debug, PartialEq)]
pub struct Hit {
    pub day: i64,
    pub file: String,
    pub status: u16,
    pub bytes: u64,
    /// The whole file's size (0 when the log has none).
    pub total: u64,
    pub country: String,
}

/// Totals of one (day, file, country).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Agg {
    pub requests: u64,
    pub bytes: u64,
    pub whole: f64,
}

/// Splits one log line into its fields: a bare token ends at a space, a quoted one at its closing quote
/// (nginx's `escape=json` writes `\"` and `\\` inside quotes).
fn fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let Some(&first) = chars.peek() else { break };
        let mut cur = String::new();
        if first == '"' {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => {
                        if let Some(n) = chars.next() {
                            cur.push(n);
                        }
                    }
                    _ => cur.push(c),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                cur.push(c);
                chars.next();
            }
        }
        out.push(cur);
    }
    out
}

/// The file's path under `/hub/`, or `None` for a request that is not a file of the mirror (our own
/// `status.json` checks (any folder's), other paths, a path that is too long or climbs out of the folder).
fn file_of(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("/hub/")?;
    if rest.is_empty() || rest.ends_with("status.json") || rest.len() > MAX_FILE_LEN || rest.contains("..") {
        return None;
    }
    Some(rest.to_string())
}

/// The whole file's size from `Content-Range: bytes a-b/total` ("" or no total: 0).
fn total_of_range(range: &str) -> u64 {
    range.rsplit_once('/').and_then(|(_, t)| t.trim().parse().ok()).unwrap_or(0)
}

/// A country code the log may carry: two capital letters, or "ZZ" (nginx's default for no country).
fn country_ok(code: &str) -> bool {
    code == "ZZ" || (code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase()))
}

/// Parses one line written with the `razdor_hub` format:
/// `$msec "$uri" $status $body_bytes_sent "$sent_http_content_range" "$sent_http_content_length" $razdor_country`.
/// Only downloads count: a 200 (a whole file) or a 206 (a Range piece). Anything else, or a malformed line,
/// gives `None`.
pub fn parse_line(line: &str) -> Option<Hit> {
    let f = fields(line);
    if f.len() != 7 {
        return None;
    }
    let msec: f64 = f[0].parse().ok()?;
    if !msec.is_finite() || msec < 0.0 {
        return None;
    }
    let day = (msec / 86_400.0).floor() as i64;
    let file = file_of(&f[1])?;
    let status: u16 = f[2].parse().ok()?;
    if status != 200 && status != 206 {
        return None;
    }
    let bytes: u64 = f[3].parse().ok()?;
    let total = if status == 206 { total_of_range(&f[4]) } else { f[5].trim().parse().unwrap_or(0) };
    let country = if country_ok(&f[6]) { f[6].clone() } else { geo::UNKNOWN.to_string() };
    Some(Hit { day, file, status, bytes, total, country })
}

/// Adds one line to the totals, for complete days only (`day < today`): today's file is still growing.
pub fn fold_line(map: &mut BTreeMap<(i64, String, String), Agg>, line: &str, today: i64) {
    let Some(hit) = parse_line(line) else { return };
    if hit.day >= today {
        return;
    }
    let agg = map.entry((hit.day, hit.file, hit.country)).or_default();
    agg.requests += 1;
    agg.bytes += hit.bytes;
    if hit.total > 0 {
        agg.whole += hit.bytes as f64 / hit.total as f64;
    }
}

/// The log file names the fold reads: `hub.log` and its rotated copies `hub.log.1`, `hub.log.2`, ...
/// (uncompressed: the logrotate of the host keeps them plain).
pub fn is_log_name(name: &str) -> bool {
    match name.strip_prefix("hub.log") {
        Some("") => true,
        Some(rest) => rest.strip_prefix('.').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
        None => false,
    }
}

/// Folds every log file of `dir` for the complete days before `today`. A line that is not valid UTF-8
/// is skipped (lossy), never a failure.
pub fn fold_dir(dir: &Path, today: i64) -> std::io::Result<BTreeMap<(i64, String, String), Agg>> {
    let mut map = BTreeMap::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !is_log_name(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let mut reader = BufReader::new(File::open(entry.path())?);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            if reader.read_until(b'\n', &mut buf)? == 0 {
                break;
            }
            let line = String::from_utf8_lossy(&buf);
            fold_line(&mut map, line.trim_end(), today);
        }
    }
    Ok(map)
}

/// Stores the folded days. Each day that the totals cover is replaced whole, so folding again changes
/// nothing. Returns the number of rows written.
pub fn store_hub(c: &mut Connection, rows: &BTreeMap<(i64, String, String), Agg>) -> rusqlite::Result<usize> {
    let tx = c.transaction()?;
    let mut days: Vec<i64> = rows.keys().map(|(d, _, _)| *d).collect();
    days.dedup();
    for day in &days {
        tx.execute("DELETE FROM daily_hub WHERE day = ?1", params![day])?;
    }
    let mut written = 0;
    for ((day, file, country), agg) in rows {
        tx.execute(
            "INSERT INTO daily_hub (day, file, country, requests, bytes, whole_files) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![day, file, country, agg.requests as i64, agg.bytes as i64, agg.whole],
        )?;
        written += 1;
    }
    tx.commit()?;
    Ok(written)
}

// ---------------------------------------------------------------------------------------------------------
// GitHub
// ---------------------------------------------------------------------------------------------------------

/// One release asset and its cumulative download count.
#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub repo: String,
    pub tag: String,
    pub name: String,
    pub count: u64,
}

/// The assets of a releases list (drafts and pre-releases are skipped, as the client does).
pub fn parse_releases(repo: &str, body: &serde_json::Value) -> Vec<Asset> {
    let mut out = Vec::new();
    let Some(releases) = body.as_array() else { return out };
    for r in releases {
        if r["draft"].as_bool().unwrap_or(false) || r["prerelease"].as_bool().unwrap_or(false) {
            continue;
        }
        let Some(tag) = r["tag_name"].as_str() else { continue };
        for a in r["assets"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let (Some(name), Some(count)) = (a["name"].as_str(), a["download_count"].as_u64()) else {
                continue;
            };
            out.push(Asset { repo: repo.to_string(), tag: tag.to_string(), name: name.to_string(), count });
        }
    }
    out
}

/// The downloads since the last poll, per asset: `(asset, delta)`. `prev` holds the last counts by
/// (repo, tag, name). In a repository's first poll (`baseline`) nothing is a day's download. A new asset
/// after that counts in full. A count below the last one (an asset replaced) gives 0 and becomes the new
/// baseline.
pub fn diff(prev: &BTreeMap<(String, String, String), u64>, now: &[Asset], baseline: bool) -> Vec<(Asset, u64)> {
    now.iter()
        .map(|a| {
            let key = (a.repo.clone(), a.tag.clone(), a.name.clone());
            let delta = match prev.get(&key) {
                Some(&p) if a.count >= p => a.count - p,
                Some(_) => 0,
                None if baseline => 0,
                None => a.count,
            };
            (a.clone(), delta)
        })
        .collect()
}

/// Stores one day's poll: the new counts, and the differences added to the day's GitHub total. Each repo's
/// first poll only sets its baseline. Returns the day's new downloads.
pub fn apply_poll(c: &mut Connection, day: i64, assets: &[Asset]) -> rusqlite::Result<u64> {
    let tx = c.transaction()?;
    let mut total = 0;
    for repo in REPOS {
        let mine: Vec<Asset> = assets.iter().filter(|a| a.repo == repo).cloned().collect();
        if mine.is_empty() {
            continue;
        }
        let prev: BTreeMap<(String, String, String), u64> = {
            let mut stmt = tx.prepare("SELECT tag, name, count FROM github_counts WHERE repo = ?1")?;
            let rows = stmt.query_map(params![repo], |r| {
                Ok(((repo.to_string(), r.get::<_, String>(0)?, r.get::<_, String>(1)?), r.get::<_, i64>(2)? as u64))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let baseline = prev.is_empty();
        for (a, delta) in diff(&prev, &mine, baseline) {
            if delta > 0 {
                tx.execute(
                    "INSERT INTO daily_github (day, repo, tag, name, downloads) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (day, repo, tag, name) DO UPDATE SET downloads = downloads + excluded.downloads",
                    params![day, a.repo, a.tag, a.name, delta as i64],
                )?;
                total += delta;
            }
            tx.execute(
                "INSERT INTO github_counts (repo, tag, name, count) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (repo, tag, name) DO UPDATE SET count = excluded.count",
                params![a.repo, a.tag, a.name, a.count as i64],
            )?;
        }
    }
    tx.commit()?;
    Ok(total)
}

/// Fetches one repository's releases (one API call; anonymous, 60 a hour per address, so one a day is
/// far below it). The error text never carries a client address (there is none here).
fn fetch_releases(repo: &str) -> Result<Vec<Asset>, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=100");
    let resp = ureq::get(&url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "razdor-server")
        .timeout(Duration::from_secs(60))
        .call()
        .map_err(|e| format!("{repo}: {e}"))?;
    let body: serde_json::Value = serde_json::from_reader(resp.into_reader()).map_err(|e| format!("{repo}: {e}"))?;
    Ok(parse_releases(repo, &body))
}

// ---------------------------------------------------------------------------------------------------------
// The daily job
// ---------------------------------------------------------------------------------------------------------

/// The platform of an asset name, as `tools/stats/downloads.py` maps it. Non-player files (checksums,
/// notes, the translation tool) are "other".
pub fn platform_of(name: &str) -> &'static str {
    let low = name.to_ascii_lowercase();
    if low.ends_with(".exe") {
        "windows"
    } else if low.ends_with(".apk") {
        "android"
    } else if low.ends_with(".ipa") {
        "ios"
    } else if low.contains("mac") || low.ends_with(".dmg") {
        "macos"
    } else if low == "razdor" || low.contains("linux") || [".tar.gz", ".tgz", ".appimage"].iter().any(|e| low.ends_with(e)) {
        "linux"
    } else {
        "other"
    }
}

/// Runs the day's fold and poll once. Each part logs its own failure and does not stop the other.
pub async fn run_once(st: &AppState) -> Result<(), ApiError> {
    let today = day_of(st.clock.now());
    if let Some(dir) = st.cfg.hub_log_dir.clone() {
        let folded = tokio::task::spawn_blocking(move || fold_dir(&dir, today)).await;
        match folded {
            Ok(Ok(rows)) => {
                let n = rows.len();
                let written = st.stats.call(move |c| Ok(store_hub(c, &rows)?)).await?;
                tracing::info!("downloads: own log folded, {n} day-file-country rows ({written} stored)");
            }
            Ok(Err(e)) => tracing::error!("downloads: reading the own log failed: {e}"),
            Err(e) => tracing::error!("downloads: fold task: {e}"),
        }
    }
    if st.cfg.github_poll {
        let fetched = tokio::task::spawn_blocking(|| {
            REPOS
                .iter()
                .filter_map(|repo| match fetch_releases(repo) {
                    Ok(assets) => Some(assets),
                    Err(e) => {
                        tracing::error!("downloads: GitHub poll failed: {e}");
                        None
                    }
                })
                .flatten()
                .collect::<Vec<Asset>>()
        })
        .await
        .map_err(|e| ApiError::Internal(format!("poll task: {e}")))?;
        if !fetched.is_empty() {
            let added = st.stats.call(move |c| Ok(apply_poll(c, today, &fetched)?)).await?;
            tracing::info!("downloads: GitHub poll, {added} new downloads since the last poll");
        }
    }
    Ok(())
}

/// Runs the job once a day, ten minutes after the service starts.
pub fn spawn(st: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(600)).await;
        loop {
            if let Err(e) = run_once(&st).await {
                tracing::error!("downloads job failed: {e:?}");
            }
            tokio::time::sleep(Duration::from_secs(86_400)).await;
        }
    });
}

// ---------------------------------------------------------------------------------------------------------
// Summary and the admin panel
// ---------------------------------------------------------------------------------------------------------

/// One file of the own mirror over the chart range.
#[derive(Debug, Serialize, PartialEq)]
pub struct FileRow {
    pub file: String,
    pub platform: String,
    pub requests: i64,
    pub bytes: i64,
    /// Whole-file equivalents (bytes sent divided by the file's size, summed).
    pub whole_files: f64,
}

/// One release asset on GitHub over the chart range.
#[derive(Debug, Serialize, PartialEq)]
pub struct AssetRow {
    pub repo: String,
    pub tag: String,
    pub name: String,
    pub platform: String,
    pub downloads: i64,
}

/// The download figures of the chart range.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Downloads {
    /// Whole files the own mirror sent, per day.
    pub own_per_day: Vec<Named>,
    /// GitHub downloads per day (the differences of the daily polls).
    pub github_per_day: Vec<Named>,
    /// The own mirror's files, most sent first.
    pub own_files: Vec<FileRow>,
    /// GitHub's assets, most downloaded first.
    pub github_assets: Vec<AssetRow>,
}

/// The chart range's download figures, from the two folds (`from` is the first day).
pub async fn downloads(st: &AppState, from: i64) -> Result<Downloads, ApiError> {
    st.stats
        .read(move |c| {
            let own_per_day = per_day(
                c,
                "SELECT day, ROUND(SUM(whole_files)) FROM daily_hub WHERE day >= ?1 GROUP BY day ORDER BY day",
                from,
            )?;
            let github_per_day = per_day(
                c,
                "SELECT day, SUM(downloads) FROM daily_github WHERE day >= ?1 GROUP BY day ORDER BY day",
                from,
            )?;
            let own_files = c
                .prepare(
                    "SELECT file, SUM(requests), SUM(bytes), SUM(whole_files) FROM daily_hub WHERE day >= ?1
                     GROUP BY file ORDER BY SUM(whole_files) DESC LIMIT 200",
                )?
                .query_map(params![from], |r| {
                    let file: String = r.get(0)?;
                    Ok(FileRow {
                        platform: platform_of(&file).to_string(),
                        file,
                        requests: r.get(1)?,
                        bytes: r.get(2)?,
                        whole_files: r.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let github_assets = c
                .prepare(
                    "SELECT repo, tag, name, SUM(downloads) FROM daily_github WHERE day >= ?1
                     GROUP BY repo, tag, name ORDER BY SUM(downloads) DESC LIMIT 200",
                )?
                .query_map(params![from], |r| {
                    let name: String = r.get(2)?;
                    Ok(AssetRow {
                        repo: r.get(0)?,
                        tag: r.get(1)?,
                        platform: platform_of(&name).to_string(),
                        name,
                        downloads: r.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(Downloads { own_per_day, github_per_day, own_files, github_assets })
        })
        .await
}

fn per_day(c: &Connection, sql: &str, from: i64) -> rusqlite::Result<Vec<Named>> {
    c.prepare(sql)?
        .query_map(params![from], |r| Ok(Named { name: ymd(r.get(0)?), value: r.get::<_, f64>(1)?.round() as i64 }))?
        .collect()
}

/// Installs per country over the chart range: the pings' installs of each day plus the folded days.
/// The numbers are install-days (one install counted on each day it pinged), not distinct installs.
pub async fn countries(st: &AppState, from: i64) -> Result<Vec<Named>, ApiError> {
    st.stats
        .read(move |c| {
            c.prepare(
                "SELECT country, SUM(n) FROM (
                   SELECT country, COUNT(DISTINCT install_id) AS n FROM pings
                   WHERE quarantined = 0 AND day >= ?1 GROUP BY country
                   UNION ALL
                   SELECT country, installs AS n FROM daily_country WHERE day >= ?1
                 ) GROUP BY country ORDER BY SUM(n) DESC LIMIT 60",
            )?
            .query_map(params![from], |r| Ok(Named { name: r.get(0)?, value: r.get(1)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(ApiError::from)
        })
        .await
}

/// The admin panel's section: installs per country, the own mirror and GitHub per day, and both per
/// platform. Every name from the log or from GitHub is escaped.
pub fn section_html(countries: &[Named], d: &Downloads) -> String {
    use crate::admin::esc;
    let mut b = String::from("<section id=\"downloads\"><h2>Countries and downloads</h2>");
    b.push_str("<p class=\"muted\">Installs per country: install-days in the chart range (the daily ping, with the country of its address; the address is dropped at once). Unknown: no country table, or an address it does not place.</p>");
    b.push_str("<table><thead><tr><th>Country</th><th class=\"num\">Install-days</th></tr></thead><tbody>");
    for c in countries {
        b.push_str(&format!("<tr><td>{}</td><td class=\"num\">{}</td></tr>", esc(&c.name), c.value));
    }
    b.push_str("</tbody></table>");

    b.push_str("<h3>Per platform (chart range)</h3><p class=\"muted\">GitHub: the day's differences of the release counters. Own: whole files the owner's mirror sent (a file's bytes over its size).</p>");
    b.push_str("<table><thead><tr><th>Platform</th><th class=\"num\">GitHub</th><th class=\"num\">Own mirror (files)</th></tr></thead><tbody>");
    for p in ["windows", "linux", "macos", "android", "ios", "other"] {
        let gh: i64 = d.github_assets.iter().filter(|a| a.platform == p).map(|a| a.downloads).sum();
        let own: f64 = d.own_files.iter().filter(|f| f.platform == p).map(|f| f.whole_files).sum();
        b.push_str(&format!(
            "<tr><td>{p}</td><td class=\"num\">{gh}</td><td class=\"num\">{}</td></tr>",
            own.round() as i64
        ));
    }
    b.push_str("</tbody></table>");

    b.push_str("<h3>Per day</h3><table><thead><tr><th>Day</th><th class=\"num\">GitHub</th><th class=\"num\">Own mirror (files)</th></tr></thead><tbody>");
    let mut days: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for n in &d.github_per_day {
        days.entry(n.name.clone()).or_default().0 = n.value;
    }
    for n in &d.own_per_day {
        days.entry(n.name.clone()).or_default().1 = n.value;
    }
    for (day, (gh, own)) in days.iter().rev() {
        b.push_str(&format!("<tr><td>{}</td><td class=\"num\">{gh}</td><td class=\"num\">{own}</td></tr>", esc(day)));
    }
    b.push_str("</tbody></table>");

    b.push_str("<h3>Release files (chart range)</h3><table><thead><tr><th>Source</th><th>File</th><th>Platform</th><th class=\"num\">GitHub</th><th class=\"num\">Own files</th><th class=\"num\">Own requests</th></tr></thead><tbody>");
    for a in d.github_assets.iter().take(60) {
        let own = d.own_files.iter().find(|f| f.file.ends_with(&a.name));
        b.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            esc(&a.repo),
            esc(&a.name),
            a.platform,
            a.downloads,
            own.map_or(0, |f| f.whole_files.round() as i64),
            own.map_or(0, |f| f.requests),
        ));
    }
    b.push_str("</tbody></table></section>");
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 20_000;

    fn line(msec: f64, uri: &str, status: u16, bytes: u64, range: &str, length: &str, country: &str) -> String {
        format!("{msec:.3} \"{uri}\" {status} {bytes} \"{range}\" \"{length}\" {country}")
    }

    fn at(day: i64) -> f64 {
        day as f64 * 86_400.0 + 3_600.0
    }

    #[test]
    fn parses_a_whole_file_and_a_range_piece() {
        let whole = parse_line(&line(at(DAY), "/hub/indicozy/razdor/releases/download/v1.6.0/a.apk", 200, 1000, "", "1000", "RU"))
            .expect("a whole file");
        assert_eq!(whole.day, DAY);
        assert_eq!(whole.file, "indicozy/razdor/releases/download/v1.6.0/a.apk");
        assert_eq!((whole.status, whole.bytes, whole.total, whole.country.as_str()), (200, 1000, 1000, "RU"));

        let piece = parse_line(&line(at(DAY), "/hub/x/y/z", 206, 250, "bytes 0-249/1000", "250", "DE")).expect("a piece");
        assert_eq!((piece.status, piece.total), (206, 1000), "the size comes from Content-Range");
    }

    #[test]
    fn skips_what_is_not_a_download() {
        let at_day = at(DAY);
        assert!(parse_line(&line(at_day, "/hub/pages/razdor/status.json", 200, 90, "", "90", "RU")).is_none(), "own checks");
        assert!(parse_line(&line(at_day, "/hub/status.json", 200, 90, "", "90", "RU")).is_none(), "the status file");
        assert!(parse_line(&line(at_day, "/hub/x/index-v2.json", 404, 0, "", "", "RU")).is_none(), "a 404 has no file");
        assert!(parse_line(&line(at_day, "/hub/x/y/304", 304, 0, "", "", "RU")).is_none(), "a 304 sends no file");
        assert!(parse_line(&line(at_day, "/api/v1/ping", 200, 9, "", "9", "RU")).is_none(), "not the mirror");
        assert!(parse_line(&line(at_day, "/hub/../etc/passwd", 200, 9, "", "9", "RU")).is_none(), "no climbing");
        assert!(parse_line("garbage").is_none());
        assert!(parse_line("nan \"/hub/a\" 200 1 \"\" \"1\" RU").is_none(), "a bad time");
    }

    #[test]
    fn an_unknown_or_odd_country_is_unknown() {
        let hit = parse_line(&line(at(DAY), "/hub/a/b", 200, 1, "", "1", "-")).expect("a line");
        assert_eq!(hit.country, geo::UNKNOWN);
        let zz = parse_line(&line(at(DAY), "/hub/a/b", 200, 1, "", "1", "ZZ")).expect("a line");
        assert_eq!(zz.country, "ZZ");
    }

    #[test]
    fn a_quoted_field_may_hold_an_escaped_quote() {
        let f = fields(r#"1 "a\"b c" 200"#);
        assert_eq!(f, vec!["1", "a\"b c", "200"]);
    }

    #[test]
    fn whole_files_add_up_across_range_pieces_and_skip_today() {
        let mut map = BTreeMap::new();
        // Four pieces of a 1000-byte file, and one whole download of 500 bytes of another file, all yesterday.
        for _ in 0..4 {
            fold_line(&mut map, &line(at(DAY), "/hub/x/f", 206, 250, "bytes 0-249/1000", "250", "RU"), DAY + 1);
        }
        fold_line(&mut map, &line(at(DAY), "/hub/x/g", 200, 500, "", "500", "RU"), DAY + 1);
        // A line of today is not folded yet.
        fold_line(&mut map, &line(at(DAY + 1), "/hub/x/f", 200, 1000, "", "1000", "RU"), DAY + 1);
        let f = map[&(DAY, "x/f".to_string(), "RU".to_string())];
        assert_eq!(f.requests, 4);
        assert_eq!(f.bytes, 1000);
        assert!((f.whole - 1.0).abs() < 1e-9, "four pieces of one file are one whole file");
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn the_log_file_names_are_the_plain_ones() {
        assert!(is_log_name("hub.log"));
        assert!(is_log_name("hub.log.1"));
        assert!(is_log_name("hub.log.14"));
        assert!(!is_log_name("hub.log.gz"));
        assert!(!is_log_name("razdor-api.log"));
        assert!(!is_log_name("hub.log."));
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-hub-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp folder");
        dir
    }

    #[test]
    fn folding_a_folder_twice_stores_each_day_once() {
        let dir = scratch("twice");
        let yesterday = [
            line(at(DAY), "/hub/x/f", 200, 1000, "", "1000", "RU"),
            "junk line that is not a log".to_string(),
        ];
        std::fs::write(dir.join("hub.log.1"), yesterday.join("\n") + "\n").expect("writes");
        std::fs::write(dir.join("hub.log"), line(at(DAY + 1), "/hub/x/f", 200, 1000, "", "1000", "DE") + "\n").expect("writes");
        std::fs::write(dir.join("other.log"), line(at(DAY), "/hub/x/f", 200, 1000, "", "1000", "RU") + "\n").expect("ignored");

        let mut db = Connection::open_in_memory().expect("a database");
        db.execute_batch(crate::schema::STATS).expect("the schema");
        let today = DAY + 2;
        let rows = fold_dir(&dir, today).expect("folds");
        assert_eq!(rows.len(), 2, "yesterday's RU row and the day after's DE row");
        store_hub(&mut db, &rows).expect("stores");
        // A second run over the same files, or a rotation that moved the file, changes nothing.
        let again = fold_dir(&dir, today).expect("folds");
        store_hub(&mut db, &again).expect("stores");
        let total: f64 = db.query_row("SELECT SUM(whole_files) FROM daily_hub", [], |r| r.get(0)).expect("sums");
        assert!((total - 2.0).abs() < 1e-9, "each day counted once, got {total}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn asset(tag: &str, name: &str, count: u64) -> Asset {
        Asset { repo: "indicozy/razdor".into(), tag: tag.into(), name: name.into(), count }
    }

    fn prev_of(list: &[Asset]) -> BTreeMap<(String, String, String), u64> {
        list.iter().map(|a| ((a.repo.clone(), a.tag.clone(), a.name.clone()), a.count)).collect()
    }

    #[test]
    fn the_first_poll_is_a_baseline_and_a_new_asset_counts_in_full() {
        let now = [asset("v1", "a.apk", 40)];
        let first = diff(&BTreeMap::new(), &now, true);
        assert_eq!(first[0].1, 0, "a baseline counts nothing");
        let later = [asset("v1", "a.apk", 45), asset("v1", "b.exe", 7)];
        let d = diff(&prev_of(&now), &later, false);
        assert_eq!(d[0].1, 5, "the difference");
        assert_eq!(d[1].1, 7, "an asset new since the baseline counts in full");
    }

    #[test]
    fn an_unchanged_or_lower_count_gives_no_downloads() {
        let before = [asset("v1", "a.apk", 45)];
        assert_eq!(diff(&prev_of(&before), &[asset("v1", "a.apk", 45)], false)[0].1, 0);
        let replaced = [asset("v1", "a.apk", 3)];
        assert_eq!(diff(&prev_of(&before), &replaced, false)[0].1, 0, "a re-upload resets the baseline");
    }

    #[test]
    fn a_release_list_skips_drafts_and_pre_releases() {
        let body = serde_json::json!([
            {"tag_name": "v1", "draft": false, "prerelease": false,
             "assets": [{"name": "a.apk", "download_count": 9}, {"name": "no-count"}]},
            {"tag_name": "v2-draft", "draft": true, "prerelease": false, "assets": [{"name": "x", "download_count": 1}]},
            {"tag_name": "v3-pre", "draft": false, "prerelease": true, "assets": [{"name": "y", "download_count": 1}]}
        ]);
        let got = parse_releases("indicozy/razdor", &body);
        assert_eq!(got, vec![asset("v1", "a.apk", 9)]);
    }

    #[test]
    fn a_poll_stores_one_day_of_differences() {
        let mut db = Connection::open_in_memory().expect("a database");
        db.execute_batch(crate::schema::STATS).expect("the schema");
        let first = [asset("v1", "a.apk", 40)];
        assert_eq!(apply_poll(&mut db, DAY, &first).expect("polls"), 0, "the baseline");
        let second = [asset("v1", "a.apk", 52)];
        assert_eq!(apply_poll(&mut db, DAY + 1, &second).expect("polls"), 12);
        // A second poll on the same day adds only what came since the last one.
        let third = [asset("v1", "a.apk", 55)];
        assert_eq!(apply_poll(&mut db, DAY + 1, &third).expect("polls"), 3);
        let on_day: i64 = db
            .query_row("SELECT downloads FROM daily_github WHERE day = ?1", params![DAY + 1], |r| r.get(0))
            .expect("the day's row");
        assert_eq!(on_day, 15);
    }

    #[test]
    fn platforms_follow_the_tool() {
        assert_eq!(platform_of("razdor-1.6.0.apk"), "android");
        assert_eq!(platform_of("Razdor.exe"), "windows");
        assert_eq!(platform_of("razdor-1.6.0-linux"), "linux");
        assert_eq!(platform_of("razdor-macos.dmg"), "macos");
        assert_eq!(platform_of("SHA256SUMS"), "other");
    }
}
