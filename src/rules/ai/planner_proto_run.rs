//! The prototype planners on the player's maps and on the bench maps: a free run per map
//! (`planner_proto_free_run`, `#[ignore]`d; one `RUN` line per map, numbers only), the same
//! loop as `real_maps::play_days` (the hero at his start, yes to every question).
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use super::bigmap_bench::{bench_scenario, roaming_template, Variant};
use super::planner_proto::ProtoStats;
use super::real_maps::fight_it_out;
use super::*;
use crate::dt::dtm::Scenario;
use crate::dt::install::{DtInstall, MapEntry};
use crate::rules::content::HeroClass;

/// What one free run counted.
struct Run {
    battles: u32,
    captures: u32,
    alive: usize,
    secs: f64,
    proto: ProtoStats,
    mode: &'static str,
    cache_bytes: usize,
}

/// Plays `days` days of `s` with the hero standing at his start (he fights whoever comes,
/// answers yes to every question), as `real_maps::play_days` does.
fn free_run(c: &Arc<Content>, s: &Scenario, days: u64) -> Run {
    let t0 = Instant::now();
    let mut g = Game::from_scenario(c.clone(), s, HeroClass::Knight);
    let end = g.clock.total_minutes() + (days * MINUTES_PER_DAY) as f64;
    let mut calls = 0;
    let mut lost = false;
    while g.clock.total_minutes() < end && calls < 200_000 {
        calls += 1;
        if g.pending_question().is_some() {
            g.answer_question(true);
            continue;
        }
        if g.foe.is_some() {
            if fight_it_out(&mut g) == Outcome::Defeat {
                lost = true;
                g.world.armies.iter_mut().for_each(|a| a.ignore_until = f64::MAX);
            }
            continue;
        }
        if lost {
            g.world.armies.iter_mut().for_each(|a| a.ignore_until = f64::MAX);
        }
        g.drain_events();
        g.wait(4);
    }
    Run {
        battles: g.ai_stats.battles,
        captures: g.ai_stats.captures,
        alive: g.world.armies.iter().filter(|a| managed(a) && !a.mind.fallen).count(),
        secs: t0.elapsed().as_secs_f64(),
        proto: g.proto.stats.clone(),
        mode: g.proto.mode.name(),
        cache_bytes: g.proto.bytes(),
    }
}

/// One `RUN` line: `key=value` pairs, spaces in the name written as `_`.
fn emit(name: &str, days: u64, r: &Run) {
    let p = &r.proto;
    println!(
        "RUN mode={} name={} days={} battles={} captures={} steps={} alive={} secs={:.3} plans={} seeds={} fallbacks={} cmp={} cmp_none={} same_first={} same_target={} same_path={} ratio_sum={:.4} ratio_n={} builds={} hits={} evicted={} peak_bytes={} peak_count={} build_ms={:.1} plan_ms={:.1} exact_ms={:.1} hpa_ms={:.1} hpa_bytes={} hpa_nodes={} hpa_legs={} cache_bytes={}",
        r.mode,
        name.replace(' ', "_"),
        days,
        r.battles,
        r.captures,
        p.steps,
        r.alive,
        r.secs,
        p.plans,
        p.seeds,
        p.fallbacks,
        p.cmp,
        p.cmp_none,
        p.same_first,
        p.same_target,
        p.same_path,
        p.ratio_sum,
        p.ratio_n,
        p.field_builds,
        p.field_hits,
        p.field_evicted,
        p.field_peak_bytes,
        p.field_peak_count,
        p.field_build_ms,
        p.plan_ms,
        p.exact_ms,
        p.hpa_precompute_ms,
        p.hpa_bytes,
        p.hpa_nodes,
        p.hpa_legs,
        r.cache_bytes
    );
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

/// Every install map (the base game, then every community install and its extra maps) for
/// `RAZDOR_PROTO_DAYS` days (30), then the bench maps (armies and events, both variant, sizes of
/// `RAZDOR_BENCH_SIZES`) for `RAZDOR_PROTO_BENCH_DAYS` days (2); `RAZDOR_PROTO_BENCH=0` skips them.
/// The planner comes from `RAZDOR_PLANNER` (and `RAZDOR_PLANNER_SHADOW=1` compares it with the exact).
#[test]
#[ignore]
fn planner_proto_free_run() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let days = env_u64("RAZDOR_PROTO_DAYS", 30);
    let bench_days = env_u64("RAZDOR_PROTO_BENCH_DAYS", 2);
    let dt = DtInstall::load(Path::new(&dir)).unwrap();
    let c = Arc::new(Content::from_dt(&dt));
    for m in &dt.maps {
        let s = m.load().unwrap();
        emit(&format!("base/{}", m.name), days, &free_run(&c, &s, days));
    }
    if let Some(root) = std::env::var_os("RAZDOR_COMMUNITY_DIR") {
        let root = std::path::PathBuf::from(root);
        let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&root).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_dir()).collect();
        entries.sort();
        for entry in entries {
            let name = entry.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let install = entry.join("install");
            if !install.exists() {
                continue;
            }
            let edt = DtInstall::load(&install).unwrap();
            let ec = Arc::new(Content::from_dt(&edt));
            let mut maps = edt.maps.clone();
            if let Ok(read) = std::fs::read_dir(entry.join("maps")) {
                let mut extra: Vec<MapEntry> = read
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case(crate::dt::install::MAP_EXTENSION)))
                    .map(|p| MapEntry { name: p.file_stem().unwrap_or_default().to_string_lossy().into_owned(), path: p })
                    .collect();
                extra.sort_by(|a, b| a.name.cmp(&b.name));
                maps.extend(extra);
            }
            for m in &maps {
                let s = m.load().unwrap();
                emit(&format!("{name}/{}", m.name), days, &free_run(&ec, &s, days));
            }
        }
    }
    if env_u64("RAZDOR_PROTO_BENCH", 1) == 1 {
        let template = roaming_template(&dt);
        let sizes: Vec<u32> = std::env::var("RAZDOR_BENCH_SIZES")
            .ok()
            .map(|l| l.split(',').filter_map(|s| s.trim().parse().ok()).collect())
            .unwrap_or_else(|| vec![200, 400, 800]);
        for side in sizes {
            let s = bench_scenario(&template, side, Variant::Both);
            emit(&format!("bench-{side}"), bench_days, &free_run(&c, &s, bench_days));
        }
    }
}

/// The prototype's paths are walks the rules accept (the unit test of `planner_proto.rs` checks
/// the details); here the mode is the one the environment picks, and a game plays some days
/// with it on the bench maps without a panic.
#[test]
fn planner_proto_plays_a_bench_map_without_a_panic() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = DtInstall::load(Path::new(&dir)).unwrap();
    let c = Arc::new(Content::from_dt(&dt));
    let template = roaming_template(&dt);
    let s = bench_scenario(&template, 200, Variant::Armies);
    let r = free_run(&c, &s, 1);
    assert!(r.alive > 0, "every army of the bench map died in a day");
}
