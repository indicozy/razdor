//! «Максимум» against the original's AI: the same positions played with the original on
//! both sides and with Max on one, each army on each side.
use std::time::Duration;

use super::*;
use crate::rules::custom::{self, Pick};

/// The deterministic budget of the harness: a few actions ahead, a node cap.
const TEST_BUDGET: MaxBudget = MaxBudget::fixed(4, 600);

/// One battle played to its end, `max` (if any) playing by «Максимум».
struct Played {
    outcome: Outcome,
    battle: Battle,
    decisions: Vec<Duration>,
    reports: Vec<MaxReport>,
}

/// The harness searches without the battle's tables with `RAZDOR_MAX_NO_TABLES` set (to
/// measure what they save).
fn with_tables() -> bool {
    std::env::var_os("RAZDOR_MAX_NO_TABLES").is_none()
}

/// The harness cuts rollouts short (`MaxBudget::short_rollouts`) with `RAZDOR_MAX_SHORT_ROLLOUTS` set, and
/// searches on one thread with `RAZDOR_MAX_SERIAL`.
fn harness(budget: MaxBudget) -> MaxBudget {
    let on = |v: &str| std::env::var_os(v).is_some();
    MaxBudget { short_rollouts: budget.short_rollouts || on("RAZDOR_MAX_SHORT_ROLLOUTS"), parallel: budget.parallel && !on("RAZDOR_MAX_SERIAL"), ..budget }
}

fn play(mut b: Battle, max: Option<Team>, budget: MaxBudget) -> Played {
    let budget = harness(budget);
    b.begin();
    let mut decisions = Vec::new();
    let mut reports = Vec::new();
    let tables = MaxTables::default();
    let tables = with_tables().then_some(&tables);
    for _ in 0..AUTO_PLAY_STEPS {
        let Some(id) = b.active() else { break };
        if Some(b.fighters[id].team) == max {
            let r = b.max_choice_with(budget, tables, None).unwrap();
            decisions.push(r.elapsed);
            reports.push(r);
            b.play_max_move(r.choice);
        } else {
            b.auto_step();
        }
    }
    Played { outcome: b.outcome(), battle: b, decisions, reports }
}

fn won(outcome: Outcome, team: Team) -> bool {
    matches!((outcome, team), (Outcome::Victory, Team::Player) | (Outcome::Defeat, Team::Enemy))
}

/// `team`'s worth left at the end as a share (0..1) of its worth at the start.
fn worth_left(b: &Battle, team: Team) -> f64 {
    let worth = |alive: bool| -> f64 {
        b.fighters
            .iter()
            .filter(|f| f.team == team && (!alive || (f.alive() && !f.surrendered)))
            .map(|f| f.tactical.max(1) as f64 * if alive { f.hp as f64 / f.max_hp().max(1) as f64 } else { 1.0 })
            .sum()
    };
    worth(true) / worth(false).max(1.0)
}

/// The tally of a harness run.
#[derive(Default)]
struct Tally {
    games: u32,
    max_wins: u32,
    orig_wins: u32,
    max_left: f64,
    orig_left: f64,
    decisions: Vec<Duration>,
    /// Summed over the decisions: the depth reached, the positions searched, the rollouts.
    depth: u64,
    nodes: u64,
    rollouts: u64,
    tt_hits: u64,
    rollout_hits: u64,
    /// Positions the original's side wins and Max's loses, or where Max keeps much less.
    worse: Vec<String>,
    /// Positions where Max ends below the original by its own score (none expected: each
    /// decision keeps at least what the original's rollout from there gives).
    below: Vec<String>,
}

impl Tally {
    /// Plays `b` with Max on each side against the original's AI, and the same position
    /// with the original on both sides.
    fn add(&mut self, name: &str, b: &Battle, budget: MaxBudget) {
        let base = play(b.clone(), None, budget);
        for team in Team::BOTH {
            let m = play(b.clone(), Some(team), budget);
            self.games += 1;
            let (mw, ow) = (won(m.outcome, team), won(base.outcome, team));
            self.max_wins += u32::from(mw);
            self.orig_wins += u32::from(ow);
            let (ml, ol) = (worth_left(&m.battle, team), worth_left(&base.battle, team));
            self.max_left += ml;
            self.orig_left += ol;
            if (ow && !mw) || (mw == ow && ol - ml > 0.25) {
                self.worse.push(format!("{name}, Max as {team:?}: original {:?} {ol:.2} left, Max {:?} {ml:.2} left", base.outcome, m.outcome));
            }
            if m.battle.max_eval(team) < base.battle.max_eval(team) {
                self.below.push(format!("{name}, Max as {team:?}"));
            }
            self.decisions.extend(m.decisions);
            for r in &m.reports {
                self.depth += r.depth as u64;
                self.nodes += r.nodes;
                self.rollouts += r.rollouts;
                self.tt_hits += r.tt_hits;
                self.rollout_hits += r.rollout_hits;
            }
        }
    }

    fn report(&self, title: &str) {
        let n = self.games.max(1) as f64;
        let mut d = self.decisions.clone();
        d.sort();
        let at = |q: f64| d.get(((d.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or_default();
        let secs: f64 = d.iter().map(|x| x.as_secs_f64()).sum();
        let k = d.len().max(1) as f64;
        println!(
            "{title}: {} games; wins Max {} ({:.0}%) vs original {} ({:.0}%); worth left Max {:.2} vs original {:.2}; {} decisions, median {:?}, 95% {:?}, max {:?}; depth {:.2}, {:.0} nodes, {:.0} rollouts, {:.0} table hits and {:.0} cached rollouts per decision, {:.0} nodes/s",
            self.games,
            self.max_wins,
            100.0 * self.max_wins as f64 / n,
            self.orig_wins,
            100.0 * self.orig_wins as f64 / n,
            self.max_left / n,
            self.orig_left / n,
            d.len(),
            at(0.5),
            at(0.95),
            d.last().copied().unwrap_or_default(),
            self.depth as f64 / k,
            self.nodes as f64 / k,
            self.rollouts as f64 / k,
            self.tt_hits as f64 / k,
            self.rollout_hits as f64 / k,
            self.nodes as f64 / secs.max(1e-9)
        );
        for w in &self.below {
            println!("  below the original's score: {w}");
        }
        for w in &self.worse {
            println!("  worse: {w}");
        }
    }
}

/// A small deterministic generator for the synthetic armies.
fn lcg(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed >> 33
}

/// A custom battle's armies (`rules::custom`), as the setup screen builds them.
fn custom_battle(c: &Content, armies: [Vec<Pick>; 2], wide: bool) -> Battle {
    let setup = custom::Setup { armies, control: [custom::Control::Ai; 2], wide };
    setup.battle(c, true).unwrap()
}

/// Custom battles of `c`'s unit types: armies of 2 to `most` units at levels 1 to 6, both
/// formations.
fn custom_battles(c: &Content, label: &str, n: usize, most: usize, mut seed: u64) -> Vec<(String, Battle)> {
    let choices = custom::choices(c);
    (0..n)
        .map(|i| {
            let army = |seed: &mut u64| -> Vec<Pick> {
                let len = 2 + lcg(seed) as usize % (most - 1);
                (0..len).map(|_| Pick { unit: choices[lcg(seed) as usize % choices.len()], level: 1 + lcg(seed) as i32 % 6, items: Vec::new() }).collect()
            };
            let armies = [army(&mut seed), army(&mut seed)];
            (format!("{label} #{i}"), custom_battle(c, armies, i % 2 == 0))
        })
        .collect()
}

/// Battles of the demo's units.
fn synthetic_battles(n: usize) -> Vec<(String, Battle)> {
    custom_battles(&Content::builtin(), "demo", n, 6, 7)
}

#[test]
fn max_keeps_the_original_choice_unless_it_finds_better() {
    for (name, mut b) in synthetic_battles(6) {
        b.begin();
        let r = b.max_choice(TEST_BUDGET).unwrap();
        assert!(r.depth >= 1, "{name}: depth 1 completes within the budget");
        assert!(r.value >= r.original_value, "{name}: never below the original's choice");
        if r.choice != r.original {
            assert!(r.value > r.original_value, "{name}: a tie keeps the original's choice");
        }
    }
}

#[test]
fn max_leaves_the_battle_and_the_patch_globals_alone_and_is_deterministic() {
    let (_, mut b) = synthetic_battles(1).remove(0);
    b.begin();
    let globals = patch_globals();
    let before = (b.log.clone(), b.fighters.iter().map(|f| (f.hp, f.slot, f.actions)).collect::<Vec<_>>());
    let first = b.max_choice(TEST_BUDGET).unwrap();
    assert_eq!(patch_globals(), globals);
    assert_eq!((b.log.clone(), b.fighters.iter().map(|f| (f.hp, f.slot, f.actions)).collect::<Vec<_>>()), before);
    let again = b.max_choice(TEST_BUDGET).unwrap();
    assert_eq!((first.choice, first.value, first.nodes), (again.choice, again.value, again.nodes));
}

#[test]
fn max_finishes_every_battle() {
    for (name, b) in synthetic_battles(4) {
        let p = play(b, Some(Team::Enemy), TEST_BUDGET);
        assert_ne!(p.outcome, Outcome::Ongoing, "{name}");
    }
}

/// The off-screen battles and `auto_play_to_end` never use Max, even with it chosen (the
/// battle window's Q and W ask for it with `max_play_to_end` and its thinkers).
#[test]
fn only_the_interface_asks_for_max() {
    let (_, mut b) = synthetic_battles(1).remove(0);
    let mut plain = b.clone();
    b.set_max_ai(true);
    assert!(b.max_ai());
    assert_eq!(b.auto_play_to_end(), plain.auto_play_to_end());
    assert_eq!(b.log, plain.log);
}

/// Max against the original's AI on the demo's units, Max on each side of each position.
#[test]
fn max_beats_the_original_on_synthetic_battles() {
    let mut t = Tally::default();
    for (name, b) in synthetic_battles(24) {
        t.add(&name, &b, TEST_BUDGET);
    }
    t.report("synthetic");
    assert!(t.below.is_empty(), "Max never ends below the original's score");
    assert!(t.max_wins >= t.orig_wins, "Max wins at least as often");
    assert!(t.max_left >= t.orig_left, "and keeps at least as much");
}

/// [`MAX_AI_BUDGET`] × `RAZDOR_PERF_BUDGET_SCALE` (2 on the loaded E-cores), with a quarter
/// over for the clock checks between rollout steps.
fn decision_limit() -> Duration {
    let scale = std::env::var("RAZDOR_PERF_BUDGET_SCALE").ok().and_then(|s| s.trim().parse::<f64>().ok()).filter(|s| *s > 0.0);
    MAX_AI_BUDGET.mul_f64(1.25 * scale.unwrap_or(1.0))
}

/// The game's own budget: every decision returns within its time.
#[test]
fn a_decision_keeps_to_its_time() {
    if cfg!(debug_assertions) {
        return;
    }
    let mut longest = Duration::ZERO;
    for (_, b) in synthetic_battles(3) {
        let p = play(b, Some(Team::Enemy), MaxBudget::GAME);
        longest = longest.max(p.decisions.iter().copied().max().unwrap_or_default());
    }
    assert!(longest <= decision_limit(), "{longest:?}");
}

/// The armies of the player's maps (`RAZDOR_DT_DIR`): each map's first armies against the
/// hero's army as the game sets up the battle, armies of near strength against each other,
/// and custom battles of the install's unit types; Max on each side of each. Prints the
/// tally; Max must win at least as often as the original and never end below its score.
#[test]
fn max_beats_the_original_on_real_armies() {
    real_armies(TEST_BUDGET);
}

/// The same with the game's time budget per decision (slow: run by hand with `--ignored`).
#[test]
#[ignore]
fn max_beats_the_original_on_real_armies_in_game_time() {
    real_armies(MaxBudget::GAME);
}

/// Named positions to play.
type Battles = Vec<(String, Battle)>;

/// The install's battle sets (`RAZDOR_DT_DIR`; `None` without it): each map's first armies
/// against the hero's army as the game sets up the battle, map armies of near strength
/// against each other (each on each side), custom battles of the install's unit types.
fn real_sets() -> Option<Vec<(&'static str, Battles)>> {
    use crate::dt::install::DtInstall;
    use crate::rules::content::HeroClass;
    use crate::rules::game::{Foe, Game};
    let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
    let dt = DtInstall::load(std::path::Path::new(&dir)).unwrap();
    let c = Arc::new(Content::from_dt(&dt));
    let mut hero = Vec::new();
    let mut armies: Vec<(String, i64, Vec<Unit>)> = Vec::new();
    for m in &dt.maps {
        let s = m.load().unwrap();
        let g0 = Game::from_scenario(c.clone(), &s, HeroClass::Knight);
        for (i, a) in g0.world.armies.iter().enumerate() {
            let units = crate::rules::ai::army_units(&c, a);
            if units.is_empty() {
                continue;
            }
            if i < 2 {
                let mut g = Game::from_scenario(c.clone(), &s, HeroClass::Knight);
                g.drain_events();
                g.improved_ai = true;
                g.foe = Some(Foe::Army(i));
                hero.push((format!("{} hero vs army {i}", m.name), g.start_battle()));
            }
            let strength = units.iter().map(|u| u.tactical(&c, 0) as i64).sum();
            armies.push((format!("{} army {i}", m.name), strength, units));
        }
    }
    // Neighbours by strength, every third pair, up to 30, each army on each side.
    armies.sort_by_key(|a| a.1);
    let mut pairs = Vec::new();
    for w in armies.windows(2).step_by(3).take(30) {
        for (p, e) in [(&w[0], &w[1]), (&w[1], &w[0])] {
            let squad: Vec<(usize, &Unit)> = p.2.iter().enumerate().map(|(k, u)| (k + 1, u)).collect();
            let mut b = Battle::new(c.clone(), &squad, &e.2, Team::Player);
            b.set_improved_ai(true);
            b.auto_arrange(Team::Player);
            b.auto_arrange(Team::Enemy);
            pairs.push((format!("{} vs {}", p.0, e.0), b));
        }
    }
    let customs = custom_battles(&c, "custom", 20, 8, 11);
    Some(vec![("real: hero vs map armies", hero), ("real: map armies of near strength", pairs), ("real: custom battles", customs)])
}

fn real_armies(budget: MaxBudget) {
    let Some(sets) = real_sets() else { return };
    let mut all = Tally::default();
    for (title, set) in sets {
        let mut t = Tally::default();
        for (name, b) in &set {
            t.add(name, b, budget);
        }
        t.report(title);
        all.games += t.games;
        all.max_wins += t.max_wins;
        all.orig_wins += t.orig_wins;
        all.max_left += t.max_left;
        all.orig_left += t.orig_left;
        all.decisions.extend(t.decisions);
        all.depth += t.depth;
        all.nodes += t.nodes;
        all.rollouts += t.rollouts;
        all.tt_hits += t.tt_hits;
        all.rollout_hits += t.rollout_hits;
        all.below.extend(t.below);
    }
    all.report("real: all");
    if budget.time.is_some() && !cfg!(debug_assertions) {
        let longest = all.decisions.iter().copied().max().unwrap_or_default();
        assert!(longest <= decision_limit(), "{longest:?}");
    }
    assert!(all.below.is_empty(), "Max never ends below the original's score");
    assert!(all.max_wins >= all.orig_wins, "Max wins at least as often");
}

/// Positions along the battles of the demo's units played by the original's AI: every
/// `every`-th action's, up to `n` of them.
fn positions(battles: usize, every: usize, n: usize) -> Vec<(String, Battle)> {
    let mut out = Vec::new();
    for (name, mut b) in synthetic_battles(battles) {
        b.begin();
        for i in 0..AUTO_PLAY_STEPS {
            if b.active().is_none() || out.len() >= n {
                break;
            }
            if i % every == 0 {
                out.push((format!("{name} at action {i}"), b.clone()));
            }
            b.auto_step();
        }
    }
    out
}

/// The tables only save work: at the same fixed depth the choice and its score are the same
/// without them, with fresh ones, and with ones that hold earlier searches (a shallower one of
/// the same position, the positions before it, the other side's).
#[test]
fn tables_never_change_the_choice_at_a_fixed_depth() {
    let depth = if cfg!(debug_assertions) { 2 } else { 3 };
    let budget = MaxBudget::fixed(depth, u64::MAX);
    let warm = MaxTables::default();
    let mut compared = 0;
    for (name, b) in positions(6, 5, 18) {
        let cold = b.max_choice_with(budget, None, None).unwrap();
        let fresh = b.max_choice_with(budget, Some(&MaxTables::default()), None).unwrap();
        let _ = b.max_choice_with(MaxBudget::fixed(depth - 1, u64::MAX), Some(&warm), None);
        let mut other = b.clone();
        if let Some(id) = other.active() {
            other.fighters[id].team = other.fighters[id].team.other();
            let _ = other.max_choice_with(MaxBudget::fixed(depth - 1, u64::MAX), Some(&warm), None);
        }
        let warmed = b.max_choice_with(budget, Some(&warm), None).unwrap();
        let again = b.max_choice_with(budget, Some(&warm), None).unwrap();
        for (what, r) in [("fresh tables", fresh), ("warm tables", warmed), ("the same search again", again)] {
            assert_eq!((r.choice, r.value, r.original_value), (cold.choice, cold.value, cold.original_value), "{name}: {what}");
        }
        assert!(again.nodes <= cold.nodes, "{name}: the tables save work");
        compared += 1;
    }
    assert!(compared >= 10);
}

/// The key tells apart what the battle's future depends on and nothing else: a clone has
/// the same key, its log does not count, a point of HP or a patch global does.
#[test]
fn the_state_key_sees_the_battle_and_the_patch_globals() {
    let (_, mut b) = synthetic_battles(1).remove(0);
    b.begin();
    let k = b.max_key(Team::Enemy);
    let mut c = b.clone();
    assert_eq!(c.max_key(Team::Enemy), k);
    c.log.push("x".into());
    assert_eq!(c.max_key(Team::Enemy), k, "the log does not count");
    assert_ne!(b.max_key(Team::Player), k, "the searching side does");
    c.fighters[0].hp -= 1;
    assert_ne!(c.max_key(Team::Enemy), k, "the HP does");
    let g = patch_globals();
    set_patch_globals(PatchGlobals { hunger_seen: g.hunger_seen + 1, ..g });
    assert_ne!(b.max_key(Team::Enemy), k, "the patch's globals do");
    set_patch_globals(g);
    let mut d = b.clone();
    d.auto_step();
    assert_ne!(d.max_key(Team::Enemy), k);
}

/// The thinker on a worker, thinking ahead after every action: each answer is for the
/// position on screen, never below the original's choice, and the battle ends.
#[test]
fn the_thinker_answers_for_the_position_on_screen() {
    let (_, mut b) = synthetic_battles(2).remove(1);
    b.begin();
    let mut t = MaxThinker::new(Team::Enemy);
    let mut answered = 0;
    for _ in 0..AUTO_PLAY_STEPS {
        let Some(id) = b.active() else { break };
        t.think_ahead(&b);
        if b.fighters[id].team != Team::Enemy {
            b.auto_step();
            continue;
        }
        let r = loop {
            match t.poll(&b) {
                MaxPoll::Thinking => std::thread::sleep(Duration::from_millis(1)),
                MaxPoll::Ready(r) => break r,
                MaxPoll::Failed => panic!("the worker failed"),
            }
        };
        assert!(r.depth >= 1 || r.settled);
        assert!(r.value >= r.original_value);
        b.play_max_move(r.choice);
        answered += 1;
    }
    assert_ne!(b.outcome(), Outcome::Ongoing);
    assert!(answered > 0);
    assert_eq!(t.pauses.len(), answered);
}

/// The battle on screen as a timeline: each action's animation (`anim`: an action, a step,
/// a pass) and the player's time to think, with Max on the enemy's side thinking ahead
/// (`ponder`) or only when asked. Gives the battle at its end and, per enemy action, the wait
/// from the ask to the answer and the depth reached.
fn timeline(mut b: Battle, ponder: bool, anim: [Duration; 3], player: Duration) -> (Battle, Vec<Duration>, Vec<u32>) {
    b.begin();
    let mut t = MaxThinker::new(Team::Enemy);
    t.set_parallel(harness(MaxBudget::GAME).parallel);
    let mut depths = Vec::new();
    for _ in 0..AUTO_PLAY_STEPS {
        if ponder {
            t.think_ahead(&b);
        }
        let Some(id) = b.active() else { break };
        let step = if b.fighters[id].team == Team::Enemy {
            loop {
                match t.poll(&b) {
                    MaxPoll::Thinking => std::thread::sleep(Duration::from_micros(500)),
                    MaxPoll::Ready(r) => {
                        depths.push(r.depth);
                        break b.play_max_move(r.choice);
                    }
                    MaxPoll::Failed => break b.auto_step(),
                }
            }
        } else {
            std::thread::sleep(player);
            b.auto_step()
        };
        if ponder {
            t.think_ahead(&b);
        }
        std::thread::sleep(match step {
            Some(Step::Act { .. }) => anim[0],
            Some(Step::Move { .. }) => anim[1],
            _ => anim[2],
        });
    }
    t.rest();
    (b, t.pauses, depths)
}

/// The pause per enemy action in game time, with thinking ahead and without, on battles of
/// the demo's units and (with `RAZDOR_DT_DIR`) custom battles of the install's units: about
/// the battle view's animations (an action ~0.55 s, a step and a pass 0.1 s) and a player who
/// takes 0.5 s per action. Slow: run by hand with `--ignored`.
#[test]
#[ignore]
fn max_pause_per_action_with_pondering() {
    let anim = [Duration::from_millis(550), Duration::from_millis(100), Duration::from_millis(100)];
    let player = Duration::from_millis(500);
    let mut set = synthetic_battles(6);
    if let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) {
        let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).unwrap();
        set.extend(custom_battles(&Content::from_dt(&dt), "custom", 6, 8, 11));
    }
    for ponder in [false, true] {
        let (mut pauses, mut depths, mut wins, mut left) = (Vec::new(), Vec::new(), 0, 0.0);
        for (_, b) in &set {
            let (end, p, d) = timeline(b.clone(), ponder, anim, player);
            wins += u32::from(won(end.outcome(), Team::Enemy));
            left += worth_left(&end, Team::Enemy);
            pauses.extend(p);
            depths.extend(d);
        }
        pauses.sort();
        let at = |q: f64| pauses.get(((pauses.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or_default();
        let mean = pauses.iter().map(Duration::as_secs_f64).sum::<f64>() / pauses.len().max(1) as f64;
        let depth = depths.iter().sum::<u32>() as f64 / depths.len().max(1) as f64;
        println!(
            "pondering {ponder}: {} battles, Max wins {wins}, worth left {:.2}; {} enemy actions, pause mean {:.1} ms, median {:?}, 95% {:?}, max {:?}; depth {depth:.2}",
            set.len(),
            left / set.len() as f64,
            pauses.len(),
            mean * 1000.0,
            at(0.5),
            at(0.95),
            pauses.last().copied().unwrap_or_default()
        );
    }
}

/// The key's hash is MurmurHash3 x64 128 (its reference values, seed 0).
#[test]
fn the_key_hash_is_murmur3() {
    assert_eq!(tables::murmur3_128(b""), 0);
    let h = tables::murmur3_128(b"hello");
    assert_eq!((h as u64, (h >> 64) as u64), (0xcbd8a7b341bd9b02, 0x5b1e906a48ae1d19));
}

/// Everything of a battle the search could change: its key for both sides, every fighter
/// field, the log.
fn snapshot(b: &Battle) -> (StateKey, StateKey, String, Vec<String>) {
    (b.max_key(Team::Player), b.max_key(Team::Enemy), format!("{:?}", b.fighters), b.log.clone())
}

/// A battle copied into the buffer of another (`clone_from`, the search's copies) is the
/// battle itself, and plays on the same; a search leaves the battle it searched exactly as
/// it was.
#[test]
fn copies_into_old_buffers_are_exact_and_the_search_changes_nothing() {
    let battles: Vec<Battle> = synthetic_battles(4)
        .into_iter()
        .map(|(_, mut b)| {
            b.begin();
            for _ in 0..3 {
                b.auto_step();
            }
            b
        })
        .collect();
    for a in &battles {
        for old in &battles {
            let mut c = old.clone();
            c.clone_from(a);
            assert_eq!(snapshot(&c), snapshot(a));
            let mut d = a.clone();
            for _ in 0..6 {
                assert_eq!(c.auto_step(), d.auto_step());
            }
            assert_eq!(snapshot(&c), snapshot(&d));
        }
        let before = (snapshot(a), patch_globals());
        let _ = a.max_choice_with(TEST_BUDGET, Some(&MaxTables::default()), None);
        assert_eq!((snapshot(a), patch_globals()), before);
    }
}

/// One rollout sample for the static evaluation's fit: at a position `rounds` rounds into a
/// rollout, the searching side's and the other's worth, the rounds left to the limit, and the
/// full rollout's end (its score, the steps it took after that position and before it).
struct Sample {
    own: i64,
    other: i64,
    rounds_left: i64,
    end: i64,
    steps_after: u32,
    steps_before: u32,
}

/// Rollouts from positions along the harness battles (the install's sets with
/// `RAZDOR_DT_DIR`, and the demo's), each sampled `rounds` rounds in.
fn rollout_samples(rounds: u32) -> Vec<Sample> {
    let mut sets: Vec<(String, Battle)> = synthetic_battles(24);
    for (_, set) in real_sets().unwrap_or_default() {
        sets.extend(set);
    }
    let silent = crate::i18n::set_silent(true);
    let saved = patch_globals();
    let mut out = Vec::new();
    for (_, mut b) in sets {
        b.begin();
        for i in 0..AUTO_PLAY_STEPS {
            let Some(id) = b.active() else { break };
            if i % 3 == 0 {
                let side = b.fighters[id].team;
                let g = patch_globals();
                let mut r = b.clone();
                let r0 = r.round;
                let mut steps_before = 0;
                while r.outcome() == Outcome::Ongoing && r.round < r0 + rounds && steps_before < AUTO_PLAY_STEPS as u32 {
                    r.auto_step();
                    steps_before += 1;
                }
                if r.outcome() == Outcome::Ongoing {
                    let (own, other, rounds_left) = (r.max_worth(side), r.max_worth(side.other()), r.turn_limit() as i64 - r.round as i64);
                    let mut steps_after = 0;
                    while r.outcome() == Outcome::Ongoing && steps_after < AUTO_PLAY_STEPS as u32 {
                        r.auto_step();
                        steps_after += 1;
                    }
                    out.push(Sample { own, other, rounds_left, end: r.max_eval(side), steps_after, steps_before });
                }
                set_patch_globals(g);
            }
            b.auto_step();
        }
    }
    set_patch_globals(saved);
    crate::i18n::set_silent(silent);
    out
}

/// The fit of the static evaluation of the cut rollouts (`SHORT_*`): how often the side with
/// the larger share of the worth `SHORT_ROUNDS` rounds into a rollout wins, by share, and how
/// well the winner's worth left at the end is foretold. Deterministic; prints its table (run
/// by hand with `--ignored`; the constants come from it).
#[test]
#[ignore]
fn fit_the_static_evaluation_of_cut_rollouts() {
    for rounds in [1, 2, 3] {
        let samples = rollout_samples(rounds);
        let n = samples.len().max(1) as f64;
        let after: f64 = samples.iter().map(|s| s.steps_after as f64).sum::<f64>() / n;
        let before: f64 = samples.iter().map(|s| s.steps_before as f64).sum::<f64>() / n;
        println!("{rounds} round(s) in: {} samples, {before:.1} steps before and {after:.1} after", samples.len());
        for margin in [2, 4] {
            for share in [300, 400, 500, 600, 700, 800] {
                let decisive: Vec<&Sample> = samples
                    .iter()
                    .filter(|s| s.rounds_left > margin && (s.own - s.other).abs() * 1000 >= share * (s.own + s.other).max(1))
                    .collect();
                let right = decisive.iter().filter(|s| (s.own > s.other) == (s.end > WIN / 2)).count();
                let (mut e_diff, mut e_lan) = (0.0, [0.0; 4]);
                for s in &decisive {
                    let end = (s.end.abs() - WIN) as f64;
                    let (w, l) = if s.own > s.other { (s.own, s.other) } else { (s.other, s.own) };
                    e_diff += ((w - l) as f64 - end).abs();
                    for (k, c) in [0.25, 0.5, 0.75, 1.0].into_iter().enumerate() {
                        e_lan[k] += (((w * w) as f64 - c * (l * l) as f64).max(0.0).sqrt() - end).abs();
                    }
                }
                let d = decisive.len().max(1) as f64;
                let saved: f64 = decisive.iter().map(|s| s.steps_after as f64).sum::<f64>() / n;
                println!(
                    "  margin {margin}, share ≥ {share}‰: decisive {:.1}%, right {:.2}%, steps saved {saved:.1} per rollout; winner's worth error: diff {:.0}, lanchester c=¼ {:.0}, ½ {:.0}, ¾ {:.0}, 1 {:.0}",
                    100.0 * decisive.len() as f64 / n,
                    100.0 * right as f64 / d,
                    e_diff / d,
                    e_lan[0] / d,
                    e_lan[1] / d,
                    e_lan[2] / d,
                    e_lan[3] / d
                );
            }
        }
    }
}

/// The cut rollouts' static score: none while the sides are close, the win for the side far
/// ahead; with them on, the first iteration still weighs every action by full rollouts, so
/// Max still never picks below the original's choice.
#[test]
fn cut_rollouts_score_only_sure_positions_and_keep_the_safety_rule() {
    let (_, mut b) = synthetic_battles(1).remove(0);
    b.begin();
    let (own, other) = (b.max_worth(Team::Enemy), b.max_worth(Team::Player));
    if (own - other).abs() * 1000 < SHORT_SHARE * (own + other) {
        assert_eq!(b.max_static(Team::Enemy), None);
    }
    let mut ahead = b.clone();
    for f in ahead.fighters.iter_mut().filter(|f| f.team == Team::Player).skip(1) {
        f.hp = 0;
    }
    if let Some(f) = ahead.fighters.iter_mut().find(|f| f.team == Team::Player) {
        f.hp = 1;
    }
    let v = ahead.max_static(Team::Enemy).expect("far ahead");
    assert!(v > WIN);
    assert_eq!(ahead.max_static(Team::Player).map(|v| v < -WIN), Some(true));
    let budget = MaxBudget { short_rollouts: true, ..TEST_BUDGET };
    for (name, mut b) in synthetic_battles(4) {
        b.begin();
        let r = b.max_choice_with(budget, Some(&MaxTables::default()), None).unwrap();
        assert!(r.value >= r.original_value, "{name}");
    }
}

/// The root's moves on several threads give the sequential search's choice and scores at
/// the same depth, with fresh tables, shared ones and none, whatever the threads' timing.
#[test]
fn threads_never_change_the_choice_at_a_fixed_depth() {
    let depth = if cfg!(debug_assertions) { 2 } else { 3 };
    let serial = MaxBudget::fixed(depth, u64::MAX);
    let parallel = MaxBudget { parallel: true, ..serial };
    let shared = MaxTables::default();
    for (name, b) in positions(6, 5, 18) {
        let one = b.max_choice_with(serial, None, None).unwrap();
        for (what, r) in [
            ("no tables", b.max_choice_with(parallel, None, None).unwrap()),
            ("fresh tables", b.max_choice_with(parallel, Some(&MaxTables::default()), None).unwrap()),
            ("shared tables", b.max_choice_with(parallel, Some(&shared), None).unwrap()),
            ("shared tables again", b.max_choice_with(parallel, Some(&shared), None).unwrap()),
        ] {
            assert_eq!((r.choice, r.value, r.original_value), (one.choice, one.value, one.original_value), "{name}: {what}");
        }
    }
}

/// A battle AI level of the round robin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    /// «Как в оригинале»: the original's AI, the improved setting off.
    Original,
    /// «Улучшенный»: the original's AI with `OptValue9`.
    Improved,
    /// «Максимум» at the harness's fixed budget (`TEST_BUDGET`: 4 actions ahead, 600
    /// positions), its tables kept for the battle.
    Max,
}

impl Level {
    const ALL: [Level; 3] = [Level::Original, Level::Improved, Level::Max];

    fn name(self) -> &'static str {
        match self {
            Level::Original => "original",
            Level::Improved => "improved",
            Level::Max => "max",
        }
    }

    /// The battle's AI level (`ai_level`) while this level plays `team`'s move. The only
    /// thing the level changes is the kill test of the side that moves: one blow for the
    /// original's computer side, all its actions for the improved one; the player's side at
    /// the normal level already counts all its actions (the original's), so the original's
    /// play on the player's side takes level 0 (one blow for both sides, the simulation's)
    /// to play as the original's computer side does. Max's search runs at level 2, as the
    /// game sets it (the setting implies the improved AI).
    fn ai_level(self, team: Team) -> u8 {
        match (self, team) {
            (Level::Original, Team::Enemy) => 1,
            (Level::Original, Team::Player) => 0,
            (Level::Improved, Team::Enemy) => 2,
            (Level::Improved, Team::Player) => 1,
            (Level::Max, _) => 2,
        }
    }
}

/// One game of the round robin: `player` plays the player's side, `enemy` the computer's.
/// Gives the battle at its end and Max's decision times.
fn duel(mut b: Battle, player: Level, enemy: Level) -> (Battle, Vec<Duration>) {
    b.begin();
    let tables = [MaxTables::default(), MaxTables::default()];
    let mut times = Vec::new();
    for _ in 0..AUTO_PLAY_STEPS {
        let Some(id) = b.active() else { break };
        let team = b.fighters[id].team;
        let level = if team == Team::Player { player } else { enemy };
        b.ai_level = level.ai_level(team);
        if level == Level::Max {
            let r = b.max_choice_with(TEST_BUDGET, Some(&tables[team.index()]), None).unwrap();
            times.push(r.elapsed);
            b.play_max_move(r.choice);
        } else {
            b.auto_step();
        }
    }
    (b, times)
}

/// The three battle AI levels head to head on 40 positions (with `RAZDOR_DT_DIR` every
/// few of the install's sets: hero vs map armies, map armies of near strength, custom
/// battles; else the demo's): each pair, every position with each level on each side.
/// Prints a table per pair: games, wins of each, ends by the turn limit (the player's side
/// wins those) and games that never ended, the average share of its worth each keeps, and
/// Max's decision times at its fixed budget. A few minutes: run by hand with `--ignored`.
#[test]
#[ignore]
fn round_robin_of_the_ai_levels() {
    let set: Battles = match real_sets() {
        Some(sets) => sets.into_iter().flat_map(|(_, s)| {
            let step = (s.len() / 13).max(1);
            s.into_iter().step_by(step).take(14)
        }).take(40).collect(),
        None => synthetic_battles(40),
    };
    println!("| pair (a vs b) | games | a wins | b wins | turn limit | unfinished | a's worth left | b's worth left | Max's decisions: median / 95% / max |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for (i, &a) in Level::ALL.iter().enumerate() {
        for &b in &Level::ALL[i + 1..] {
            let (mut games, mut wins, mut limit, mut unfinished, mut left) = (0, [0; 2], 0, 0, [0.0; 2]);
            let mut times = Vec::new();
            for (_, battle) in &set {
                // a on the player's side, then a on the computer's.
                for (player, enemy, a_team) in [(a, b, Team::Player), (b, a, Team::Enemy)] {
                    let (end, t) = duel(battle.clone(), player, enemy);
                    games += 1;
                    times.extend(t);
                    for (k, team) in [a_team, a_team.other()].into_iter().enumerate() {
                        wins[k] += u32::from(won(end.outcome(), team));
                        left[k] += worth_left(&end, team);
                    }
                    limit += u32::from(end.end_reason() == Some(EndReason::TurnLimit));
                    unfinished += u32::from(end.outcome() == Outcome::Ongoing);
                }
            }
            times.sort();
            let at = |q: f64| times.get(((times.len() as f64 - 1.0) * q).round() as usize).map_or("—".to_string(), |d| format!("{:.0} ms", d.as_secs_f64() * 1000.0));
            let g = games.max(1) as f64;
            println!(
                "| {} vs {} | {games} | {} ({:.0}%) | {} ({:.0}%) | {limit} | {unfinished} | {:.2} | {:.2} | {} / {} / {} |",
                a.name(),
                b.name(),
                wins[0],
                100.0 * wins[0] as f64 / g,
                wins[1],
                100.0 * wins[1] as f64 / g,
                left[0] / g,
                left[1] / g,
                at(0.5),
                at(0.95),
                at(1.0)
            );
        }
    }
}

/// Who plays a side of the hero's auto-battle (Q) in [`auto_battle`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Auto {
    /// The hero's auto-battle without «Максимум» (`hero_auto_step`: the original's AI, the
    /// player's units by the hero's rules).
    Hero,
    Max,
}

/// One quick battle: each side by `who`, Max at `budget` with its own tables per side.
fn auto_battle(mut b: Battle, who: [Auto; 2], budget: MaxBudget) -> (Battle, Vec<Duration>) {
    b.begin();
    let tables = [MaxTables::default(), MaxTables::default()];
    let mut times = Vec::new();
    for _ in 0..AUTO_PLAY_STEPS {
        let Some(id) = b.active() else { break };
        let team = b.fighters[id].team;
        if who[team.index()] == Auto::Max {
            let r = b.max_choice_with(budget, Some(&tables[team.index()]), None).unwrap();
            times.push(r.elapsed);
            b.play_max_move(r.choice);
        } else {
            b.hero_auto_step();
        }
    }
    (b, times)
}

/// The quick battle with «Максимум» chosen (the 2026-10-18 reports), at its own budget
/// (`MAX_QUICK_BUDGET`), on the install's hero battles (with `RAZDOR_MAX_ALL_SETS` all its
/// sets; else the demo's): the player's side by the hero's auto-battle (as before) or by
/// Max, the enemy by Max (as when the player plays himself) and, with `RAZDOR_MAX_ALL_SETS`,
/// by the hero's auto-battle (the original's AI) too. Prints the player's wins and worth
/// left per pairing; against Max, Max on the player's side must win at least as often as
/// the hero's auto-battle and keep at least as much.
#[test]
fn max_plays_the_hero_s_side_of_the_quick_battle_better() {
    let all = std::env::var_os("RAZDOR_MAX_ALL_SETS").is_some();
    let set: Battles = match real_sets() {
        Some(sets) => sets.into_iter().take(if all { 3 } else { 1 }).flat_map(|(_, s)| s).collect(),
        None => synthetic_battles(12),
    };
    let mut pairs = vec![[Auto::Hero, Auto::Max], [Auto::Max, Auto::Max]];
    if all {
        pairs.extend([[Auto::Hero, Auto::Hero], [Auto::Max, Auto::Hero]]);
    }
    let mut tally: Vec<(u32, f64, Vec<Duration>)> = vec![Default::default(); pairs.len()];
    for (_, b) in &set {
        for (k, who) in pairs.iter().enumerate() {
            let (end, t) = auto_battle(b.clone(), *who, MAX_QUICK_BUDGET);
            tally[k].0 += u32::from(end.outcome() == Outcome::Victory);
            tally[k].1 += worth_left(&end, Team::Player);
            tally[k].2.extend(t);
        }
    }
    let n = set.len().max(1) as f64;
    for (who, (wins, left, times)) in pairs.iter().zip(&mut tally) {
        times.sort();
        let at = |q: f64| times.get(((times.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or_default();
        println!(
            "player {:?} vs enemy {:?}: {} battles, player wins {wins} ({:.0}%), worth left {:.3}; Max's decisions median {:?}, 95% {:?}, all {:?}",
            who[0],
            who[1],
            set.len(),
            100.0 * *wins as f64 / n,
            *left / n,
            at(0.5),
            at(0.95),
            times.iter().sum::<Duration>()
        );
    }
    assert!(tally[1].0 >= tally[0].0, "against Max, Max on the hero's side wins at least as often as the hero's auto-battle");
    assert!(tally[1].1 >= tally[0].1, "and keeps at least as much");
}

/// #18: the rollouts played ahead on other threads ([`MaxBudget::prefetch`]) change nothing
/// the search finds: every decision of whole quick battles (Max on both sides, each with its
/// tables) has the same choice, scores, depth and positions with them or without.
#[test]
fn prefetched_rollouts_change_no_decision() {
    let serial = MaxBudget { prefetch: false, ..MAX_QUICK_BUDGET };
    for (name, b) in synthetic_battles(6) {
        let decide = |budget: MaxBudget| {
            let mut b = b.clone();
            b.begin();
            let tables = [MaxTables::default(), MaxTables::default()];
            let mut seen = Vec::new();
            for _ in 0..AUTO_PLAY_STEPS {
                let Some(id) = b.active() else { break };
                let side = b.fighters[id].team.index();
                let r = b.max_choice_with(budget, Some(&tables[side]), None).unwrap();
                seen.push((r.choice, r.value, r.original_value, r.depth, r.nodes, r.settled));
                b.play_max_move(r.choice);
            }
            (seen, b.log.clone(), b.outcome())
        };
        assert_eq!(decide(MAX_QUICK_BUDGET), decide(serial), "{name}");
    }
}

/// #18: the slowest of 120 quick battles with «Максимум» of the install's units (23 fighters,
/// 24 rounds: 10.5 s on a quiet E-core before, about 5 s on 3) keeps under 8 s, scaled by
/// `RAZDOR_PERF_BUDGET_SCALE` on slow cores. It takes the cores it measures, so it runs by
/// hand (`--ignored`, alone): among the other tests it is neither fair to them nor to itself.
#[test]
#[ignore]
fn a_long_quick_battle_keeps_to_its_time() {
    use crate::dt::install::DtInstall;
    if cfg!(debug_assertions) || max_threads() < 3 {
        return;
    }
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = DtInstall::load(std::path::Path::new(&dir)).unwrap();
    let c = Arc::new(Content::from_dt(&dt));
    let (name, mut b) = custom_battles(&c, "custom", 37, 12, 23).remove(36);
    let t = std::time::Instant::now();
    b.max_play_to_end(MAX_QUICK_BUDGET);
    let scale = std::env::var("RAZDOR_PERF_BUDGET_SCALE").ok().and_then(|s| s.trim().parse::<f64>().ok()).filter(|s| *s > 0.0).unwrap_or(1.0);
    assert!(t.elapsed() <= Duration::from_secs(8).mul_f64(scale), "{name}: {:?}", t.elapsed());
}
