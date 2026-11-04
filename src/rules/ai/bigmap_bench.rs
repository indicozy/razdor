//! The AI's planner on big synthetic maps (`bigmap_ai_bench`), and the fingerprint of the AI's
//! state on every map (`ai_fingerprint`). Both need `RAZDOR_DT_DIR` (they pass without it) and
//! are `#[ignore]`d: run them by name, see `docs/flows/testing-and-tools.md`. The maps are built
//! in memory, never written into the repo.
use std::sync::Arc;
use std::time::Instant;

use super::real_maps::{fight_it_out, play_days, DAYS};
use super::*;
use crate::dt::dtm::{Army as DtArmy, Scenario, Surface};
use crate::dt::install::{DtInstall, MapEntry};
use crate::editor::defaults::{new_scenario, NewMap};
use crate::editor::events::new_event;
use crate::rules::content::HeroClass;
use crate::rules::game::STEP_SECONDS;
use crate::rules::map::{object_class, surface_value};

/// Map sides of the bench, unless `RAZDOR_BENCH_SIZES` (a comma list) picks others.
const SIZES: [u32; 3] = [200, 400, 800];
/// Armies on a bench map (the `armies` and `both` variants).
const BENCH_ARMIES: u32 = 100;
/// Global once-events on a bench map (the `events` and `both` variants).
const BENCH_EVENTS: u32 = 10_000;
/// Days of waiting timed by `bigmap_ai_bench`.
const BENCH_DAYS: u64 = 2;

/// Terrain codes of the synthetic maps (the editor's surface palette).
const GRASS: u8 = Surface::GrassPlain as u8;
const MARSH: u8 = Surface::Marsh as u8;
const SWAMP: u8 = Surface::ImpassableSwamp as u8;
const LAKE: u8 = Surface::DeepSea as u8;

/// What a bench map carries: the AI armies, the events, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Variant {
    Armies,
    Events,
    Both,
}

impl Variant {
    const ALL: [Variant; 3] = [Variant::Armies, Variant::Events, Variant::Both];

    fn name(self) -> &'static str {
        match self {
            Variant::Armies => "armies",
            Variant::Events => "events",
            Variant::Both => "both",
        }
    }

    fn has_armies(self) -> bool {
        self != Variant::Events
    }

    fn has_events(self) -> bool {
        self != Variant::Armies
    }

    /// `RAZDOR_BENCH_VARIANT`: `armies`, `events` or `both` (the default), else all three.
    fn from_env() -> Vec<Variant> {
        match std::env::var("RAZDOR_BENCH_VARIANT").ok().as_deref().map(str::trim) {
            Some("armies") => vec![Variant::Armies],
            Some("events") => vec![Variant::Events],
            Some("both") | None => vec![Variant::Both],
            Some(_) => Variant::ALL.to_vec(),
        }
    }
}

/// Map sides picked by `RAZDOR_BENCH_SIZES` (a comma list), all three by default.
fn sizes_from_env() -> Vec<u32> {
    match std::env::var("RAZDOR_BENCH_SIZES") {
        Ok(list) => list.split(',').filter_map(|s| s.trim().parse().ok()).collect(),
        Err(_) => SIZES.to_vec(),
    }
}

/// A fixed-seed generator for the synthetic maps (not the game's).
struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 8
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

/// The first roaming army of the install's maps: a non-patrolling army (`patrols` 0) that is
/// not a ship, not inactive and has troops. Its record is the template of every bench army.
pub(super) fn roaming_template(dt: &DtInstall) -> DtArmy {
    for m in &dt.maps {
        let Ok(s) = m.load() else { continue };
        if let Some(a) = s.armies.iter().find(|a| a.patrols == 0 && a.ship == 0 && a.inactive == 0 && a.troops.iter().any(|t| !t.is_empty())) {
            return a.clone();
        }
    }
    panic!("no roaming army in the install's maps");
}

/// A `side × side` map with grass, blobs of forest (tree objects), marsh, swamp and lakes (about
/// one blob per 900 cells of each kind, radius 2–6), the hero's start cleared, and the variant's
/// armies (cloned from `template`, on random passable cells, not patrolling, not hunting only the
/// player, with random targets) and events (global, once, never holding: hero level 30 000).
pub(super) fn bench_scenario(template: &DtArmy, side: u32, variant: Variant) -> Scenario {
    let mut s = new_scenario(NewMap { width: side, height: side, fill: GRASS });
    let (w, n) = (side as i32, (side * side) as usize);
    let mut rng = Lcg(0x5eed_0000 ^ side);
    let mut terrain = vec![GRASS; n];
    let mut trees = vec![false; n];
    let hero = (w / 2, w / 2);
    let in_hero_start = |x: i32, y: i32| (x - hero.0).abs() <= 3 && (y - hero.1).abs() <= 3;
    let idx = |x: i32, y: i32| (y * w + x) as usize;
    for (kind, code) in [(None, GRASS), (Some(MARSH), MARSH), (Some(SWAMP), SWAMP), (Some(LAKE), LAKE)] {
        // `None` is the forest: tree objects on grass.
        for _ in 0..(n as u32 / 900).max(1) {
            let (cx, cy) = (rng.below(side) as i32, rng.below(side) as i32);
            let r = 2 + rng.below(5) as i32;
            for y in cy - r..=cy + r {
                for x in cx - r..=cx + r {
                    if x < 0 || y < 0 || x >= w || y >= w || (x - cx).pow(2) + (y - cy).pow(2) > r * r || in_hero_start(x, y) {
                        continue;
                    }
                    match kind {
                        None => trees[idx(x, y)] = true,
                        Some(_) => terrain[idx(x, y)] = code,
                    }
                }
            }
        }
    }
    s.terrain = terrain;
    s.objects = (0..n)
        .filter(|&i| trees[i])
        .map(|i| crate::dt::dtm::MapObject { x: (i as i32 % w) as u16, y: (i as i32 / w) as u16, sprite: 0, class: object_class::TREES })
        .collect();
    if variant.has_armies() {
        for k in 0..BENCH_ARMIES {
            let (x, y) = loop {
                let (x, y) = (rng.below(side) as i32, rng.below(side) as i32);
                let passable = surface_value(Surface::from_code(s.terrain[idx(x, y)]).unwrap_or(Surface::GrassPlain)) > 0;
                if passable && !in_hero_start(x, y) {
                    break (x, y);
                }
            };
            let mut a = template.clone();
            a.id = (k + 1) as u8;
            a.x = x as u16;
            a.y = y as u16;
            a.patrols = 0;
            a.patrol_radius = 0;
            a.hunts_player_only = 0;
            a.no_random_targets = 0;
            a.ignored_by_ai = 0;
            a.home_building = 0;
            a.named_character = 0;
            s.armies.push(a);
        }
    }
    if variant.has_events() {
        for _ in 0..BENCH_EVENTS {
            let mut e = new_event(&s, 1);
            e.conditions.stats_check = 1;
            e.conditions.level = 30_000;
            s.events.push(e);
        }
    }
    s
}

/// What [`bigmap_ai_bench`] timed on one map.
struct BenchRun {
    ticks: usize,
    plans: u32,
    /// Milliseconds of each `Game::tick`, in order.
    ms: Vec<f64>,
    mode: &'static str,
    proto: crate::rules::ai::planner_proto::ProtoStats,
    cache_bytes: usize,
}

/// Waits [`BENCH_DAYS`] days on the endless wait, a `Game::tick` at a time, timing each tick. A
/// fight the hero is pulled into is played out (the AI on both sides) and does not count.
fn bench_run(c: &Arc<Content>, s: &Scenario, days: u64) -> BenchRun {
    let mut g = Game::from_scenario(c.clone(), s, HeroClass::Knight);
    let end = g.clock.total_minutes() + (days * MINUTES_PER_DAY) as f64;
    let plans0 = g.ai_stats.paths;
    g.begin_endless_wait();
    let mut ms = Vec::new();
    let mut calls = 0;
    while g.clock.total_minutes() < end && calls < 2_000_000 {
        calls += 1;
        if g.pending_question().is_some() {
            g.answer_question(true);
            continue;
        }
        if g.foe.is_some() {
            if fight_it_out(&mut g) == Outcome::Defeat {
                g.world.armies.iter_mut().for_each(|a| a.ignore_until = f64::MAX);
            }
            g.begin_endless_wait();
            continue;
        }
        g.drain_events();
        let t0 = Instant::now();
        g.tick(STEP_SECONDS);
        ms.push(t0.elapsed().as_secs_f64() * 1e3);
    }
    g.end_endless_wait();
    assert!(g.clock.total_minutes() >= end, "the bench did not reach the end of its days (stalled)");
    BenchRun { ticks: ms.len(), plans: g.ai_stats.paths - plans0, ms, mode: g.proto.mode.name(), proto: g.proto.stats.clone(), cache_bytes: g.proto.bytes() }
}

/// The value at quantile `q` (0..1) of `sorted` (ascending).
fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

/// Times the AI's planner on big synthetic maps: one line per size and variant, with the ticks,
/// the plans, the median, p95 and max of a `Game::tick` (ms) and the total for the 2 days.
/// `RAZDOR_BENCH_SIZES` (e.g. `200,400`) and `RAZDOR_BENCH_VARIANT` (`armies|events|both`) pick
/// the maps. Deterministic maps (fixed seeds); time it pinned to one core:
/// `nice -n 19 taskset -c 8 <test binary> bigmap_ai_bench --ignored --nocapture`.
#[test]
#[ignore]
fn bigmap_ai_bench() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = DtInstall::load(std::path::Path::new(&dir)).unwrap();
    let c = Arc::new(Content::from_dt(&dt));
    let template = roaming_template(&dt);
    for side in sizes_from_env() {
        for variant in Variant::from_env() {
            let s = bench_scenario(&template, side, variant);
            let run = bench_run(&c, &s, BENCH_DAYS);
            let mut sorted = run.ms.clone();
            sorted.sort_by(f64::total_cmp);
            let total: f64 = run.ms.iter().sum();
            println!(
                "{side:>4}x{side:<4} {:<7} ticks {:>3} plans {:>6}: median {:>8.3} ms, p95 {:>8.3} ms, max {:>8.3} ms per tick; total {:>9.1} ms per {BENCH_DAYS} days",
                variant.name(),
                run.ticks,
                run.plans,
                quantile(&sorted, 0.5),
                quantile(&sorted, 0.95),
                quantile(&sorted, 1.0),
                total
            );
            let p = &run.proto;
            println!(
                "      planner {}: seeds/plan {:.1}, fallbacks {}, fields built {} hits {} evicted {} peak {} ({:.2} MB), build {:.0} ms; hpa {:.0} ms {} nodes {:.2} MB; plan {:.0} ms, exact {:.0} ms; cache now {:.2} MB",
                run.mode,
                p.seeds as f64 / p.plans.max(1) as f64,
                p.fallbacks,
                p.field_builds,
                p.field_hits,
                p.field_evicted,
                p.field_peak_count,
                p.field_peak_bytes as f64 / 1048576.0,
                p.field_build_ms,
                p.hpa_precompute_ms,
                p.hpa_nodes,
                p.hpa_bytes as f64 / 1048576.0,
                p.plan_ms,
                p.exact_ms,
                run.cache_bytes as f64 / 1048576.0
            );
        }
    }
}

/// FNV-1a of a label and a hash, one line per map: `<map> <hash>` (the output of `--nocapture`;
/// `docs/flows/testing-and-tools.md`). Every install map plays [`DAYS`] days with the AI as
/// `real_maps` plays them, every map of `RAZDOR_COMMUNITY_DIR`'s installs (their maps and extra
/// maps) likewise, and every bench map (armies and both variants, every size) 2 days. Equal
/// output before and after a change to the AI's planner proves the AI is unchanged.
#[test]
#[ignore]
fn ai_fingerprint() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = DtInstall::load(std::path::Path::new(&dir)).unwrap();
    let c = Arc::new(Content::from_dt(&dt));
    for m in &dt.maps {
        let s = m.load().unwrap();
        println!("{} {:016x}", m.name, play_days(&c, &s, DAYS).fingerprint);
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
                println!("{name}/{} {:016x}", m.name, play_days(&ec, &s, DAYS).fingerprint);
            }
        }
    }
    let template = roaming_template(&dt);
    for side in sizes_from_env() {
        for variant in [Variant::Armies, Variant::Both] {
            let s = bench_scenario(&template, side, variant);
            println!("bench-{side}x{side}-{} {:016x}", variant.name(), play_days(&c, &s, BENCH_DAYS).fingerprint);
        }
    }
}
