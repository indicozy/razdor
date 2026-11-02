//! Plays a scenario for some days with the AI, headless: the hero stands at his start, fights
//! whoever comes and answers yes to every question. Used by the community map smoke test
//! (`rules::ai::real_maps`) and by `razdor-pack sim`, so both judge a map the same way.

use std::sync::Arc;
use std::time::Instant;

use crate::dt::dtm::Scenario;
use crate::rules::ai::AiStats;
use crate::rules::battle::Outcome;
use crate::rules::clock::MINUTES_PER_DAY;
use crate::rules::content::{Content, HeroClass};
use crate::rules::game::Game;
use crate::rules::script::ScriptEnd;

/// What [`play_days`] saw on one scenario.
#[derive(Debug)]
pub struct DaysRun {
    /// The AI's counts over the run.
    pub ai: AiStats,
    /// Armies at the start and at the end of the run.
    pub armies: usize,
    pub armies_after: usize,
    /// Armies waiting to respawn at the end.
    pub waiting: usize,
    pub player_battles: usize,
    /// The hero lost a battle (the world went on without him).
    pub player_lost: bool,
    /// The clock did not reach the end within the call budget (a softlock).
    pub stalled: bool,
    /// Days after the start when the scenario's victory event fired, or `None`.
    pub victory_day: Option<f64>,
    /// Days after the start when the hero fell (lost a battle) or the defeat event fired.
    pub defeat_day: Option<f64>,
    pub secs: f64,
}

/// Auto-plays the player's pending battle, the AI on both sides.
fn fight_it_out(g: &mut Game) -> Outcome {
    let mut b = g.start_battle();
    b.auto_play_to_end();
    g.resolve_battle(&b);
    b.outcome()
}

/// Plays `days` days of `s` with the hero of class `hero` standing at his start.
pub fn play_days(c: &Arc<Content>, s: &Scenario, hero: HeroClass, days: u64) -> DaysRun {
    let t0 = Instant::now();
    let mut g = Game::from_scenario(c.clone(), s, hero);
    let start = g.clock.total_minutes();
    let end = start + (days * MINUTES_PER_DAY) as f64;
    let armies = g.world.armies.len();
    let mut player_battles = 0;
    let mut player_lost = false;
    let mut victory_day = None;
    let mut defeat_day = None;
    let mut calls = 0;
    let day_now = |g: &Game| (g.clock.total_minutes() - start) / MINUTES_PER_DAY as f64;
    while g.clock.total_minutes() < end && calls < 200_000 {
        calls += 1;
        match g.script_end() {
            Some(ScriptEnd::Victory(_)) if victory_day.is_none() => victory_day = Some(day_now(&g)),
            Some(ScriptEnd::Defeat(_)) if defeat_day.is_none() => defeat_day = Some(day_now(&g)),
            _ => {}
        }
        if g.pending_question().is_some() {
            g.answer_question(true);
            continue;
        }
        if g.foe.is_some() {
            player_battles += 1;
            if fight_it_out(&mut g) == Outcome::Defeat {
                // The game would be over; the world goes on without the player.
                player_lost = true;
                defeat_day.get_or_insert_with(|| day_now(&g));
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
    match g.script_end() {
        Some(ScriptEnd::Victory(_)) if victory_day.is_none() => victory_day = Some(day_now(&g)),
        Some(ScriptEnd::Defeat(_)) if defeat_day.is_none() => defeat_day = Some(day_now(&g)),
        _ => {}
    }
    DaysRun {
        ai: g.ai_stats,
        armies,
        armies_after: g.world.armies.len(),
        waiting: g.world.respawns.len(),
        player_battles,
        player_lost,
        stalled: g.clock.total_minutes() < end,
        victory_day,
        defeat_day,
        secs: t0.elapsed().as_secs_f64(),
    }
}
