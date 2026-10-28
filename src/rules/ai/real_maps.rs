//! The AI on the player's maps; skipped without `RAZDOR_DT_DIR`. Numbers only.
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Instant;

use super::*;
use crate::dt::dtm::Scenario;
use crate::dt::install::{DtInstall, MapEntry};
use crate::rules::content::HeroClass;
use crate::rules::game::BattleResult;

/// Days simulated on every map.
const DAYS: u64 = 30;
/// Seconds a map may take in a release build, times `RAZDOR_PERF_BUDGET_SCALE` (a float,
/// 1 by default) for slower cores: the E-cores the tests are pinned to need 2 (РК6 takes ~8 s).
const RELEASE_BUDGET_SECS: f64 = 5.0;

/// [`RELEASE_BUDGET_SECS`] scaled by `RAZDOR_PERF_BUDGET_SCALE`.
fn release_budget_secs() -> f64 {
    let scale = std::env::var("RAZDOR_PERF_BUDGET_SCALE").ok().and_then(|s| s.trim().parse::<f64>().ok()).filter(|s| *s > 0.0);
    RELEASE_BUDGET_SECS * scale.unwrap_or(1.0)
}

/// Auto-plays the player's pending battle, the AI on both sides.
fn fight_it_out(g: &mut Game) -> Outcome {
    let mut b = g.start_battle();
    b.auto_play_to_end();
    g.resolve_battle(&b);
    b.outcome()
}

/// Quick battle against the first armies of every shipped map: each reaches an outcome and
/// resolves (XP, loot, capture, events) as a battle played to its end.
#[test]
fn quick_battles_against_real_armies() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = DtInstall::load(std::path::Path::new(&dir)).unwrap();
    let c = Arc::new(Content::from_dt(&dt));
    let mut fought = 0;
    for m in &dt.maps {
        let s = m.load().unwrap();
        let g0 = Game::from_scenario(c.clone(), &s, HeroClass::Knight);
        for i in 0..g0.world.armies.len().min(4) {
            let mut g = Game::from_scenario(c.clone(), &s, HeroClass::Knight);
            g.drain_events();
            g.foe = Some(Foe::Army(i));
            let mut b = g.start_battle();
            let outcome = b.auto_play_to_end();
            assert_ne!(outcome, Outcome::Ongoing, "{}: army {i}", m.name);
            let result = g.resolve_battle(&b);
            match outcome {
                Outcome::Victory => assert!(matches!(result, BattleResult::Victory { .. }), "{}: army {i}", m.name),
                _ => assert!(matches!(result, BattleResult::Defeat | BattleResult::Withdrew { .. }), "{}: army {i}", m.name),
            }
            fought += 1;
        }
    }
    assert!(fought > 0);
}

/// What [`play_days`] saw on one map.
struct DaysRun {
    /// The AI's counts over the run.
    ai: AiStats,
    /// Armies at the start and at the end of the run.
    armies: usize,
    armies_after: usize,
    /// Armies waiting to respawn at the end.
    waiting: usize,
    player_battles: usize,
    /// The hero lost a battle (the world went on without him).
    player_lost: bool,
    /// The clock did not reach the end within the call budget (a softlock).
    stalled: bool,
    secs: f64,
}

/// Plays `days` days of `s` with the hero standing at his start: he fights whoever comes and
/// answers yes to every question.
fn play_days(c: &Arc<Content>, s: &Scenario, days: u64) -> DaysRun {
    let t0 = Instant::now();
    let mut g = Game::from_scenario(c.clone(), s, HeroClass::Knight);
    let end = g.clock.total_minutes() + (days * MINUTES_PER_DAY) as f64;
    let armies = g.world.armies.len();
    let mut player_battles = 0;
    let mut player_lost = false;
    let mut calls = 0;
    while g.clock.total_minutes() < end && calls < 200_000 {
        calls += 1;
        if g.pending_question().is_some() {
            g.answer_question(true);
            continue;
        }
        if g.foe.is_some() {
            player_battles += 1;
            if fight_it_out(&mut g) == Outcome::Defeat {
                // The game would be over; the world goes on without the player.
                player_lost = true;
                g.world.armies.iter_mut().for_each(|a| a.ignore_until = f64::MAX);
            }
            continue;
        }
        if player_lost {
            g.world.armies.iter_mut().for_each(|a| a.ignore_until = f64::MAX);
        }
        g.drain_events();
        g.wait(4);
    }
    DaysRun {
        ai: g.ai_stats,
        armies,
        armies_after: g.world.armies.len(),
        waiting: g.world.respawns.len(),
        player_battles,
        player_lost,
        stalled: g.clock.total_minutes() < end,
        secs: t0.elapsed().as_secs_f64(),
    }
}

/// Every shipped map for 30 days with the hero standing at his start (he fights whoever
/// comes, answers yes to every question): no panic, every map within the time budget
/// (release builds), and the AI does something.
#[test]
fn thirty_days_on_every_map() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = DtInstall::load(std::path::Path::new(&dir)).unwrap();
    let c = Arc::new(Content::from_dt(&dt));
    let mut totals = AiStats::default();
    for m in &dt.maps {
        let s = m.load().unwrap();
        let r = play_days(&c, &s, DAYS);
        let st = r.ai;
        println!(
            "{:<28} {:>3}x{:<3} armies {:>2}→{:>2} waiting {:>2}: AI battles {:>3}, captures {:>2}, respawns {:>2}, hired {:>3}, bought {:>2}, routes {:>5}; player battles {:>2}{}; {:.2} s",
            m.name,
            s.width(),
            s.height(),
            r.armies,
            r.armies_after,
            r.waiting,
            st.battles,
            st.captures,
            st.respawns,
            st.hired,
            st.bought,
            st.paths,
            r.player_battles,
            if r.player_lost { " (lost)" } else { "" },
            r.secs
        );
        assert!(!r.stalled, "{}: the simulation stalled", m.name);
        if !cfg!(debug_assertions) {
            assert!(r.secs < release_budget_secs(), "{}: {:.2} s for {DAYS} days (set RAZDOR_PERF_BUDGET_SCALE on slow cores)", m.name, r.secs);
        }
        totals.battles += st.battles;
        totals.captures += st.captures;
        totals.respawns += st.respawns;
        totals.paths += st.paths;
        totals.hired += st.hired;
        totals.bought += st.bought;
    }
    println!("total: {totals:?}");
    assert!(totals.paths > 0 && totals.hired > 0, "the AI moved and hired somewhere");
}

/// РК1's sea robbers (army 9, a ship) sail into the shore village Соленая, as in the original
/// (building footprints are open on the SHIP map, world.md §1; ai.md §13), and are caught
/// there: from next to the village one step onto any of its cells engages the army standing
/// in it (world.md §4.2 rule 2, 0x4ad94c); with the hero inside his village they assault it
/// and fight him (ai.md §9.1, 0x4a548c). Playtest 2026-10-08: "they cannot be caught".
#[test]
fn rk1_sea_robbers_are_caught_in_the_village() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = DtInstall::load(std::path::Path::new(&dir)).unwrap();
    let c = Arc::new(Content::from_dt(&dt));
    let s = dt.maps.iter().find(|m| m.name.starts_with("РК1")).unwrap().load().unwrap();
    let robbers = |g: &Game| g.world.armies.iter().position(|a| a.id == 9).expect("the robbers");
    // Hours waited (1 h at a time) until `done`, at most 10 days.
    let wait_until = |g: &mut Game, done: &dyn Fn(&Game) -> bool| {
        for h in 0..240 {
            if g.pending_question().is_some() {
                g.answer_question(true);
            }
            g.drain_events();
            if done(g) {
                return h;
            }
            g.wait(1);
        }
        panic!("not within 10 days");
    };
    // The hero waits next to the village; once they stand in it, one step in catches them.
    let mut g = Game::from_scenario(c.clone(), &s, HeroClass::Knight);
    let village = g.world.locations.iter().position(|l| l.name == "Деревня Соленая").unwrap();
    assert!(g.world.armies[robbers(&g)].sails());
    g.pos = g.world.map.center((27, 38));
    wait_until(&mut g, &|g| g.world.armies[robbers(g)].mind.standing == Some(village));
    let i = robbers(&g);
    let inside = g.world.armies[i].tile(&g.world.map);
    assert_eq!(g.world.location_covering(inside), Some(village));
    assert_ne!(inside, (26, 38), "on another cell than the one stepped onto");
    assert!(g.set_destination((26, 38)));
    for _ in 0..100 {
        g.tick(0.05);
        if g.foe.is_some() || !g.moving() {
            break;
        }
    }
    assert_eq!(g.foe, Some(Foe::Army(i)));
    assert_eq!(g.tile(), (27, 38), "he stays on the cell he was leaving");
    // The hero waits inside his village: they sail in and attack him.
    let mut g = Game::from_scenario(c, &s, HeroClass::Knight);
    g.pos = g.world.map.center((28, 36));
    assert!(g.set_destination((25, 36)));
    for _ in 0..400 {
        g.tick(0.05);
        if !g.moving() {
            break;
        }
    }
    assert_eq!(g.location, Some(village));
    wait_until(&mut g, &|g| g.foe.is_some());
    assert_eq!(g.foe, Some(Foe::Army(robbers(&g))));
}

/// Text of a caught panic's payload.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with a non-text payload".to_string()
    }
}

/// Community map smoke test, the community part of the Definition of Done in CLAUDE.md (the
/// owner's decision, 2026-10-28). Every map of every install in `RAZDOR_COMMUNITY_DIR` plays
/// 30 days with the AI; every failing map is reported, not just the first.
///
/// Layout: `$RAZDOR_COMMUNITY_DIR/<entry>/install` is a Discord Times install dir (a mod is a
/// modified install; a symlink is fine), and `$RAZDOR_COMMUNITY_DIR/<entry>/maps/*.DTm` are
/// optional extra community maps played with that install's data. Prints one line per map.
/// No time budget: a compatibility check, not a speed check. Passes without the variable.
#[test]
fn community_maps_smoke() {
    let Some(root) = std::env::var_os("RAZDOR_COMMUNITY_DIR") else { return };
    let root = std::path::PathBuf::from(root);
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("RAZDOR_COMMUNITY_DIR {}: {e}", root.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();
    let mut failures: Vec<String> = Vec::new();
    let mut played = 0;
    for entry in entries {
        let name = entry.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let install = entry.join("install");
        if !install.exists() {
            continue;
        }
        let loaded = std::panic::catch_unwind(AssertUnwindSafe(|| DtInstall::load(&install).map(|dt| (Arc::new(Content::from_dt(&dt)), dt))));
        let (c, dt) = match loaded {
            Ok(Ok((c, dt))) => (c, dt),
            Ok(Err(e)) => {
                failures.push(format!("{name}: install does not load: {e}"));
                continue;
            }
            Err(payload) => {
                failures.push(format!("{name}: install does not load: {}", panic_text(&*payload)));
                continue;
            }
        };
        let mut maps = dt.maps.clone();
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
            played += 1;
            let label = format!("{name}/{}", m.name);
            let t0 = Instant::now();
            let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| -> Result<DaysRun, String> {
                let s = m.load().map_err(|e| format!("does not load: {e}"))?;
                Ok(play_days(&c, &s, DAYS))
            }));
            let wall = t0.elapsed().as_secs_f64();
            let why = match outcome {
                Err(payload) => format!("panic: {}", panic_text(&*payload)),
                Ok(Err(why)) => why,
                Ok(Ok(r)) if r.stalled => {
                    println!("{label}: FAIL stalled (softlock), {DAYS} days, AI battles {}, player battles {}, {:.2} s", r.ai.battles, r.player_battles, r.secs);
                    failures.push(format!("{label}: stalled (softlock)"));
                    continue;
                }
                Ok(Ok(r)) => {
                    println!("{label}: ok, {DAYS} days, AI battles {}, player battles {}, {:.2} s", r.ai.battles, r.player_battles, r.secs);
                    continue;
                }
            };
            println!("{label}: FAIL {why}, {DAYS} days, AI battles -, player battles -, {wall:.2} s");
            failures.push(format!("{label}: {why}"));
        }
    }
    if !failures.is_empty() {
        panic!("{} failing item(s) in RAZDOR_COMMUNITY_DIR:\n{}", failures.len(), failures.join("\n"));
    }
    assert!(played > 0, "RAZDOR_COMMUNITY_DIR {} has no <entry>/install with a map to play", root.display());
}
