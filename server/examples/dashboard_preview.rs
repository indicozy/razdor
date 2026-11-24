//! Writes the admin dashboard as a standalone page, filled with a made-up summary (40 days, four
//! versions, 12 maps, 8 crash groups, 10 FPS rows, 2 bans), for screenshots and layout checks:
//! `cargo run --example dashboard_preview -- <out.html>`. It reads no server data.

use razdor_server::admin::{page_html, render};
use razdor_server::budget::Figures;
use razdor_server::civil::ymd;
use razdor_server::summary::{
    BanRow, CloudStats, CrashRate, CrashRow, FpsRow, MapRow, Named, Overview, Release, Retention, Summary, VersionDay,
};

/// Day 20780 is 2026-11-23, the last day of the preview.
const TODAY: i64 = 20780;
const DAYS: i64 = 40;

fn main() {
    let out = std::env::args().nth(1).expect("usage: dashboard_preview <out.html>");
    let days: Vec<String> = (0..DAYS).map(|k| ymd(TODAY - (DAYS - 1) + k)).collect();
    let day = |k: usize| days[k].clone();

    // A small fixed generator, so the page is the same on every run.
    let mut seed: u64 = 7;
    let mut rnd = move |n: u64| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) % n
    };

    let mut players_per_day = Vec::new();
    let mut versions_per_day = Vec::new();
    let mut syncs_per_day = Vec::new();
    for k in 0..DAYS as usize {
        let weekend = k % 7 >= 5;
        let p = 180 + k as i64 * 4 + rnd(60) as i64 - if weekend { 40 } else { 0 };
        players_per_day.push(Named { name: day(k), value: p });
        syncs_per_day.push(Named { name: day(k), value: 900 + rnd(400) as i64 + k as i64 * 10 });
        // (version, share of the day's players in percent)
        let split: Vec<(&str, i64)> = if k < 9 {
            vec![("1.5.0", 100)]
        } else if k < 21 {
            vec![("1.5.0", 35), ("1.5.1", 65)]
        } else if k < 33 {
            vec![("1.5.1", 25), ("1.5.2", 75)]
        } else {
            vec![("1.5.2", 20), ("1.5.3", 80)]
        };
        for (version, share) in split {
            versions_per_day.push(VersionDay { day: day(k), version: version.into(), installs: p * share / 100 });
        }
    }

    let last7: i64 = players_per_day[33..].iter().map(|d| d.value).sum();
    let prev7: i64 = players_per_day[26..33].iter().map(|d| d.value).sum();

    let map = |name: &str, starts: i64, wins: i64, abandoned: i64, at: &[(&str, i64)]| MapRow {
        map: name.into(),
        starts,
        wins,
        losses: starts - wins - abandoned,
        abandoned,
        abandoned_at: at.iter().map(|(n, v)| Named { name: (*n).into(), value: *v }).collect(),
    };
    let maps = vec![
        map("Ashen Ford", 212, 96, 18, &[("battle", 11), ("town", 4)]),
        map("Iron Pass", 180, 71, 52, &[("battle", 30), ("ai turn", 12)]),
        map("Grey Harbour", 164, 80, 9, &[("town", 5)]),
        map("Salt Marsh", 150, 60, 41, &[("battle", 22), ("map", 10)]),
        map("Red Hills", 133, 70, 6, &[("battle", 4)]),
        map("Long Road", 121, 40, 33, &[("battle", 20)]),
        map("Crooked Bay", 110, 55, 12, &[("town", 7)]),
        map("Old Mill", 98, 44, 4, &[]),
        map("Stone Steppe", 91, 38, 22, &[("battle", 14)]),
        map("Vale of Reeds", 70, 30, 9, &[("map", 4)]),
        map("North Keep", 44, 19, 3, &[]),
        map("Ember Coast", 12, 2, 6, &[("battle", 5)]),
    ];

    let crash = |location: &str, message: &str, count: i64, last: usize, first: usize, version: &str, is_new: bool| CrashRow {
        location: location.into(),
        message: message.into(),
        count,
        last_day: day(last),
        last_version: version.into(),
        first_day: day(first),
        first_version: if first < 21 { "1.5.1".into() } else { version.into() },
        is_new,
    };
    let crashes = vec![
        crash("battle/ai.rs:412", "index out of bounds: the len is 3 but the index is 7", 41, 38, 35, "1.5.3", true),
        crash("ui/map_view.rs:88", "called `Option::unwrap()` on a `None` value", 23, 39, 36, "1.5.3", true),
        crash("world/towns.rs:150", "attempt to subtract with overflow", 17, 39, 10, "1.5.2", false),
        crash("save/load.rs:64", "unexpected end of file", 9, 37, 14, "1.5.2", false),
        crash("battle/move.rs:203", "assertion failed: hp >= 0", 7, 33, 22, "1.5.2", false),
        crash("audio/mix.rs:31", "buffer underrun in mixer", 5, 29, 3, "1.5.1", false),
        crash("ui/tooltip.rs:57", "range end index 12 out of range for slice of length 10", 4, 30, 25, "1.5.2", false),
        crash("net/cloud.rs:120", "timed out reading response body", 2, 22, 5, "1.5.0", false),
    ];

    let fps = |gpu: &str, phone: &str, sessions: i64, median: f64, p99: f64| FpsRow {
        gpu: gpu.into(),
        phone: phone.into(),
        sessions,
        median_ms: median,
        p99_ms: p99,
    };
    let fps_rows = vec![
        fps("Adreno 740", "Pixel 8", 320, 14.2, 21.0),
        fps("Mali-G78", "Galaxy A54", 210, 19.8, 33.0),
        fps("Apple A17 GPU", "iPhone 15", 180, 11.5, 16.0),
        fps("Adreno 650", "Galaxy S20", 96, 24.0, 38.0),
        fps("Adreno 610", "Galaxy A13", 74, 41.5, 88.0),
        fps("Mali-G52", "Redmi Note 11", 60, 27.0, 46.0),
        fps("Mali-G57", "Galaxy A34", 41, 36.0, 52.0),
        fps("Adreno 506", "Moto G7", 33, 52.0, 97.0),
        fps("PowerVR GE8320", "Nokia 3.4", 19, 63.0, 120.0),
        fps("Apple A14 GPU", "iPhone 12", 12, 13.0, 18.0),
    ];

    let sm = Summary {
        generated_at: 1_795_435_200,
        range_days: 30,
        overview: Overview {
            players_7d: last7,
            players_prev_7d: prev7,
            new_installs_7d: 260,
            new_installs_prev_7d: 310,
            crashes_7d: 41,
            crashes_prev_7d: 28,
        },
        players_per_day,
        adoption: vec![],
        versions_per_day,
        releases: vec![
            Release { version: "1.5.0".into(), first_day: "2026-09-14".into() },
            Release { version: "1.5.1".into(), first_day: "2026-10-02".into() },
            Release { version: "1.5.2".into(), first_day: day(21) },
            Release { version: "1.5.3".into(), first_day: day(33) },
        ],
        latest_version: "1.5.3".into(),
        previous_version: "1.5.2".into(),
        crash_rates: vec![
            CrashRate { version: "1.5.3".into(), crashes: 64, installs: 1_420 },
            CrashRate { version: "1.5.2".into(), crashes: 58, installs: 2_310 },
            CrashRate { version: "1.5.1".into(), crashes: 22, installs: 1_050 },
            CrashRate { version: "1.5.0".into(), crashes: 9, installs: 400 },
        ],
        week_by_version: vec![
            Named { name: "1.5.3".into(), value: 812 },
            Named { name: "1.5.2".into(), value: 640 },
            Named { name: "1.5.1".into(), value: 170 },
        ],
        week_by_platform: vec![Named { name: "Android".into(), value: 1_020 }, Named { name: "Windows".into(), value: 480 }],
        week_by_game: vec![Named { name: "campaign".into(), value: 900 }, Named { name: "skirmish".into(), value: 420 }],
        retention: Retention { week1_cohort: 210, week1_returned: 66, week4_cohort: 180, week4_returned: 31 },
        maps,
        crashes,
        fps: fps_rows,
        experiments: vec![Named { name: "fast AI".into(), value: 120 }, Named { name: "wide view".into(), value: 46 }],
        bans: vec![
            BanRow {
                kind: "install".into(),
                key: "3f9a…c201".into(),
                strikes: 4,
                active: true,
                reason: "cloud abuse".into(),
                created_day: day(30),
            },
            BanRow {
                kind: "ip".into(),
                key: "203.0.113.7".into(),
                strikes: 2,
                active: false,
                reason: "login flood".into(),
                created_day: day(12),
            },
        ],
        cloud: CloudStats { accounts: 340, used_bytes: 96_000_000, syncs_per_day },
        storage: Figures { used_bytes: 1_700_000_000, max_bytes: 2_000_000_000, free_bytes: 900_000_000, min_free_bytes: 500_000_000, over: false },
        countries: Vec::new(),
        downloads: Default::default(),
    };

    std::fs::write(&out, page_html("Razdor stats", &render(&sm))).expect("write the preview page");
    println!("wrote {out}");
}
