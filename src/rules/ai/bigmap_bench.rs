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
use crate::rules::content::{ExperimentalRules, HeroClass};
use crate::rules::game::STEP_SECONDS;
use crate::rules::map::{object_class, surface_value};

/// Map sides of the bench, unless `RAZDOR_BENCH_SIZES` (a comma list) picks others.
const SIZES: [u32; 3] = [200, 400, 800];
/// Armies on a bench map (the `armies` and `both` variants), unless `RAZDOR_BENCH_ARMIES` picks another count.
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
enum Variant {
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

/// Roaming armies on a bench map: `RAZDOR_BENCH_ARMIES` (a count), [`BENCH_ARMIES`] by default.
fn armies_from_env() -> u32 {
    std::env::var("RAZDOR_BENCH_ARMIES").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(BENCH_ARMIES)
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
fn roaming_template(dt: &DtInstall) -> DtArmy {
    for m in &dt.maps {
        let Ok(s) = m.load() else { continue };
        if let Some(a) = s.armies.iter().find(|a| a.patrols == 0 && a.ship == 0 && a.inactive == 0 && a.troops.iter().any(|t| !t.is_empty())) {
            return a.clone();
        }
    }
    panic!("no roaming army in the install's maps");
}

/// The bench's terrain: `RAZDOR_BENCH_TERRAIN=islands` for the islands map ([`islands_land`]),
/// else the blobs map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BenchTerrain {
    Blobs,
    Islands,
}

fn terrain_from_env() -> BenchTerrain {
    match std::env::var("RAZDOR_BENCH_TERRAIN").ok().as_deref().map(str::trim) {
        Some("islands") => BenchTerrain::Islands,
        _ => BenchTerrain::Blobs,
    }
}

/// Island clusters of the islands map (besides its continent).
const ISLAND_CLUSTERS: usize = 6;

/// A straight one-cell line of land from `a` to `b` (both on the map's grid): its cells that were
/// water become land, and are marked as bridge when `as_bridge`.
fn walk(land: &mut [bool], bridge: &mut [bool], a: (i32, i32), b: (i32, i32), as_bridge: bool) {
    let w = (land.len() as f64).sqrt().round() as i32;
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
    for i in 0..=steps {
        let x = (a.0 as f64 + (b.0 - a.0) as f64 * i as f64 / steps as f64).round() as i32;
        let y = (a.1 as f64 + (b.1 - a.1) as f64 * i as f64 / steps as f64).round() as i32;
        if x < 0 || y < 0 || x >= w || y >= w {
            continue;
        }
        let k = (y * w + x) as usize;
        if !land[k] {
            land[k] = true;
            bridge[k] = as_bridge;
        }
    }
}

/// The islands map's shape: a land mask and a bridge mask (`bridge` cells are land too). The
/// continent is a disc of about 60% of the map at its centre; each island cluster is four discs
/// of land in the water, outside the continent's edge. A bridge is a one-cell line of land across
/// the water from each cluster's centre to the continent's centre, and every second cluster also
/// gets one to the cluster before it, so each cluster is joined to the rest of the land.
fn islands_land(side: u32, rng: &mut Lcg) -> (Vec<bool>, Vec<bool>) {
    let w = side as i32;
    let c = w / 2;
    let idx = |x: i32, y: i32| (y * w + x) as usize;
    let inside = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < w;
    let r = (side as f64 * (0.6 / std::f64::consts::PI).sqrt()) as i32;
    let mut land = vec![false; (side * side) as usize];
    let mut bridge = vec![false; land.len()];
    for y in 0..w {
        for x in 0..w {
            land[idx(x, y)] = (x - c).pow(2) + (y - c).pow(2) <= r * r;
        }
    }
    let mut centres: Vec<(i32, i32)> = Vec::new();
    while centres.len() < ISLAND_CLUSTERS {
        let (cx, cy) = (rng.below(side) as i32, rng.below(side) as i32);
        if (cx - c).pow(2) + (cy - c).pow(2) <= (r + 12).pow(2) {
            continue;
        }
        // Each disc is joined to the cluster's centre by a one-cell land line (island land, not a bridge).
        for _ in 0..4 {
            let (bx, by) = (cx + rng.below(13) as i32 - 6, cy + rng.below(13) as i32 - 6);
            let br = 2 + rng.below(3) as i32;
            for y in by - br..=by + br {
                for x in bx - br..=bx + br {
                    if inside(x, y) && (x - bx).pow(2) + (y - by).pow(2) <= br * br {
                        land[idx(x, y)] = true;
                    }
                }
            }
            walk(&mut land, &mut bridge, (cx, cy), (bx, by), false);
        }
        centres.push((cx, cy));
    }
    for k in 0..centres.len() {
        walk(&mut land, &mut bridge, centres[k], (c, c), true);
        if k % 2 == 1 {
            walk(&mut land, &mut bridge, centres[k], centres[k - 1], true);
        }
    }
    (land, bridge)
}

/// A `side × side` map with grass, blobs of forest (tree objects), marsh, swamp and lakes (about
/// one blob per 900 cells of each kind, radius 2–6), the hero's start cleared, and the variant's
/// armies (cloned from `template`, on random passable cells, not patrolling, not hunting only the
/// player, with random targets) and events (global, once, never holding: hero level 30 000).
fn bench_scenario(template: &DtArmy, side: u32, variant: Variant) -> Scenario {
    let mut s = new_scenario(NewMap { width: side, height: side, fill: GRASS });
    let (w, n) = (side as i32, (side * side) as usize);
    let mut rng = Lcg(0x5eed_0000 ^ side);
    let islands = terrain_from_env() == BenchTerrain::Islands;
    // The islands variant: the land mask and the bridge mask (land that is only a bridge).
    let shape = islands.then(|| islands_land(side, &mut rng));
    let open = |i: usize| shape.as_ref().is_none_or(|(land, bridge)| land[i] && !bridge[i]);
    let mut terrain = match &shape {
        Some((land, _)) => land.iter().map(|&l| if l { GRASS } else { LAKE }).collect(),
        None => vec![GRASS; n],
    };
    let mut trees = vec![false; n];
    let hero = (w / 2, w / 2);
    let in_hero_start = |x: i32, y: i32| (x - hero.0).abs() <= 3 && (y - hero.1).abs() <= 3;
    let idx = |x: i32, y: i32| (y * w + x) as usize;
    for (kind, code) in [(None, GRASS), (Some(MARSH), MARSH), (Some(SWAMP), SWAMP), (Some(LAKE), LAKE)] {
        // The islands' water is already lake: no lake blobs there.
        if islands && code == LAKE {
            continue;
        }
        // `None` is the forest: tree objects on grass.
        for _ in 0..(n as u32 / 900).max(1) {
            let (cx, cy) = (rng.below(side) as i32, rng.below(side) as i32);
            let r = 2 + rng.below(5) as i32;
            for y in cy - r..=cy + r {
                for x in cx - r..=cx + r {
                    if x < 0 || y < 0 || x >= w || y >= w || (x - cx).pow(2) + (y - cy).pow(2) > r * r || in_hero_start(x, y) || !open(idx(x, y)) {
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
        for k in 0..armies_from_env() {
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
}

/// Waits [`BENCH_DAYS`] days on the endless wait, a `Game::tick` at a time, timing each tick. A
/// fight the hero is pulled into is played out (the AI on both sides) and does not count.
fn bench_run(c: &Arc<Content>, s: &Scenario, days: u64) -> BenchRun {
    let mut g = Game::from_scenario(c.clone(), s, HeroClass::Knight);
    // Army ids are u8 (a bench map with more than 255 armies wraps them), but the AI looks armies
    // up by `uid`: give each army its own uid here (bench only; the engine keeps `uid = id`).
    for (k, a) in g.world.armies.iter_mut().enumerate() {
        a.uid = k as u32 + 1;
    }
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
    BenchRun { ticks: ms.len(), plans: g.ai_stats.paths - plans0, ms }
}

/// The value at quantile `q` (0..1) of `sorted` (ascending).
fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

/// The experimental switches of the bench's playthrough, from `RAZDOR_BENCH_RULES` (a comma list:
/// `fast` for «Ускоренный ИИ», the one planner switch);
/// none by default, the original's planner.
fn bench_rules_from_env() -> ExperimentalRules {
    let list = std::env::var("RAZDOR_BENCH_RULES").unwrap_or_default();
    let has = |k: &str| list.split(',').any(|f| f.trim() == k);
    ExperimentalRules { fast_ai: has("fast"), ..Default::default() }
}

/// Times the AI's planner on big synthetic maps: one line per size and variant, with the ticks,
/// the plans, the median, p95 and max of a `Game::tick` (ms) and the total for the 2 days.
/// `RAZDOR_BENCH_SIZES` (e.g. `200,400`), `RAZDOR_BENCH_VARIANT` (`armies|events|both`) and
/// `RAZDOR_BENCH_RULES` (`fast`) picks the maps and the switch, `RAZDOR_BENCH_ARMIES` (a count,
/// 100 by default) the roaming armies of the `armies` and `both` variants. Deterministic maps
/// (fixed seeds); time it pinned to one core:
/// `nice -n 19 taskset -c 8 <test binary> bigmap_ai_bench --ignored --nocapture`.
#[test]
#[ignore]
fn bigmap_ai_bench() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = DtInstall::load(std::path::Path::new(&dir)).unwrap();
    let c = Arc::new(Content::from_dt(&dt).with_experimental(bench_rules_from_env()));
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

#[cfg(test)]
mod islands_tests {
    use super::*;

    /// The islands map: the continent is about 60% of the map, and every land cell is joined to
    /// the continent's centre by land (a flood over the land mask reaches all of it).
    #[test]
    fn islands_are_joined_to_the_continent() {
        for side in [200u32, 400, 800] {
            let mut rng = Lcg(0x5eed_0000 ^ side);
            let (land, bridge) = islands_land(side, &mut rng);
            let w = side as i32;
            let idx = |x: i32, y: i32| (y * w + x) as usize;
            let c = w / 2;
            let r = (side as f64 * (0.6 / std::f64::consts::PI).sqrt()) as i32;
            let disc = (0..w * w).filter(|&i| {
                let (x, y) = (i % w, i / w);
                (x - c).pow(2) + (y - c).pow(2) <= r * r
            });
            let share = disc.count() as f64 / (side * side) as f64;
            assert!((0.55..=0.65).contains(&share), "continent share {share}");
            assert!(bridge.iter().any(|&b| b), "no bridges at {side}");
            let mut seen = vec![false; land.len()];
            let mut stack = vec![(c, c)];
            seen[idx(c, c)] = true;
            while let Some((x, y)) = stack.pop() {
                for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w || ny >= w || seen[idx(nx, ny)] || !land[idx(nx, ny)] {
                        continue;
                    }
                    seen[idx(nx, ny)] = true;
                    stack.push((nx, ny));
                }
            }
            let land_total = land.iter().filter(|&&l| l).count();
            let reached = seen.iter().filter(|&&s| s).count();
            assert_eq!(reached, land_total, "land not joined at {side}: {reached} of {land_total}");
        }
    }
}
