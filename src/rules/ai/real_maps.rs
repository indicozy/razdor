//! The AI on the player's maps; skipped without `RAZDOR_DT_DIR`. Numbers only.
use std::sync::Arc;
use std::time::Instant;

use super::*;
use crate::dt::install::DtInstall;
use crate::rules::content::HeroClass;
use crate::rules::game::BattleResult;

/// Days simulated on every map.
const DAYS: u64 = 30;
/// Seconds a map may take in a release build.
const RELEASE_BUDGET_SECS: f64 = 5.0;

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
        let t0 = Instant::now();
        let mut g = Game::from_scenario(c.clone(), &s, HeroClass::Knight);
        let end = g.clock.total_minutes() + (DAYS * MINUTES_PER_DAY) as f64;
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
        let secs = t0.elapsed().as_secs_f64();
        let st = g.ai_stats;
        println!(
            "{:<28} {:>3}x{:<3} armies {:>2}→{:>2} waiting {:>2}: AI battles {:>3}, captures {:>2}, respawns {:>2}, hired {:>3}, bought {:>2}, routes {:>5}; player battles {:>2}{}; {:.2} s",
            m.name,
            s.width(),
            s.height(),
            armies,
            g.world.armies.len(),
            g.world.respawns.len(),
            st.battles,
            st.captures,
            st.respawns,
            st.hired,
            st.bought,
            st.paths,
            player_battles,
            if player_lost { " (lost)" } else { "" },
            secs
        );
        assert!(g.clock.total_minutes() >= end, "{}: the simulation stalled", m.name);
        if !cfg!(debug_assertions) {
            assert!(secs < RELEASE_BUDGET_SECS, "{}: {secs:.2} s for {DAYS} days", m.name);
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
