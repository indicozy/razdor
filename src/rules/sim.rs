//! Plays a scenario for some days with the AI, headless: the hero stands at his start, fights
//! whoever comes and answers yes to every question. Used by the community map smoke test
//! (`rules::ai::real_maps`) and by `razdor-pack sim`, so both judge a map the same way. A package's
//! story can be attached ([`play_days_with`]): its dialogs are answered by the caller's choice.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use crate::dt::dtm::Scenario;
use crate::rules::ai::AiStats;
use crate::rules::battle::Outcome;
use crate::rules::clock::MINUTES_PER_DAY;
use crate::rules::content::{Content, HeroClass};
use crate::rules::game::{Event, Game};
use crate::rules::script::ScriptEnd;
use crate::rules::triggers::When;
use crate::rules::triggers::{Ending, PendingDialog, Story, Trace};

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
    /// Days after the start when the scenario's victory event (or a package's `victory`) fired.
    pub victory_day: Option<f64>,
    /// Days after the start when the hero fell (lost a battle) or the defeat event fired.
    pub defeat_day: Option<f64>,
    /// What the package's story did, by day after the start, when its trace was asked for.
    pub trace: Vec<(u64, Trace)>,
    pub secs: f64,
}

/// A package's story to run with the scenario: the story itself, whether to keep its trace, and
/// the choice taken in each dialog that opens (`None` for «Далее»).
pub struct StoryPlay<'a> {
    pub story: Story,
    pub trace: bool,
    /// Variables forced before the story starts (`razdor-pack sim --set`): a developer switch.
    pub preset: Vec<(String, crate::pack::triggers::VarValue)>,
    pub pick: &'a mut dyn FnMut(&PendingDialog) -> Option<String>,
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
    play_days_with(c, s, hero, days, None)
}

/// [`play_days`] with a package's story attached: its `game_start` moment is raised first, and
/// the dialogs it opens are answered by `story.pick`.
pub fn play_days_with(c: &Arc<Content>, s: &Scenario, hero: HeroClass, days: u64, mut story: Option<StoryPlay>) -> DaysRun {
    let t0 = Instant::now();
    let mut g = Game::from_scenario(c.clone(), s, hero);
    if let Some(p) = story.as_mut() {
        g.attach_story(p.story.clone());
        // Forced before the story starts, so its game_start moment sees them (`--set`).
        for (name, value) in &p.preset {
            g.triggers.vars.insert(name.clone(), value.clone());
        }
        if p.trace {
            g.triggers.trace = Some(Vec::new());
        }
        g.start_story();
    }
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
        note_ending(&g, &day_now, &mut victory_day, &mut defeat_day);
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
        let events = g.drain_events();
        answer_dialogs(&mut g, events, story.as_mut());
        g.wait(4);
    }
    note_ending(&g, &day_now, &mut victory_day, &mut defeat_day);
    // The trace's days count from the start, as the other days of this run do.
    let day0 = start as u64 / MINUTES_PER_DAY;
    let trace = g.triggers.trace.take().unwrap_or_default().into_iter().map(|(at, what)| (at / MINUTES_PER_DAY - day0, what)).collect();
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
        trace,
        secs: t0.elapsed().as_secs_f64(),
    }
}

/// Notes the scenario's ending and the package's, once each, at the day it came.
fn note_ending(g: &Game, day_now: &dyn Fn(&Game) -> f64, victory_day: &mut Option<f64>, defeat_day: &mut Option<f64>) {
    match g.script_end() {
        Some(ScriptEnd::Victory(_) | ScriptEnd::Package(Ending::Victory)) if victory_day.is_none() => *victory_day = Some(day_now(g)),
        Some(ScriptEnd::Defeat(_) | ScriptEnd::Package(Ending::Defeat)) if defeat_day.is_none() => *defeat_day = Some(day_now(g)),
        _ => {}
    }
}

/// Answers the package's dialogs among `events` with the story's choice; the dialogs an answer
/// opens are answered in turn.
fn answer_dialogs(g: &mut Game, events: Vec<Event>, story: Option<&mut StoryPlay>) {
    let Some(p) = story else { return };
    answer_picks(g, events, &mut *p.pick);
}

/// [`answer_dialogs`] with the choice rule given directly.
fn answer_picks(g: &mut Game, events: Vec<Event>, pick: &mut dyn FnMut(&PendingDialog) -> Option<String>) {
    let mut queue: VecDeque<PendingDialog> = dialogs_of(events).collect();
    while let Some(d) = queue.pop_front() {
        let choice = pick(&d);
        queue.extend(dialogs_of(g.answer_story_dialog(&d.id, choice.as_deref())));
    }
}

/// How a campaign map's play ended (`play_campaign_map`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapEnd {
    /// The scenario's or the story's victory.
    Victory,
    /// The hero fell, or the defeat fired.
    Defeat,
    /// A story asked to travel to another map (`Game::triggers.travel`).
    Travel,
    /// A story reached its ending (`Game::triggers.ending`).
    Ending,
    /// The day limit came first.
    Days,
    /// The clock did not reach the limit within the call budget (a softlock).
    Stalled,
}

/// Plays campaign map `g` (the game of one map, begun) for `days` days, answering its dialogs with
/// `pick`. `auto_win_after` (a developer switch of `razdor-pack sim --auto-win-after`) fires the
/// map's victory once that many days have passed, so a campaign's graph can be walked without the
/// hero reaching the victory. Returns how the map's play ended; the caller takes the travel or
/// the ending from the game.
pub fn play_campaign_map(g: &mut Game, days: u64, auto_win_after: Option<u64>, pick: &mut dyn FnMut(&PendingDialog) -> Option<String>) -> MapEnd {
    let start = g.clock.total_minutes();
    let end = start + (days * MINUTES_PER_DAY) as f64;
    let day_now = |g: &Game| (g.clock.total_minutes() - start) / MINUTES_PER_DAY as f64;
    let mut calls = 0;
    let mut auto_fired = false;
    while g.clock.total_minutes() < end && calls < 200_000 {
        calls += 1;
        if !auto_fired && auto_win_after.is_some_and(|d| day_now(g) >= d as f64) {
            // The story gets its moment first: a chapter's last dialog (a council, a sim end) plays
            // as the victory would have, and an ending it reaches ends the campaign instead.
            auto_fired = true;
            let events = g.story_events(When::SimVictory);
            answer_picks(g, events, pick);
        }
        if auto_fired && g.triggers.ending.is_none() {
            g.triggers.ended.get_or_insert(Ending::Victory);
        }
        match g.script_end() {
            Some(ScriptEnd::Victory(_) | ScriptEnd::Package(Ending::Victory)) => return MapEnd::Victory,
            Some(ScriptEnd::Defeat(_) | ScriptEnd::Package(Ending::Defeat)) => return MapEnd::Defeat,
            None => {}
        }
        if g.pending_question().is_some() {
            g.answer_question(true);
            continue;
        }
        if g.foe.is_some() {
            if fight_it_out(g) == Outcome::Defeat {
                return MapEnd::Defeat;
            }
            continue;
        }
        let events = g.drain_events();
        answer_picks(g, events, pick);
        if g.triggers.travel.is_some() {
            return MapEnd::Travel;
        }
        if g.triggers.ending.is_some() {
            return MapEnd::Ending;
        }
        let waited = g.wait(4);
        answer_picks(g, waited, pick);
    }
    if g.clock.total_minutes() < end {
        MapEnd::Stalled
    } else {
        MapEnd::Days
    }
}

fn dialogs_of(events: Vec<Event>) -> impl Iterator<Item = PendingDialog> {
    events.into_iter().filter_map(|e| if let Event::Package(d) = e { Some(d) } else { None })
}
