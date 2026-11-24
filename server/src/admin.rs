//! The admin dashboard: a password (argon2) for the browser, a cookie session of 12 hours, and the
//! admin token (its SHA-256 in the config) for the API. Server-rendered HTML with inline SVG charts;
//! every value from a player (a panic text, a GPU name, a map name) is escaped.

use std::fmt::Write as _;

use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;

use crate::abuse;
use crate::analytics::ip_of;
use crate::civil::day_of;
use crate::http::{json, ApiError, Peer};
use crate::state::{sha256_hex, AppState};
use crate::summary::{self, FpsRow, Named, Release, Summary};

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

/// The page skeleton: the login page, the dashboard and the preview example.
pub fn page_html(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"robots\" content=\"noindex\">\
         <title>{title}</title><style>{CSS}</style></head><body>{body}</body></html>"
    )
}

pub fn page(status: StatusCode, title: &str, body: &str) -> Response {
    (status, Html(page_html(title, body))).into_response()
}

const CSS: &str = "\
:root{--bg:#fafaf8;--fg:#1d1f23;--muted:#5d6068;--line:#e2e2dc;--head:#efefe8;--card:#ffffff;\
--accent:#2f6185;--good:#166534;--bad:#b42318;--warn:#8a3a0c;--good-bg:#dcfce7;--bad-bg:#fde2e1;--warn-bg:#fdf0c4;\
--grid:#e3e3dd;--s1:#3b6e8f;--s2:#d08c2e;--s3:#5b9a5e;--s4:#9b5fa8;--s5:#c4553f;--s6:#a6a69f}\
@media (prefers-color-scheme: dark){:root:not([data-theme=\"light\"]){--bg:#15171b;--fg:#e7e7e2;--muted:#a3a7af;\
--line:#33363d;--head:#23262c;--card:#1c1f25;--accent:#8cc0e6;--good:#86efac;--bad:#fca5a5;--warn:#fcd34d;\
--good-bg:#14452a;--bad-bg:#5c1d1d;--warn-bg:#4a3a10;--grid:#2c2f36;--s1:#7fb3d5;--s2:#e0a94f;--s3:#7fc282;\
--s4:#c79ad8;--s5:#e88a79;--s6:#8d9099}}\
body{font:14px/1.45 system-ui,sans-serif;margin:0 auto;max-width:1100px;padding:16px;color:var(--fg);background:var(--bg)}\
h1{font-size:20px;margin:4px 0}h2{font-size:17px;margin:30px 0 6px;border-bottom:1px solid var(--line);padding-bottom:4px}\
h3{font-size:14px;margin:18px 0 4px}a{color:var(--accent)}\
.head{display:flex;flex-wrap:wrap;align-items:center;gap:6px 14px}.badges{display:flex;flex-wrap:wrap;gap:6px}\
.badge{display:inline-flex;align-items:center;min-height:28px;padding:2px 10px;border-radius:999px;font-size:12px;font-weight:600;text-decoration:none}\
.badge.good,.good{color:var(--good)}.badge.good{background:var(--good-bg)}\
.badge.warn,.warn{color:var(--warn)}.badge.warn{background:var(--warn-bg)}\
.badge.bad,.bad{color:var(--bad)}.badge.bad{background:var(--bad-bg)}\
.muted{color:var(--muted)}\
nav.menu{position:sticky;top:0;z-index:5;display:flex;gap:4px;overflow-x:auto;white-space:nowrap;background:var(--bg);\
border-bottom:1px solid var(--line);margin:12px 0 0;padding:2px 0}\
nav.menu a{display:inline-flex;align-items:center;min-height:40px;padding:0 10px;border-radius:6px;text-decoration:none}\
nav.menu a:hover{background:var(--head)}\
[id]{scroll-margin-top:56px}\
.cards{display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));gap:10px;margin-top:14px}\
.card{display:flex;flex-direction:column;gap:2px;background:var(--card);border:1px solid var(--line);border-radius:8px;padding:10px 12px;color:var(--fg);text-decoration:none}\
.card .lbl{font-size:12px;color:var(--muted)}.card .big{font-size:24px;font-weight:650;font-variant-numeric:tabular-nums}\
.card .sub{font-size:12px}\
table{border-collapse:collapse;width:100%;margin:6px 0}\
td,th{text-align:left;padding:6px 8px;border-bottom:1px solid var(--line);vertical-align:top}\
th{font-weight:600;background:var(--head)}td.n,th.n{text-align:right;font-variant-numeric:tabular-nums}\
th.sort{cursor:pointer;user-select:none;white-space:nowrap}th.sort .arr{font-size:11px;margin-left:4px}\
td.warm{background:var(--warn-bg)}td.hot{background:var(--bad-bg)}tr.slow td{background:var(--warn-bg)}\
.scroll{overflow-x:auto;max-width:100%;-webkit-overflow-scrolling:touch}\
.grid3{display:flex;gap:24px;flex-wrap:wrap}.grid3>div{flex:1;min-width:240px}\
.range{margin:2px 0 0;font-size:13px;color:var(--muted)}\
.range a{display:inline-flex;align-items:center;min-height:40px;padding:0 6px}\
.range a[aria-current=\"page\"]{color:var(--fg);font-weight:650}\
.chartwrap{position:relative;margin:4px 0}\
svg.chart{display:block;width:100%;max-width:760px;height:auto;background:var(--card);border:1px solid var(--line);border-radius:6px}\
svg .grid{stroke:var(--grid);stroke-width:1}svg .ax{fill:var(--muted);font-size:13px}\
svg .rel{stroke:var(--muted);stroke-width:1;stroke-dasharray:4 3}svg .relt{fill:var(--fg);font-size:12px}\
rect.s1{fill:var(--s1)}rect.s2{fill:var(--s2)}rect.s3{fill:var(--s3)}rect.s4{fill:var(--s4)}rect.s5{fill:var(--s5)}rect.s6{fill:var(--s6)}\
.legend{display:flex;flex-wrap:wrap;gap:4px 14px;list-style:none;padding:0;margin:6px 0;font-size:12px;color:var(--muted)}\
.legend i{display:inline-block;width:12px;height:12px;border-radius:2px;margin-right:5px;vertical-align:-1px}\
i.s1{background:var(--s1)}i.s2{background:var(--s2)}i.s3{background:var(--s3)}i.s4{background:var(--s4)}i.s5{background:var(--s5)}i.s6{background:var(--s6)}\
.tip{position:absolute;z-index:6;pointer-events:none;background:var(--fg);color:var(--bg);font-size:12px;padding:4px 8px;border-radius:4px;white-space:nowrap}\
[hidden]{display:none!important}\
code{font-size:12px;word-break:break-all}\
button,input{font:inherit;min-height:40px;padding:0 12px;border-radius:6px;border:1px solid var(--line);background:var(--card);color:var(--fg)}\
.filter{margin:8px 0}.filter input{width:min(320px,100%)}\
form.inline{display:inline}\
@media (max-width:640px){body{padding:12px}h1{font-size:18px}.cards{grid-template-columns:1fr 1fr}\
.card .big{font-size:20px}svg.chart{min-width:600px}table.wide{min-width:560px}}";

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

/// One piece of a column: its series (1 to 6, the colour class), its value and its hover text.
struct Seg {
    series: usize,
    value: i64,
    tip: String,
}

const CW: f64 = 760.0;
const CH: f64 = 220.0;
/// Plot margins: left for the y labels, top for the release labels, bottom for the x labels.
const PL: f64 = 46.0;
const PR: f64 = 8.0;
const PT: f64 = 20.0;
const PB: f64 = 24.0;

/// The top of the y axis: the largest value rounded up to 1, 2, 2.5, 5 or 10 times a power of ten.
fn nice_top(max: f64) -> f64 {
    let mag = 10f64.powf(max.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|t| *t >= max).unwrap_or(max)
}

fn axis_label(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v as i64)
    } else {
        format!("{v:.1}")
    }
}

/// A column chart as inline SVG: a y axis with gridlines, x labels, a dashed line with a label for
/// each mark (column index, text), and a hover text per piece (`<title>` and `data-tip` for the
/// script's tooltip). Wrapped so a phone can scroll it sideways.
fn columns(names: &[String], cols: &[Vec<Seg>], marks: &[(usize, String)], what: &str) -> String {
    if names.is_empty() {
        return "<p class=\"muted\">No data yet.</p>".to_string();
    }
    let totals = cols.iter().map(|c| c.iter().map(|s| s.value).sum::<i64>()).max().unwrap_or(0);
    let top = nice_top(totals.max(1) as f64);
    let ph = CH - PT - PB;
    let bw = (CW - PL - PR) / names.len() as f64;
    let y = |v: f64| PT + ph - v / top * ph;
    let mut s = format!("<svg class=\"chart\" viewBox=\"0 0 {CW} {CH}\" role=\"img\" aria-label=\"{}\">", esc(what));
    for v in [0.0, top / 2.0, top] {
        let yy = y(v);
        let _ = write!(
            s,
            "<line class=\"grid\" x1=\"{PL}\" x2=\"{}\" y1=\"{yy:.1}\" y2=\"{yy:.1}\"/>\
             <text class=\"ax\" x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\">{}</text>",
            CW - PR,
            PL - 6.0,
            yy + 4.0,
            axis_label(v)
        );
    }
    for (i, col) in cols.iter().enumerate() {
        let x = PL + i as f64 * bw + bw * 0.14;
        let mut acc = 0.0;
        for seg in col.iter().filter(|s| s.value > 0) {
            let lo = acc;
            acc += seg.value as f64;
            let (top_y, bottom_y) = (y(acc), y(lo));
            let tip = esc(&seg.tip);
            let _ = write!(
                s,
                "<rect class=\"s{}\" x=\"{x:.1}\" y=\"{top_y:.1}\" width=\"{:.1}\" height=\"{:.1}\" data-tip=\"{tip}\"><title>{tip}</title></rect>",
                seg.series,
                bw * 0.72,
                bottom_y - top_y
            );
        }
    }
    let step = (names.len() as f64 / 10.0).ceil().max(1.0) as usize;
    for (i, name) in names.iter().enumerate().step_by(step) {
        let label = name.get(5..).filter(|_| name.len() == 10).unwrap_or(name.as_str());
        let _ = write!(
            s,
            "<text class=\"ax\" x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>",
            PL + (i as f64 + 0.5) * bw,
            CH - 6.0,
            esc(label)
        );
    }
    for (i, label) in marks {
        let x = PL + (*i as f64 + 0.5) * bw;
        let _ = write!(
            s,
            "<line class=\"rel\" x1=\"{x:.1}\" x2=\"{x:.1}\" y1=\"{PT}\" y2=\"{:.1}\"/>\
             <text class=\"relt\" x=\"{:.1}\" y=\"{:.1}\">{}</text>",
            PT + ph,
            x + 3.0,
            PT - 6.0,
            esc(label)
        );
    }
    s.push_str("</svg>");
    format!("<div class=\"chartwrap\"><div class=\"scroll\">{s}</div><div class=\"tip\" hidden></div></div>")
}

/// One value per day, as a single-colour column chart.
fn daily(items: &[Named], unit: &str) -> (Vec<String>, Vec<Vec<Seg>>) {
    let names: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
    let cols: Vec<Vec<Seg>> = items
        .iter()
        .map(|i| vec![Seg { series: 1, value: i.value, tip: format!("{}: {} {unit}", i.name, i.value) }])
        .collect();
    (names, cols)
}

/// Releases that fall on a day in the chart: (column, version).
fn release_marks(names: &[String], releases: &[Release]) -> Vec<(usize, String)> {
    releases
        .iter()
        .filter_map(|r| names.iter().position(|d| *d == r.first_day).map(|i| (i, r.version.clone())))
        .collect()
}

/// Installs per version per day as stacked columns: the five biggest versions and the rest as "other".
fn versions_chart(sm: &Summary) -> String {
    let days: Vec<String> = sm.versions_per_day.iter().map(|v| v.day.clone()).collect::<BTreeSet<String>>().into_iter().collect();
    let mut totals: BTreeMap<&str, i64> = BTreeMap::new();
    for v in &sm.versions_per_day {
        *totals.entry(v.version.as_str()).or_insert(0) += v.installs;
    }
    let mut ranked: Vec<(&str, i64)> = totals.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let top: Vec<&str> = ranked.iter().take(5).map(|r| r.0).collect();
    let cols: Vec<Vec<Seg>> = days
        .iter()
        .map(|day| {
            let mut sums = [0i64; 7];
            for v in sm.versions_per_day.iter().filter(|v| v.day == *day) {
                let series = top.iter().position(|t| *t == v.version.as_str()).map_or(6, |i| i + 1);
                sums[series] += v.installs;
            }
            (1..=6)
                .filter(|s| sums[*s] > 0)
                .map(|s| {
                    let tip = if s < 6 {
                        format!("{day}, version {}: {}", top[s - 1], sums[s])
                    } else {
                        format!("{day}, other versions: {}", sums[s])
                    };
                    Seg { series: s, value: sums[s], tip }
                })
                .collect()
        })
        .collect();
    let mut legend = String::from("<ul class=\"legend\">");
    for (k, v) in top.iter().enumerate() {
        let _ = write!(legend, "<li><i class=\"s{}\"></i>{}</li>", k + 1, esc(v));
    }
    if ranked.len() > 5 {
        legend.push_str("<li><i class=\"s6\"></i>other</li>");
    }
    legend.push_str("</ul>");
    format!("{}{legend}", columns(&days, &cols, &[], "Installs per version per day"))
}

fn named_table(title: &str, rows: &[Named]) -> String {
    let mut s = format!("<div class=\"scroll\"><table><tr><th>{}</th><th class=\"n\">installs</th></tr>", esc(title));
    for r in rows {
        let _ = write!(s, "<tr><td>{}</td><td class=\"n\">{}</td></tr>", esc(&r.name), r.value);
    }
    s.push_str("</table></div>");
    s
}

/// A table header cell; a sortable one gets the arrow the script fills in.
fn th(label: &str, num: bool, sortable: bool) -> String {
    let class = match (sortable, num) {
        (true, true) => "sort n",
        (true, false) => "sort",
        (false, true) => "n",
        (false, false) => "",
    };
    let mut attrs = String::new();
    if sortable {
        attrs.push_str(" tabindex=\"0\"");
    }
    if num {
        attrs.push_str(" data-num=\"1\"");
    }
    let arrow = if sortable { "<span class=\"arr\"></span>" } else { "" };
    format!("<th class=\"{class}\"{attrs}>{}{arrow}</th>", esc(label))
}

/// A table head from (label, numeric, sortable) cells.
fn head(cols: &[(&str, bool, bool)]) -> String {
    let cells: String = cols.iter().map(|(label, num, sortable)| th(label, *num, *sortable)).collect();
    format!("<thead><tr>{cells}</tr></thead>")
}

fn pct(part: i64, whole: i64) -> String {
    if whole == 0 {
        "-".to_string()
    } else {
        format!("{:.0}%", part as f64 * 100.0 / whole as f64)
    }
}

fn mb(bytes: i64) -> f64 {
    bytes as f64 / 1_048_576.0
}

/// A week-over-week figure: the arrow and the change, coloured good or bad (the text carries the sign too).
fn delta(now: i64, prev: i64, up_is_bad: bool) -> String {
    let (arrow, level) = if now > prev {
        ("▲", if up_is_bad { "bad" } else { "good" })
    } else if now < prev {
        ("▼", if up_is_bad { "good" } else { "bad" })
    } else {
        ("=", "muted")
    };
    let text = if now == prev {
        "same as the 7 days before".to_string()
    } else if prev == 0 {
        "up from zero".to_string()
    } else {
        format!("{:.0}% vs the 7 days before", (now - prev).abs() as f64 * 100.0 / prev as f64)
    };
    format!("<span class=\"{level}\">{arrow} {text}</span>")
}

fn card(href: &str, label: &str, big: &str, sub_html: &str) -> String {
    format!(
        "<a class=\"card\" href=\"{href}\"><span class=\"lbl\">{}</span><span class=\"big\">{}</span><span class=\"sub\">{sub_html}</span></a>",
        esc(label),
        esc(big)
    )
}

fn badge(href: &str, level: &str, text: &str) -> String {
    format!("<a class=\"badge {level}\" href=\"{href}\">{}</a>", esc(text))
}

/// The health badges next to the heading: storage, free disk, crashes per 100 players, bans.
fn health(sm: &Summary) -> String {
    let s = &sm.storage;
    let used_pct = if s.max_bytes > 0 { s.used_bytes as f64 * 100.0 / s.max_bytes as f64 } else { 0.0 };
    let (level, text) = if s.over {
        ("bad", "Storage: over budget".to_string())
    } else if used_pct >= 80.0 {
        ("warn", format!("Storage: {used_pct:.0}% of budget"))
    } else {
        ("good", format!("Storage: OK ({used_pct:.0}%)"))
    };
    let mut out = badge("#admin", level, &text);
    let free_gb = s.free_bytes as f64 / 1e9;
    let (level, text) = if s.free_bytes < s.min_free_bytes {
        ("bad", format!("Free disk: below floor ({free_gb:.1} GB)"))
    } else if s.free_bytes < 2 * s.min_free_bytes {
        ("warn", format!("Free disk: low ({free_gb:.1} GB)"))
    } else {
        ("good", format!("Free disk: OK ({free_gb:.1} GB)"))
    };
    out.push_str(&badge("#admin", level, &text));
    let rate = |version: &str| {
        sm.crash_rates
            .iter()
            .find(|r| r.version == version && r.installs > 0)
            .map(|r| r.crashes as f64 * 100.0 / r.installs as f64)
    };
    if let (Some(now), Some(prev)) = (rate(&sm.latest_version), rate(&sm.previous_version)) {
        let (level, word) = if now > prev && now >= 2.0 * prev {
            ("bad", "2x up")
        } else if now > prev {
            ("warn", "up")
        } else {
            ("good", "OK")
        };
        out.push_str(&badge("#crashes", level, &format!("Crashes/100: {word} ({now:.1} vs {prev:.1})")));
    }
    let active = sm.bans.iter().filter(|b| b.active).count();
    if active > 0 {
        out.push_str(&badge("#admin", "warn", &format!("Bans: {active} active")));
    } else {
        out.push_str(&badge("#admin", "good", "Bans: none active"));
    }
    out
}

fn range_links(current: i64) -> String {
    let links: Vec<String> = [7, 30, 90]
        .iter()
        .map(|d| {
            let current_attr = if *d == current { " aria-current=\"page\"" } else { "" };
            format!("<a href=\"/admin?days={d}\"{current_attr}>{d} days</a>")
        })
        .collect();
    format!("<p class=\"range\">Range: {}</p>", links.join(" · "))
}

const MENU: &str = "<nav class=\"menu\" aria-label=\"Sections\">\
<a href=\"#overview\">Overview</a><a href=\"#players\">Players</a><a href=\"#adoption\">Adoption</a><a href=\"#maps\">Maps</a>\
<a href=\"#crashes\">Crashes</a><a href=\"#performance\">Performance</a><a href=\"#cloud\">Cloud</a>\
<a href=\"#admin\">Admin</a></nav>";

/// The dashboard's HTML, from the same numbers as the JSON. Works without the script; the script
/// only adds sorting, the filter, the tooltips and the copy buttons.
pub fn render(sm: &Summary) -> String {
    let days = sm.range_days;
    let ov = &sm.overview;
    let s = &sm.storage;
    let r = &sm.retention;
    let mut b = String::new();
    let _ = write!(b, "<div class=\"head\"><h1>Razdor stats</h1><div class=\"badges\">{}</div></div>", health(sm));
    let _ = write!(
        b,
        "<p class=\"muted\">Generated day {}. Raw rows: pings 90 days, events and performance 180 days, \
         crash reports 1 year (saves 90 days), battles forever. Quarantined rows are not counted.</p>",
        esc(&crate::civil::ymd(day_of(sm.generated_at)))
    );
    b.push_str(MENU);

    b.push_str("<div id=\"overview\" class=\"cards\">");
    b.push_str(&card(
        "#players",
        "Players, last 7 days",
        &ov.players_7d.to_string(),
        &delta(ov.players_7d, ov.players_prev_7d, false),
    ));
    b.push_str(&card(
        "#players",
        "New installs, last 7 days",
        &ov.new_installs_7d.to_string(),
        &delta(ov.new_installs_7d, ov.new_installs_prev_7d, false),
    ));
    b.push_str(&card(
        "#crashes",
        "Crashes, last 7 days",
        &ov.crashes_7d.to_string(),
        &delta(ov.crashes_7d, ov.crashes_prev_7d, true),
    ));
    b.push_str(&card(
        "#players",
        "Week-1 retention",
        &pct(r.week1_returned, r.week1_cohort),
        &format!("<span class=\"muted\">{} of {} came back</span>", r.week1_returned, r.week1_cohort),
    ));
    b.push_str(&card(
        "#cloud",
        "Cloud accounts",
        &sm.cloud.accounts.to_string(),
        &format!("<span class=\"muted\">{:.2} MB saved</span>", mb(sm.cloud.used_bytes)),
    ));
    b.push_str(&card(
        "#admin",
        "Storage, of budget",
        &pct(s.used_bytes as i64, s.max_bytes as i64),
        &format!(
            "<span class=\"muted\">{:.2} GB of {:.2} GB</span>",
            s.used_bytes as f64 / 1e9,
            s.max_bytes as f64 / 1e9
        ),
    ));
    b.push_str("</div>");

    // Players: the range switch, the daily chart with the releases, the week tables, versions, retention.
    let _ = write!(b, "<section id=\"players\"><h2>Players</h2>{}", range_links(days));
    let (day_names, day_cols) = daily(&sm.players_per_day, "players");
    let _ = write!(
        b,
        "<h3>Players per day (last {days} days)</h3>{}",
        columns(&day_names, &day_cols, &release_marks(&day_names, &sm.releases), "Players per day")
    );
    b.push_str("<h3>The last 7 days</h3><div class=\"grid3\">");
    let _ = write!(
        b,
        "<div>{}</div><div>{}</div><div>{}</div></div>",
        named_table("version", &sm.week_by_version),
        named_table("platform", &sm.week_by_platform),
        named_table("game", &sm.week_by_game)
    );
    let _ = write!(b, "<h3>Versions per day (last {days} days)</h3>{}", versions_chart(sm));
    if !sm.releases.is_empty() {
        let list: Vec<String> =
            sm.releases.iter().map(|rel| format!("{} (first seen {})", esc(&rel.version), esc(&rel.first_day))).collect();
        let _ = write!(b, "<p class=\"muted\">Releases: {}. Dashed lines on the charts mark them.</p>", list.join(", "));
    }
    let _ = write!(
        b,
        "<h3>Retention</h3><div class=\"scroll\"><table><tr><th>cohort</th><th class=\"n\">installs</th><th class=\"n\">came back</th><th class=\"n\">rate</th></tr>\
         <tr><td>week 1 (first ping 14-20 days ago, back 7-13 days later)</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>\
         <tr><td>week 4 (first ping 34-40 days ago, back 28-34 days later)</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr></table></div>",
        r.week1_cohort,
        r.week1_returned,
        pct(r.week1_returned, r.week1_cohort),
        r.week4_cohort,
        r.week4_returned,
        pct(r.week4_returned, r.week4_cohort)
    );
    b.push_str("</section>");

    // Maps: sortable and filterable; the filter and the sort need the script.
    b.push_str("<section id=\"maps\"><h2>Maps</h2><p class=\"muted\">Last 180 days. Win % and abandon % are of starts.</p>");
    b.push_str(
        "<div class=\"filter\" data-js hidden><input type=\"search\" data-filter=\"maps-table\" placeholder=\"Filter maps\" aria-label=\"Filter maps\"></div>",
    );
    let _ = write!(
        b,
        "<div class=\"scroll\"><table class=\"sortable wide\" id=\"maps-table\">{}<tbody>",
        head(&[
            ("map", false, true),
            ("starts", true, true),
            ("wins", true, true),
            ("losses", true, true),
            ("abandoned", true, true),
            ("win %", true, true),
            ("abandon %", true, true),
            ("abandoned at (top screens)", false, false),
        ])
    );
    for m in &sm.maps {
        let at: Vec<String> = m.abandoned_at.iter().map(|a| format!("{} ({})", esc(&a.name), a.value)).collect();
        let abandon = if m.starts > 0 { m.abandoned as f64 * 100.0 / m.starts as f64 } else { 0.0 };
        let heat = if m.starts == 0 {
            ""
        } else if abandon >= 25.0 {
            " hot"
        } else if abandon >= 10.0 {
            " warm"
        } else {
            ""
        };
        let _ = write!(
            b,
            "<tr><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n{heat}\">{}</td><td>{}</td></tr>",
            esc(&m.map),
            m.starts,
            m.wins,
            m.losses,
            m.abandoned,
            pct(m.wins, m.starts),
            pct(m.abandoned, m.starts),
            at.join(", ")
        );
    }
    b.push_str("</tbody></table></div>");
    b.push_str("<h3>Experiments (map starts with each switch on)</h3>");
    b.push_str(&named_table("experiment", &sm.experiments));
    b.push_str("</section>");

    // Adoption: installs per version and per platform, per day (the daily ping).
    b.push_str(&crate::adoption::section(&sm.adoption));

    // Crashes: the groups, the new ones marked, and the crash rate per version.
    b.push_str("<section id=\"crashes\"><h2>Crashes (last 30 days, grouped by location and message)</h2>");
    b.push_str("<div class=\"scroll\"><table class=\"wide\">");
    b.push_str(&head(&[
        ("count", true, false),
        ("location", false, false),
        ("message", false, false),
        ("last", false, false),
        ("version", false, false),
        ("status", false, false),
        ("", false, false),
    ]));
    b.push_str("<tbody>");
    for c in &sm.crashes {
        let status = if c.is_new { "<span class=\"badge bad\">new</span> " } else { "" };
        let copy = format!(
            "Crash: {} — {} ({}×, last {}, version {}, first seen {})",
            c.location, c.message, c.count, c.last_day, c.last_version, c.first_version
        );
        let _ = write!(
            b,
            "<tr><td class=\"n\">{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{status}<span class=\"muted\">first seen {}</span></td>\
             <td><a href=\"{}\">Reports</a> <button type=\"button\" data-js hidden data-copy=\"{}\">Copy</button></td></tr>",
            c.count,
            esc(&c.location),
            esc(&c.message),
            esc(&c.last_day),
            esc(&c.last_version),
            esc(&c.first_version),
            esc(&crate::crash_view::group_href(&c.location, &c.message)),
            esc(&copy)
        );
    }
    b.push_str("</tbody></table></div>");
    b.push_str("<h3>Crashes per 100 players by version</h3><div class=\"scroll\"><table class=\"sortable wide\" id=\"rates-table\">");
    b.push_str(&head(&[
        ("version", false, true),
        ("crashes", true, true),
        ("installs", true, true),
        ("per 100 players", true, true),
    ]));
    b.push_str("<tbody>");
    for rate in &sm.crash_rates {
        let per = if rate.installs > 0 {
            format!("{:.2}", rate.crashes as f64 * 100.0 / rate.installs as f64)
        } else {
            "-".to_string()
        };
        let _ = write!(
            b,
            "<tr><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{per}</td></tr>",
            esc(&rate.version),
            rate.crashes,
            rate.installs
        );
    }
    b.push_str("</tbody></table></div></section>");

    // Performance: sessions first; slow rows are tinted and labelled.
    b.push_str("<section id=\"performance\"><h2>Frames by GPU and phone</h2>");
    b.push_str("<p class=\"muted\">Sorted by sessions. Rows with a median FPS under 30, or a p99 over 50 ms, are marked slow.</p>");
    b.push_str("<div class=\"scroll\"><table class=\"sortable wide\" id=\"fps-table\">");
    b.push_str(&head(&[
        ("GPU", false, true),
        ("phone", false, true),
        ("sessions", true, true),
        ("median ms", true, true),
        ("p99 ms", true, true),
        ("FPS (median)", true, true),
        ("flag", false, true),
    ]));
    b.push_str("<tbody>");
    let mut fps: Vec<&FpsRow> = sm.fps.iter().collect();
    fps.sort_by(|x, y| y.sessions.cmp(&x.sessions));
    for f in fps {
        let fps_now = if f.median_ms > 0.0 { 1000.0 / f.median_ms } else { 0.0 };
        let slow = (f.median_ms > 0.0 && fps_now < 30.0) || f.p99_ms > 50.0;
        let _ = write!(
            b,
            "<tr{}><td>{}</td><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{:.1}</td><td class=\"n\">{:.1}</td><td class=\"n\">{:.0}</td><td>{}</td></tr>",
            if slow { " class=\"slow\"" } else { "" },
            esc(&f.gpu),
            esc(&f.phone),
            f.sessions,
            f.median_ms,
            f.p99_ms,
            fps_now,
            if slow { "<span class=\"badge warn\">slow</span>" } else { "" }
        );
    }
    b.push_str("</tbody></table></div></section>");

    // Cloud.
    let (sync_names, sync_cols) = daily(&sm.cloud.syncs_per_day, "syncs");
    let _ = write!(
        b,
        "<section id=\"cloud\"><h2>Cloud</h2>\
         <p>Accounts: <b>{}</b>. Saved on the server: <b>{:.2} MB</b>.</p>\
         <h3>Syncs per day (last {days} days)</h3>\
         <p class=\"muted\">Saves, marks, settings, hub, bench, links and accounts.</p>{}</section>",
        sm.cloud.accounts,
        mb(sm.cloud.used_bytes),
        columns(&sync_names, &sync_cols, &[], "Cloud syncs per day")
    );

    // Admin: storage, bans, the links and logout.
    b.push_str(&crate::downloads::section_html(&sm.countries, &sm.downloads));
    b.push_str("<section id=\"admin\"><h2>Admin</h2><h3>Storage</h3>");
    let state = if s.over {
        "<span class=\"bad\">over the budget: no new uploads</span>"
    } else {
        "<span class=\"muted\">within the budget</span>"
    };
    let _ = write!(
        b,
        "<p>Stored files: <b>{:.2} GB</b> of <b>{:.2} GB</b>. Free on the disk: <b>{:.2} GB</b> (floor {:.2} GB). {}</p>",
        s.used_bytes as f64 / 1e9,
        s.max_bytes as f64 / 1e9,
        s.free_bytes as f64 / 1e9,
        s.min_free_bytes as f64 / 1e9,
        state
    );
    b.push_str("<h3>Bans</h3><div class=\"scroll\"><table class=\"wide\"><tr><th>kind</th><th>key</th><th class=\"n\">strikes</th><th>since</th><th>reason</th><th>state</th><th></th></tr>");
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
    b.push_str("</table></div>");
    b.push_str("<p class=\"muted\"><a href=\"/admin/api/summary.json\">summary.json</a> (the same numbers; admin token or this session). ");
    b.push_str("<form class=\"inline\" method=\"post\" action=\"/admin/logout\"><button>Log out</button></form></p></section>");
    b.push_str(SCRIPT);
    b
}

/// The dashboard's small script: sorting, the maps filter, the chart tooltips and the copy buttons.
/// Every control it touches is shown only by the script (`data-js`, `hidden`), so without it the
/// page is the same tables, unsorted.
const SCRIPT: &str = r#"<script>
(function () {
  'use strict';
  function all(sel, root) { return Array.prototype.slice.call((root || document).querySelectorAll(sel)); }
  all('[data-js]').forEach(function (el) { el.hidden = false; });
  // Phones: a chart wider than the screen opens on its newest days.
  all('.chartwrap .scroll').forEach(function (el) { el.scrollLeft = el.scrollWidth; });

  function hideTips() { all('.tip').forEach(function (t) { t.hidden = true; }); }
  function showTip(el) {
    var wrap = el.closest('.chartwrap');
    if (!wrap) return;
    hideTips();
    var tip = wrap.querySelector('.tip');
    tip.textContent = el.getAttribute('data-tip');
    tip.hidden = false;
    var r = el.getBoundingClientRect(), w = wrap.getBoundingClientRect();
    var x = r.left + r.width / 2 - w.left - tip.offsetWidth / 2;
    x = Math.max(0, Math.min(x, w.width - tip.offsetWidth));
    tip.style.left = x + 'px';
    tip.style.top = Math.max(0, r.top - w.top - tip.offsetHeight - 4) + 'px';
  }
  document.addEventListener('pointerover', function (e) {
    var el = e.target.closest && e.target.closest('[data-tip]');
    if (el && e.pointerType === 'mouse') showTip(el);
  });
  document.addEventListener('pointerout', function (e) {
    var el = e.target.closest && e.target.closest('[data-tip]');
    if (el && e.pointerType === 'mouse') hideTips();
  });

  function copyText(btn) {
    function done(ok) {
      btn.textContent = ok ? 'Copied' : 'Copy failed';
      setTimeout(function () { btn.textContent = 'Copy'; }, 1500);
    }
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(btn.getAttribute('data-copy')).then(function () { done(true); }, function () { done(false); });
    } else {
      done(false);
    }
  }

  function keyOf(cell, num) {
    var t = cell.textContent.trim();
    if (!num) return t.toLowerCase();
    var n = parseFloat(t.replace(/,/g, '').replace(/[^0-9.\-]/g, ''));
    return isNaN(n) ? -Infinity : n;
  }
  function sortOn(th) {
    var table = th.closest('table');
    var idx = th.cellIndex;
    var num = th.getAttribute('data-num') === '1';
    var prev = th.getAttribute('data-dir');
    var dir = prev ? (prev === 'asc' ? 'desc' : 'asc') : (num ? 'desc' : 'asc');
    all('th', table.tHead).forEach(function (h) {
      h.removeAttribute('data-dir');
      var a = h.querySelector('.arr');
      if (a) a.textContent = '';
    });
    th.setAttribute('data-dir', dir);
    var arr = th.querySelector('.arr');
    if (arr) arr.textContent = dir === 'asc' ? '▲' : '▼';
    var body = table.tBodies[0];
    var rows = all('tr', body);
    rows.sort(function (a, b) {
      var x = keyOf(a.cells[idx], num), y = keyOf(b.cells[idx], num);
      var c = x < y ? -1 : (x > y ? 1 : 0);
      return dir === 'asc' ? c : -c;
    });
    rows.forEach(function (r) { body.appendChild(r); });
  }

  document.addEventListener('click', function (e) {
    var copy = e.target.closest && e.target.closest('button[data-copy]');
    if (copy) { copyText(copy); return; }
    var th = e.target.closest && e.target.closest('table.sortable th');
    if (th) { sortOn(th); return; }
    var el = e.target.closest && e.target.closest('[data-tip]');
    if (el) showTip(el); else hideTips();
  });
  document.addEventListener('keydown', function (e) {
    if (e.key === 'Enter' && e.target.matches && e.target.matches('table.sortable th')) sortOn(e.target);
  });
  document.addEventListener('input', function (e) {
    var id = e.target.getAttribute && e.target.getAttribute('data-filter');
    if (!id) return;
    var q = e.target.value.trim().toLowerCase();
    all('tbody tr', document.getElementById(id)).forEach(function (r) {
      r.hidden = q !== '' && r.textContent.toLowerCase().indexOf(q) < 0;
    });
  });
})();
</script>"#;

/// The chart range asked for with `?days=N` (30 when absent or not a number).
fn days_param(q: &HashMap<String, String>) -> i64 {
    q.get("days").and_then(|v| v.parse().ok()).unwrap_or(30)
}

pub async fn page_get(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if !session_ok(&st, &headers) {
        return Ok(login_page());
    }
    let sm = summary::build(&st, days_param(&q)).await?;
    Ok(page(StatusCode::OK, "Razdor stats", &render(&sm)))
}

/// The password form the dashboard and the crash pages show without a session.
pub fn login_page() -> Response {
    page(
        StatusCode::OK,
        "Razdor stats",
        "<h1>Razdor stats</h1><form method=\"post\" action=\"/admin/login\">\
         <label>Password <input type=\"password\" name=\"password\" autofocus></label> <button>Enter</button></form>",
    )
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

pub async fn summary_json(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if !token_ok(&st, &headers) && !session_ok(&st, &headers) {
        return Err(ApiError::Unauthorized("admin token or dashboard session"));
    }
    let sm = summary::build(&st, days_param(&q)).await?;
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

    fn sample(range: i64, map: &str, msg: &str) -> Summary {
        use crate::budget::Figures;
        use crate::summary::{BanRow, CloudStats, CrashRate, CrashRow, MapRow, Overview, Release, Retention, VersionDay};
        let crash = |message: &str, is_new: bool| CrashRow {
            location: "battle.rs:10".into(),
            message: message.into(),
            count: 2,
            last_day: "2026-11-22".into(),
            last_version: "1.5.3".into(),
            first_day: "2026-11-20".into(),
            first_version: "1.5.3".into(),
            is_new,
        };
        Summary {
            generated_at: 1_795_435_200,
            range_days: range,
            overview: Overview {
                players_7d: 10,
                players_prev_7d: 8,
                new_installs_7d: 3,
                new_installs_prev_7d: 4,
                crashes_7d: 2,
                crashes_prev_7d: 1,
            },
            players_per_day: vec![Named { name: "2026-11-22".into(), value: 5 }],
            versions_per_day: vec![VersionDay { day: "2026-11-22".into(), version: "1.5.3".into(), installs: 5 }],
            releases: vec![Release { version: "1.5.3".into(), first_day: "2026-11-22".into() }],
            latest_version: "1.5.3".into(),
            previous_version: "1.5.2".into(),
            crash_rates: vec![CrashRate { version: "1.5.3".into(), crashes: 2, installs: 10 }],
            adoption: vec![],
            week_by_version: vec![],
            week_by_platform: vec![],
            week_by_game: vec![],
            retention: Retention { week1_cohort: 4, week1_returned: 1, week4_cohort: 0, week4_returned: 0 },
            maps: vec![MapRow { map: map.into(), starts: 4, wins: 1, losses: 1, abandoned: 2, abandoned_at: vec![] }],
            crashes: vec![crash(msg, false), crash("fresh crash", true)],
            fps: vec![FpsRow { gpu: "gpu".into(), phone: "phone".into(), sessions: 3, median_ms: 40.0, p99_ms: 60.0 }],
            experiments: vec![],
            bans: vec![BanRow {
                kind: "ip".into(),
                key: "k".into(),
                strikes: 3,
                active: true,
                reason: "r".into(),
                created_day: "2026-11-20".into(),
            }],
            cloud: CloudStats { accounts: 2, used_bytes: 1000, syncs_per_day: vec![] },
            storage: Figures { used_bytes: 850, max_bytes: 1000, free_bytes: 900, min_free_bytes: 100, over: false },
            countries: Vec::new(),
            downloads: Default::default(),
        }
    }

    #[test]
    fn render_has_sections_and_range_links() {
        let html = render(&sample(7, "Ashen Ford", "plain"));
        for id in ["overview", "players", "adoption", "maps", "crashes", "performance", "cloud", "admin"] {
            assert!(html.contains(&format!("id=\"{id}\"")), "missing section {id}");
        }
        for days in [7, 30, 90] {
            assert!(html.contains(&format!("href=\"/admin?days={days}\"")), "missing range {days}");
        }
        assert!(html.contains("<a href=\"/admin?days=30\">30 days</a>"));
        assert!(html.contains("aria-current=\"page\""));
        assert!(html.contains("Players per day (last 7 days)"));
    }

    #[test]
    fn player_text_is_escaped() {
        let html = render(&sample(30, "<script>evil()</script>", "<script>boom()</script>"));
        assert!(!html.contains("<script>evil"));
        assert!(!html.contains("<script>boom"));
        assert!(html.contains("&lt;script&gt;evil()&lt;/script&gt;"));
        assert!(html.contains("&lt;script&gt;boom()&lt;/script&gt;"));
    }

    #[test]
    fn new_crash_groups_get_a_badge() {
        let html = render(&sample(30, "Ashen Ford", "plain"));
        assert_eq!(html.matches("<span class=\"badge bad\">new</span>").count(), 1);
    }

    #[test]
    fn crash_rows_link_to_their_group_page() {
        let html = render(&sample(30, "Ashen Ford", "a & <b>"));
        assert!(html.contains("<a href=\"/admin/crash?loc=battle.rs%3A10&amp;msg=a%20%26%20%3Cb%3E\">Reports</a>"));
        assert!(!html.contains("a & <b>"), "the raw message must not reach the page");
    }

    #[test]
    fn constant_compare() {
        assert!(constant_eq(b"abc", b"abc"));
        assert!(!constant_eq(b"abc", b"abd"));
        assert!(!constant_eq(b"ab", b"abc"));
    }
}
