//! The «Максимум» benchmark (`docs/flows/battle-ai.md` §7). A fixed set of battles, each
//! played to its end three ways: Max on the enemy's side, Max on the player's side (the
//! original's AI on the other), and the original's AI on both sides (the baseline). Each game
//! is tallied into numbers (winner, turns, each side's worth left, Max's blunder counts, its
//! decisions) and compared with the committed `bench_baseline.json`. Node budgets only, so
//! the numbers are the same on every machine.
//!
//! `max_bench` (ignored; the whole set, with the install's battles when `RAZDOR_DT_DIR` is
//! set) prints the summary and the per-battle diff against the baseline; with
//! `RAZDOR_MAX_BENCH_WRITE=1` it rewrites the baseline (merging the battles it did not run).
//! `max_bench_does_not_regress` is the fast check: the demo's battles, and every
//! [`FAST_EVERY`]-th install battle when `RAZDOR_DT_DIR` is set.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::tests::{real_sets, synthetic_battles, worth_left, Battles};
use super::*;

/// The budget of every game: 4 actions ahead and 600 positions, no clock (the tests' budget).
const BENCH_BUDGET: MaxBudget = MaxBudget::fixed(4, 600);
/// The demo's battles in the set (all of them in the fast check too).
const DEMO_BATTLES: usize = 24;
/// The benchmark plays every this-many install battles, the fast check every [`FAST_EVERY`]-th
/// of the install's (a subset of the benchmark's).
const BENCH_EVERY: usize = 2;
const FAST_EVERY: usize = 4;
/// The fast check's tolerance for the mean worth Max keeps, in permille of the worth at the start.
const KEPT_TOLERANCE: i64 = 10;

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/rules/battle/max_ai/bench_baseline.json")
}

/// One battle played to its end, and the tally of its decisions.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
struct Game {
    /// `player`, `enemy`, or `none` (still ongoing at the step cap).
    winner: String,
    /// The battle ended at the turn limit, a win for the player.
    turn_limit: bool,
    turns: u32,
    /// Each side's worth left at the end, in permille of its worth at the start.
    player_kept: i64,
    enemy_kept: i64,
    /// The decisions of the counted side(s): Max's, or both sides' in the original's game.
    decisions: u32,
    /// Steps into the reserve by a unit from outside it (`max_never_steps_into_the_reserve`).
    reserve: u32,
    /// A pass, a step or a refused plan while a damaging strike was open
    /// (`max_blunders_over_the_battles_to_the_end`).
    idle: u32,
    /// Of those, a strike that kills a unit was open.
    idle_kill: u32,
}

impl Game {
    fn kept(&self, team: Team) -> i64 {
        if team == Team::Player { self.player_kept } else { self.enemy_kept }
    }

    fn won(&self, team: Team) -> bool {
        matches!((self.winner.as_str(), team), ("player", Team::Player) | ("enemy", Team::Enemy))
    }
}

/// One battle of the set: the original on both sides, and Max on each side in turn.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
struct Entry {
    id: String,
    original: Game,
    /// Max on the enemy's side.
    max_enemy: Game,
    /// Max on the player's side.
    max_player: Game,
}

impl Entry {
    fn max(&self, team: Team) -> &Game {
        if team == Team::Player { &self.max_player } else { &self.max_enemy }
    }
}

/// The committed baseline, one battle per line in the file.
#[derive(Serialize, Deserialize, Default)]
struct Baseline {
    battles: Vec<Entry>,
}

/// Plays `b` to its end. `max` (if any) plays by «Максимум» on that side; the original's AI
/// plays the rest, and all of it when `max` is `None` (then both sides' decisions are tallied).
fn play(mut b: Battle, max: Option<Team>) -> Game {
    b.begin();
    let tables = MaxTables::default();
    let mut g = Game::default();
    for _ in 0..AUTO_PLAY_STEPS {
        let Some(id) = b.active() else { break };
        let team = b.fighters[id].team;
        let from_reserve = b.fighters[id].slot.row == Row::Reserve;
        if Some(team) == max {
            let Some(r) = b.max_choice_with(BENCH_BUDGET, Some(&tables), None) else { break };
            tally(&mut g, &b, id, from_reserve, r.choice.plan());
            b.play_max_move(r.choice);
        } else {
            if max.is_none() {
                tally(&mut g, &b, id, from_reserve, b.plan(false));
            }
            b.auto_step();
        }
    }
    g.winner = match b.outcome() {
        Outcome::Victory => "player",
        Outcome::Defeat => "enemy",
        Outcome::Ongoing => "none",
    }
    .to_string();
    g.turn_limit = b.end_reason() == Some(EndReason::TurnLimit);
    g.turns = b.round;
    g.player_kept = (worth_left(&b, Team::Player) * 1000.0).round() as i64;
    g.enemy_kept = (worth_left(&b, Team::Enemy) * 1000.0).round() as i64;
    g
}

/// Tallies one decision of the active unit `id` whose plan is `plan` (`None`: the rules refuse
/// it, a skip), as `blunders_in` does for Max's.
fn tally(g: &mut Game, b: &Battle, id: usize, from_reserve: bool, plan: Option<Plan>) {
    g.decisions += 1;
    if let Some(Plan::Move(s)) = plan {
        g.reserve += u32::from(s.row == Row::Reserve && !from_reserve);
    }
    if matches!(plan, Some(Plan::Act(..))) {
        return;
    }
    let damage = |&(tg, k): &(usize, ActionKind)| k.is_hostile() && matches!(b.preview(id, tg, k), Preview::Damage(d) if d > 0);
    let open: Vec<(usize, ActionKind)> = b.all_options(id).into_iter().filter(damage).collect();
    if !open.is_empty() {
        g.idle += 1;
        let kill = open.iter().any(|&(tg, k)| matches!(b.preview(id, tg, k), Preview::Damage(d) if d >= b.fighters[tg].hp));
        g.idle_kill += u32::from(kill);
    }
}

/// The battle set: the demo's first `demo` battles, then the install's (`RAZDOR_DT_DIR`) every
/// `every`-th, named by their set and index. Install battles are fixed by the install.
fn battles(demo: usize, every: usize) -> Battles {
    let mut set = synthetic_battles(demo);
    if let Some(sets) = real_sets() {
        let mut k = 0;
        for (title, sets) in sets {
            for (name, b) in sets {
                if k % every == 0 {
                    set.push((format!("{title} {k}: {name}"), b));
                }
                k += 1;
            }
        }
    }
    set
}

/// Plays every battle of `set` the three ways.
fn run(set: Battles) -> Vec<Entry> {
    set.into_iter()
        .map(|(id, b)| Entry {
            id,
            original: play(b.clone(), None),
            max_enemy: play(b.clone(), Some(Team::Enemy)),
            max_player: play(b, Some(Team::Player)),
        })
        .collect()
}

/// Totals of one side of the comparison: Max's games, or the original's over the same games.
#[derive(Default)]
struct Side {
    games: u32,
    wins: u32,
    turn_limit: u32,
    /// Worth kept by the side's own units, and taken from the other side, in permille.
    kept: i64,
    taken: i64,
    decisions: u32,
    reserve: u32,
    idle: u32,
    idle_kill: u32,
}

impl Side {
    fn game(&mut self, g: &Game, team: Team) {
        self.games += 1;
        self.wins += u32::from(g.won(team));
        self.turn_limit += u32::from(g.turn_limit);
        self.kept += g.kept(team);
        self.taken += 1000 - g.kept(team.other());
    }

    fn counts(&mut self, g: &Game) {
        self.decisions += g.decisions;
        self.reserve += g.reserve;
        self.idle += g.idle;
        self.idle_kill += g.idle_kill;
    }

    fn print(&self, title: &str) {
        let n = self.games.max(1) as f64;
        let per_k = |x: u32| 1000.0 * x as f64 / self.decisions.max(1) as f64;
        println!(
            "{title}: wins {} of {} games ({:.1}%), turn limit {}; worth kept {:.0}‰, taken {:.0}‰; {} decisions: \
             reserve steps {} ({:.2}‰), idle with a damaging strike {} ({:.1}‰), of which a kill {} ({:.2}‰)",
            self.wins,
            self.games,
            100.0 * self.wins as f64 / n,
            self.turn_limit,
            self.kept as f64 / n,
            self.taken as f64 / n,
            self.decisions,
            self.reserve,
            per_k(self.reserve),
            self.idle,
            per_k(self.idle),
            self.idle_kill,
            per_k(self.idle_kill)
        );
    }
}

/// Max's totals and the original's over the same games (the original's decisions and blunders
/// counted for both sides, once per battle).
fn summarize(entries: &[Entry]) -> (Side, Side) {
    let (mut max, mut base) = (Side::default(), Side::default());
    for e in entries {
        for team in Team::BOTH {
            max.game(e.max(team), team);
            max.counts(e.max(team));
            base.game(&e.original, team);
        }
        base.counts(&e.original);
    }
    (max, base)
}

/// The fields of a battle's record that differ from the baseline, as `path old -> new`.
fn diff(old: &Entry, new: &Entry) -> Vec<String> {
    fn leaves(prefix: &str, v: &Value, out: &mut BTreeMap<String, Value>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    leaves(&format!("{prefix}{k}."), x, out);
                }
            }
            _ => {
                out.insert(prefix.trim_end_matches('.').to_string(), v.clone());
            }
        }
    }
    let (mut a, mut b) = (BTreeMap::new(), BTreeMap::new());
    leaves("", &serde_json::to_value(old).unwrap(), &mut a);
    leaves("", &serde_json::to_value(new).unwrap(), &mut b);
    a.remove("id");
    b.remove("id");
    let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| a.get(*k) != b.get(*k))
        .map(|k| format!("{k} {} -> {}", a.get(k).map_or("-".into(), Value::to_string), b.get(k).map_or("-".into(), Value::to_string)))
        .collect()
}

fn load_baseline() -> Option<Baseline> {
    let text = std::fs::read_to_string(baseline_path()).ok()?;
    Some(serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", baseline_path().display())))
}

/// The baseline file: one battle per line, so a change shows as the battles it touches.
fn write_baseline(battles: &[Entry]) {
    let mut out = String::from("{\n\"battles\": [\n");
    let lines: Vec<String> = battles.iter().map(|e| serde_json::to_string(e).unwrap()).collect();
    out.push_str(&lines.join(",\n"));
    out.push_str("\n]\n}\n");
    std::fs::write(baseline_path(), out).unwrap();
}

/// The full benchmark: prints the summary and the per-battle diff against the baseline. With
/// `RAZDOR_MAX_BENCH_WRITE=1` it writes the baseline: the battles it ran replace the baseline's
/// records of the same ids, and the others stay. Slow (about every battle three times):
/// run by hand with `--ignored`.
#[test]
#[ignore]
fn max_bench() {
    let entries = run(battles(DEMO_BATTLES, BENCH_EVERY));
    let (max, base) = summarize(&entries);
    println!("Max against the original's AI, {} battles:", entries.len());
    max.print("  Max");
    base.print("  original vs original");
    let old = load_baseline().unwrap_or_default();
    let mut changed = 0;
    for e in &entries {
        let Some(o) = old.battles.iter().find(|o| o.id == e.id) else {
            println!("  {}: not in the baseline", e.id);
            continue;
        };
        let d = diff(o, e);
        if !d.is_empty() {
            changed += 1;
            println!("  {}:\n    {}", e.id, d.join("\n    "));
        }
    }
    println!("{changed} of {} battles differ from the baseline ({} in it)", entries.len(), old.battles.len());
    if std::env::var_os("RAZDOR_MAX_BENCH_WRITE").is_some() {
        let mut merged = entries.clone();
        merged.extend(old.battles.into_iter().filter(|o| !entries.iter().any(|e| e.id == o.id)));
        write_baseline(&merged);
        println!("baseline written: {} battles", merged.len());
    }
}

/// The fast check of «Максимум» against the committed baseline (every change to it runs
/// this): on the demo's battles, and on every [`FAST_EVERY`]-th install battle with
/// `RAZDOR_DT_DIR`. Fails when Max wins fewer of the games than the baseline did, keeps less
/// worth on average (more than [`KEPT_TOLERANCE`] permille), or any of its games has more
/// reserve steps or blunders than the baseline's. A battle the baseline lacks fails too: an
/// intended change rewrites the baseline with `max_bench` (`RAZDOR_MAX_BENCH_WRITE=1`).
#[test]
fn max_bench_does_not_regress() {
    if cfg!(debug_assertions) {
        return;
    }
    let baseline = load_baseline().expect("the baseline src/rules/battle/max_ai/bench_baseline.json is missing");
    let entries = run(battles(DEMO_BATTLES, FAST_EVERY));
    let mut problems = Vec::new();
    let (mut wins, mut old_wins, mut kept, mut old_kept, mut games) = (0i64, 0i64, 0i64, 0i64, 0i64);
    for e in &entries {
        let Some(o) = baseline.battles.iter().find(|o| o.id == e.id) else {
            problems.push(format!("{}: not in the baseline", e.id));
            continue;
        };
        for team in Team::BOTH {
            let (now, then) = (e.max(team), o.max(team));
            games += 1;
            wins += i64::from(now.won(team));
            old_wins += i64::from(then.won(team));
            kept += now.kept(team);
            old_kept += then.kept(team);
            for (what, n, t) in [("reserve steps", now.reserve, then.reserve), ("idle with a damaging strike", now.idle, then.idle), ("idle with a kill open", now.idle_kill, then.idle_kill)] {
                if n > t {
                    problems.push(format!("{} as {team:?}: {what} {t} -> {n}", e.id));
                }
            }
        }
    }
    if wins < old_wins {
        problems.push(format!("Max wins {wins} of {games} games, the baseline {old_wins}"));
    }
    if kept < old_kept - KEPT_TOLERANCE * games {
        problems.push(format!("Max keeps {:.1}‰ of its worth on average, the baseline {:.1}‰", kept as f64 / games.max(1) as f64, old_kept as f64 / games.max(1) as f64));
    }
    assert!(problems.is_empty(), "«Максимум» regressed against the baseline (judge it with `max_bench`):\n{}", problems.join("\n"));
}
