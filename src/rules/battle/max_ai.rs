//! «Максимум», Razdor's strongest battle AI (the owner's request; a deviation from the
//! original, main only, never on `dt-original`, off by default). The original's AI
//! ([`Battle::plan`]) stays as it is and keeps playing everywhere else: both sides of
//! off-screen battles, the pre-simulation, the world AI's simulated battles and the rollouts
//! here. With Max chosen, the hero's auto-battle (Q, W) is played by Max on both sides
//! ([`Battle::max_play_to_end`]; before the 2026-10-18 reports it fell back to the
//! original's AI).
//!
//! A decision is a minimax search with alpha-beta pruning over the battle's real actions:
//! every legal action of the active unit (each target with its cell's action, each step,
//! a pass) is played on a clone by the game's own rules, and so on for whoever acts next
//! (a node maximises when a unit of the searching side acts, minimises when the other
//! side's does; the order is the battle's own initiative queue, several actions of a unit
//! in a row included). Battles have no randomness, so each line is exact. A leaf is scored
//! by a rollout: the rest of the battle played by the original's AI on both sides, then a
//! win or loss term that outweighs everything plus each side's remaining worth (tactical
//! value × the share of its HP left) of the searching side minus the other's. Leaves deep
//! in the tree are scored statically (the same worth, no rollout).
//!
//! Iterative deepening runs until the time (or, in tests, the node) budget is spent; the
//! deepest completed iteration decides. The original's own pick is searched first at every
//! node (the move ordering for the pruning, then the per-target damage) and at the root it
//! keeps the choice unless another action scores strictly higher, so Max never picks what
//! its own evaluation rates below the original's choice, and ties stay predictable.
//!
//! The battle's [`MaxTables`] (a transposition table and a rollout cache, `tables.rs`) only
//! save work: the choice of a search to a given depth is the same with them or without, and
//! with whatever they hold. [`MaxThinker`] (`ponder.rs`) runs the searches on a worker
//! thread ahead of the need: the next decision as soon as it is known, or the one after the
//! player's predicted reply.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::*;

mod ponder;
mod tables;

pub use ponder::{MaxPoll, MaxThinker};
pub use tables::MaxTables;
use tables::{Bound, StateKey};

/// The time one decision may take on screen (the search returns the best found by then).
pub const MAX_AI_BUDGET: Duration = Duration::from_millis(150);
/// A decision of the quick battle with «Максимум» (Q, [`Battle::max_play_to_end`]): a node
/// budget, no clock, so the same battle ends the same way on any machine; played on a worker
/// while the battle window waits. 4 actions ahead and 600 positions (the harness's budget):
/// on the install's armies about 0.8 s a battle on one E-core, and 2000 positions won no
/// more (`max_plays_the_hero_s_side_of_the_quick_battle_better`: 43% against 42%, 2.7 times
/// as long).
pub const MAX_QUICK_BUDGET: MaxBudget = MaxBudget::fixed(4, 600);
/// Iterative deepening stops at this many actions ahead.
const MAX_DEPTH: u32 = 24;
/// Leaves more than this many actions ahead are scored without a rollout.
const ROLLOUT_PLIES: u32 = 8;
/// A battle won or lost outweighs any strength left.
const WIN: i64 = 1 << 40;
const INF: i64 = i64::MAX / 4;
/// A unit's worth is its tactical value × this × its share of HP left.
const WORTH_SCALE: i64 = 1000;
/// Rollout steps between two looks at the clock.
const CLOCK_EVERY: usize = 8;
/// A rollout's start and its first positions, this many in all, go in the cache.
const ROLLOUT_KEYS: usize = 4;
/// A cut rollout (`MaxBudget::short_rollouts`): from this many rounds in, it stops as soon
/// as one side holds at least `SHORT_SHARE`‰ more of the worth left than the other (of the
/// sum), with more than `SHORT_MARGIN` rounds to the turn limit, and is scored statically
/// (`Battle::max_static`). Fitted by `fit_the_static_evaluation_of_cut_rollouts` on the
/// harness's positions: one round in and 500‰, the side ahead won every such rollout.
const SHORT_ROUNDS: u32 = 1;
const SHORT_SHARE: i64 = 500;
const SHORT_MARGIN: i64 = 4;
/// The cut rollouts' scores are cached apart from the full ones'.
const SHORT_SALT: StateKey = 0x5348_4f52_5420_524f_4c4c_4f55_5420_2020;

/// How long a decision may search: a time limit (the game's), or none and a node limit
/// (deterministic, for tests); both stop at `max_depth` actions ahead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaxBudget {
    pub time: Option<Duration>,
    pub max_depth: u32,
    pub max_nodes: u64,
    /// Rollouts of the leaves past the root's children are cut short where the static
    /// evaluation is sure ([`SHORT_ROUNDS`]). Off in the game: measured, it gave about a
    /// fifth more positions a second but no greater depth and a little less worth left
    /// (the rollouts are short already, ~27 actions, and only ~7% of their actions are saved
    /// where the fit is always right).
    pub short_rollouts: bool,
    /// The root's actions are searched on [`max_threads`] threads (the game's; a node limit
    /// needs one thread to be met exactly).
    pub parallel: bool,
}

impl MaxBudget {
    /// The battle on screen: [`MAX_AI_BUDGET`] per decision.
    pub const GAME: MaxBudget = MaxBudget { time: Some(MAX_AI_BUDGET), max_depth: MAX_DEPTH, max_nodes: u64::MAX, short_rollouts: false, parallel: true };

    /// No clock: at most `max_depth` actions ahead and `max_nodes` positions, the same
    /// choice on any machine.
    pub const fn fixed(max_depth: u32, max_nodes: u64) -> MaxBudget {
        MaxBudget { time: None, max_depth, max_nodes, short_rollouts: false, parallel: false }
    }
}

/// The threads a decision searches on: the owner's limit of 4, and one core left free;
/// at least 1.
pub fn max_threads() -> usize {
    static THREADS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *THREADS.get_or_init(|| std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1)).clamp(1, 4))
}

/// One decision for the active unit: an action, or (`None`) the original AI's skip of the
/// unit's turn when the rules refuse its plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaxMove(Option<Plan>);

/// What a decision found.
#[derive(Clone, Copy, Debug)]
pub struct MaxReport {
    pub choice: MaxMove,
    /// The original AI's choice in the same position.
    pub original: MaxMove,
    /// The deepest completed iteration's scores of the choice and of the original's (0 and
    /// 0 when not even depth 1 completed: the original's choice is kept).
    pub value: i64,
    pub original_value: i64,
    /// Actions ahead of the deepest completed iteration.
    pub depth: u32,
    /// The search was over before its time: a single action, every line ended in the
    /// battle's end, or one action left after depth 1.
    pub settled: bool,
    pub nodes: u64,
    /// Rollouts played, positions settled by the table, rollouts found in the cache.
    pub rollouts: u64,
    pub tt_hits: u64,
    pub rollout_hits: u64,
    pub elapsed: Duration,
}

impl Battle {
    /// The enemy of the battle on screen plays by «Максимум» (Razdor's, [`Battle::max_choice`]);
    /// the interface reads it, the rules never do.
    pub fn set_max_ai(&mut self, on: bool) {
        self.max_ai = on;
    }

    pub fn max_ai(&self) -> bool {
        self.max_ai
    }

    /// Every action the active unit `id` may take: its cells' actions, its steps, a pass.
    fn max_moves(&self, id: usize) -> Vec<Plan> {
        let mut v: Vec<Plan> = self.all_options(id).into_iter().map(|(t, k)| Plan::Act(t, k)).collect();
        v.extend(self.moves(id).into_iter().map(Plan::Move));
        v.push(Plan::Pass);
        v
    }

    /// The original AI's decision as the auto-battle carries it out: its plan, or a skip
    /// when the rules would refuse that plan.
    fn original_move(&self, legal: &[Plan]) -> MaxMove {
        MaxMove(self.plan(false).filter(|p| legal.contains(p)))
    }

    /// The order to search `id`'s actions in: the original's first, then the most damage
    /// (in tactical worth, a kill counting its whole worth again), heals, the rest of the
    /// cells' actions, steps, a pass.
    fn max_order(&self, id: usize) -> Vec<MaxMove> {
        let legal = self.max_moves(id);
        let original = self.original_move(&legal);
        let key = |p: &Plan| -> i64 {
            match *p {
                Plan::Act(t, k) => {
                    let tf = &self.fighters[t];
                    let worth = (tf.tactical.max(1) as i64) * WORTH_SCALE / tf.max_hp().max(1) as i64;
                    match self.preview(id, t, k) {
                        Preview::Damage(d) => 2 + d.max(0) as i64 * worth + if d >= tf.hp { tf.tactical.max(1) as i64 * WORTH_SCALE } else { 0 },
                        Preview::Heal(h) => 2 + h.max(0) as i64 * worth,
                        Preview::Buff(_) => 2,
                    }
                }
                Plan::Move(_) => 1,
                Plan::Pass => 0,
            }
        };
        let mut rest: Vec<Plan> = legal.iter().copied().filter(|&p| MaxMove(Some(p)) != original).collect();
        rest.sort_by_key(|p| std::cmp::Reverse(key(p)));
        std::iter::once(original).chain(rest.into_iter().map(|p| MaxMove(Some(p)))).collect()
    }

    /// Carries out a decision for the active unit; a refused action ends its turn, as the
    /// auto-battle's (`auto_step`).
    pub fn play_max_move(&mut self, m: MaxMove) -> Option<Step> {
        let actor = self.active()?;
        let step = m.0.and_then(|p| self.step_plan(p));
        step.or_else(|| {
            self.skip();
            Some(Step::Wait { actor })
        })
    }

    /// One action of the active unit by «Максимум».
    pub fn max_step(&mut self, budget: MaxBudget) -> Option<Step> {
        let report = self.max_choice(budget)?;
        self.play_max_move(report.choice)
    }

    /// The hero's auto-battle with «Максимум» chosen (Q; the owner's and the players'
    /// reports, 2026-10-18): the rest of the battle played at once by Max on both sides, each
    /// side with its own tables, so the enemy plays as it does against the player's own
    /// moves and the player's units are played by the same search. Deploys as the cards
    /// stand when still deploying; a position Max cannot search (nobody to act) leaves the
    /// step to the hero's auto-battle. With a fixed budget ([`MAX_QUICK_BUDGET`]) the same
    /// position always ends the same way. Returns the outcome.
    pub fn max_play_to_end(&mut self, budget: MaxBudget) -> Outcome {
        self.begin();
        let tables = [MaxTables::default(), MaxTables::default()];
        for _ in 0..AUTO_PLAY_STEPS {
            if self.outcome() != Outcome::Ongoing {
                break;
            }
            let Some(id) = self.active() else { break };
            let side = self.fighters[id].team;
            match self.max_choice_with(budget, Some(&tables[side.index()]), None) {
                Some(r) => self.play_max_move(r.choice),
                None => self.hero_auto_step(),
            };
        }
        self.outcome()
    }

    /// `side`'s score of the battle as it stands: a win or loss, then the worth left of each
    /// side, `side`'s minus the other's.
    fn max_eval(&self, side: Team) -> i64 {
        let won = match self.outcome() {
            Outcome::Ongoing => 0,
            Outcome::Victory if side == Team::Player => WIN,
            Outcome::Defeat if side == Team::Enemy => WIN,
            _ => -WIN,
        };
        won + self.max_worth(side) - self.max_worth(side.other())
    }

    /// `side`'s static score of a rollout cut short, when it is sure who wins: the win or
    /// loss and the winner's worth left by Lanchester's square law, √(W² − L²/4) (its fit
    /// beat W − L and the other weights of L²).
    fn max_static(&self, side: Team) -> Option<i64> {
        let (own, other) = (self.max_worth(side), self.max_worth(side.other()));
        if self.turn_limit() as i64 - (self.round as i64) <= SHORT_MARGIN || (own - other).abs() * 1000 < SHORT_SHARE * (own + other).max(1) {
            return None;
        }
        let (w, l) = (own.max(other) as f64, own.min(other) as f64);
        let left = (w * w - l * l / 4.0).max(0.0).sqrt() as i64;
        Some(if own > other { WIN + left } else { -WIN - left })
    }

    /// `team`'s worth left: each standing unit's tactical value × its share of HP left, plus
    /// its HP.
    fn max_worth(&self, team: Team) -> i64 {
        self.fighters
            .iter()
            .filter(|f| f.team == team && f.alive() && !f.surrendered)
            .map(|f| f.tactical.max(1) as i64 * WORTH_SCALE * f.hp as i64 / f.max_hp().max(1) as i64 + f.hp as i64)
            .sum()
    }

    /// The search for the active unit's best action (the module's comment), with no tables
    /// and no clock but the budget's. The battle is left as it is, the Community patch's
    /// globals too. `None` when nobody is to act.
    pub fn max_choice(&self, budget: MaxBudget) -> Option<MaxReport> {
        self.max_choice_with(budget, None, None)
    }

    /// [`Battle::max_choice`] with the battle's transposition table and rollout cache
    /// (`tables`, which only save work: the choice at each depth is the same with them or
    /// without) and, for a search on a worker, its `control` (a stop and a deadline set from
    /// outside, in place of the budget's time).
    pub fn max_choice_with(&self, budget: MaxBudget, tables: Option<&MaxTables>, control: Option<&MaxControl>) -> Option<MaxReport> {
        let id = self.active()?;
        let t0 = Instant::now();
        let saved = patch_globals();
        let silent = crate::i18n::set_silent(true);
        let mut root = self.clone();
        root.log.clear();
        root.stale_reads.borrow_mut().clear();
        let mut moves = root.max_order(id);
        let original = moves[0];
        let mut s = Search {
            side: root.fighters[id].team,
            deadline: if control.is_some() { None } else { budget.time.map(|t| t0 + t) },
            control,
            tables,
            max_nodes: budget.max_nodes,
            nodes: 0,
            rollouts: 0,
            tt_hits: 0,
            rollout_hits: 0,
            aborted: false,
            cut: false,
            short: budget.short_rollouts,
            scratch: Vec::new(),
            roll: None,
        };
        let mut done = None;
        // A single action (a pass): nothing to weigh.
        let mut settled = moves.len() == 1;
        for depth in 1..=budget.max_depth.max(1) {
            if settled {
                break;
            }
            s.cut = false;
            let threads = if budget.parallel && budget.max_nodes == u64::MAX { max_threads().min(moves.len()) } else { 1 };
            let found = if threads > 1 { s.root_parallel(&root, &moves, depth, threads, saved) } else { s.root(&root, &moves, depth) };
            let Some((best, value, original_value, values)) = found else { break };
            done = Some((best, value, original_value, depth));
            if depth == 1 {
                // Only the actions whose rollout does at least as well as the original's
                // choice's stay in the deeper search: against the original's AI, whose next
                // moves the rollout plays, every decision then keeps at least the score the
                // last one expected, so Max ends no worse than the original would have.
                moves = moves.iter().zip(&values).filter(|&(_, &v)| v >= original_value).map(|(&m, _)| m).collect();
            }
            // The previous best next after the original's.
            if let Some(i) = moves.iter().skip(1).position(|&m| m == best) {
                let m = moves.remove(i + 1);
                moves.insert(1, m);
            }
            // Decided: every line ended within the depth (the battle's end is proven, a
            // deeper search finds the same), or the original's choice is the only one left.
            settled = !s.cut || moves.len() == 1;
        }
        set_patch_globals(saved);
        crate::i18n::set_silent(silent);
        let (choice, value, original_value, depth) = done.unwrap_or((original, 0, 0, 0));
        Some(MaxReport {
            choice,
            original,
            value,
            original_value,
            depth,
            settled,
            nodes: s.nodes,
            rollouts: s.rollouts,
            tt_hits: s.tt_hits,
            rollout_hits: s.rollout_hits,
            elapsed: t0.elapsed(),
        })
    }
}

/// A search on a worker thread steered from outside: stopped (its answer no longer wanted),
/// or given a deadline once the battle needs the move.
pub struct MaxControl {
    start: Instant,
    stop: AtomicBool,
    /// Nanoseconds from `start`.
    deadline: AtomicU64,
}

impl MaxControl {
    /// A search that may run `limit` at most unless told otherwise.
    pub fn new(limit: Duration) -> MaxControl {
        MaxControl { start: Instant::now(), stop: AtomicBool::new(false), deadline: AtomicU64::new(limit.as_nanos() as u64) }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// The battle needs the move now: the search ends `budget` after it began, or at once
    /// if it has run that long already (the deepest completed depth decides).
    pub fn need(&self, budget: Duration) {
        let now = self.start.elapsed().as_nanos() as u64;
        self.deadline.fetch_min(now.max(budget.as_nanos() as u64), Ordering::Relaxed);
    }

    fn expired(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || self.start.elapsed().as_nanos() as u64 >= self.deadline.load(Ordering::Relaxed)
    }
}

/// One decision's search state.
struct Search<'a> {
    side: Team,
    deadline: Option<Instant>,
    control: Option<&'a MaxControl>,
    tables: Option<&'a MaxTables>,
    max_nodes: u64,
    nodes: u64,
    rollouts: u64,
    tt_hits: u64,
    rollout_hits: u64,
    /// The budget ran out: the iteration under way is dropped.
    aborted: bool,
    /// A line was cut by the depth while the battle went on.
    cut: bool,
    short: bool,
    /// The positions under way by distance from the root, and the rollout's: their buffers
    /// are reused (`Battle::clone_from`), the battle searched is never changed.
    scratch: Vec<Option<Battle>>,
    roll: Option<Battle>,
}

/// `b` copied into the buffer of an old position, if any.
fn copy_into(buf: Option<Battle>, b: &Battle) -> Battle {
    match buf {
        Some(mut c) => {
            c.clone_from(b);
            c
        }
        None => b.clone(),
    }
}

/// The leaves' class of a subtree, for the transposition table: a position's score depends
/// on how far from the root its subtree's leaves are (past [`ROLLOUT_PLIES`] they get no
/// rollout), so it is its distance from the root, or one class for all subtrees whose leaves
/// are all rolled out.
fn class(ply: u32, depth: u32) -> u8 {
    if ply + depth <= ROLLOUT_PLIES { u8::MAX } else { ply.min(ROLLOUT_PLIES + 1) as u8 }
}

impl<'a> Search<'a> {
    fn time_up(&self) -> bool {
        self.deadline.is_some_and(|d| Instant::now() >= d) || self.control.is_some_and(MaxControl::expired)
    }

    fn out_of_time(&mut self) -> bool {
        if !self.aborted && (self.nodes >= self.max_nodes || self.time_up()) {
            self.aborted = true;
        }
        self.aborted
    }

    /// The root of one iteration: the original's move first and exactly; another move
    /// replaces it only by scoring strictly higher. Gives the best move, its score, the
    /// original's and every move's score (exact at depth 1, where nothing is pruned). `None`
    /// if the budget ran out. The root's order is the caller's alone, never the table's, so
    /// which of equal moves wins does not depend on what the tables hold.
    fn root(&mut self, b: &Battle, moves: &[MaxMove], depth: u32) -> Option<(MaxMove, i64, i64, Vec<i64>)> {
        let (mut best, mut alpha, mut original_value) = (moves[0], -INF, 0);
        let mut values = Vec::with_capacity(moves.len());
        for (i, &m) in moves.iter().enumerate() {
            let v = self.child(b, m, depth - 1, 1, alpha, INF);
            if self.aborted {
                return None;
            }
            values.push(v);
            if i == 0 {
                (best, alpha, original_value) = (m, v, v);
            } else if v > alpha {
                (best, alpha) = (m, v);
            }
        }
        Some((best, alpha, original_value, values))
    }

    /// A search like this one for another thread, its counters at 0.
    fn fork(&self) -> Search<'a> {
        Search {
            side: self.side,
            deadline: self.deadline,
            control: self.control,
            tables: self.tables,
            max_nodes: self.max_nodes,
            nodes: 0,
            rollouts: 0,
            tt_hits: 0,
            rollout_hits: 0,
            aborted: false,
            cut: false,
            short: self.short,
            scratch: Vec::new(),
            roll: None,
        }
    }

    /// [`Search::root`] on `threads` threads, which take the root's moves in turn and share
    /// the tables; the same answer whatever the threads' timing. The best so far is the
    /// highest exact score with the lowest index; move `i` is searched with alpha at the
    /// best's score when the best is before it (a tie keeps the earlier move) and one below
    /// when after it (a tie takes `i`), so a score above alpha is exact and one at or below it
    /// cannot win. The result is the first move with the highest score, as the sequential
    /// search finds it; move 0 (the original's choice) is searched with no bound, exactly.
    /// `None` if a thread ran out of time (the iteration is dropped).
    fn root_parallel(&mut self, b: &Battle, moves: &[MaxMove], depth: u32, threads: usize, globals: PatchGlobals) -> Option<(MaxMove, i64, i64, Vec<i64>)> {
        use std::sync::atomic::AtomicUsize;
        use std::sync::Mutex;
        let next = AtomicUsize::new(0);
        let best = Mutex::new((-INF, usize::MAX));
        let values = Mutex::new(vec![None; moves.len()]);
        let forks: Vec<Search> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|_| {
                    let (mut s, b) = (self.fork(), b.clone());
                    let (next, best, values) = (&next, &best, &values);
                    scope.spawn(move || {
                        set_patch_globals(globals);
                        crate::i18n::set_silent(true);
                        loop {
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            if i >= moves.len() {
                                break;
                            }
                            let alpha = match *best.lock().unwrap_or_else(|e| e.into_inner()) {
                                _ if i == 0 => -INF,
                                (v, at) if at < i => v,
                                (v, _) => v - 1,
                            };
                            let v = s.child(&b, moves[i], depth - 1, 1, alpha, INF);
                            if s.aborted {
                                break;
                            }
                            values.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(v);
                            if i == 0 || v > alpha {
                                let mut g = best.lock().unwrap_or_else(|e| e.into_inner());
                                if v > g.0 || (v == g.0 && i < g.1) {
                                    *g = (v, i);
                                }
                            }
                        }
                        s
                    })
                })
                .collect();
            workers.into_iter().filter_map(|w| w.join().ok()).collect()
        });
        let all = forks.len() == threads;
        for f in forks {
            self.nodes += f.nodes;
            self.rollouts += f.rollouts;
            self.tt_hits += f.tt_hits;
            self.rollout_hits += f.rollout_hits;
            self.aborted |= f.aborted;
            self.cut |= f.cut;
        }
        let values: Option<Vec<i64>> = values.into_inner().unwrap_or_else(|e| e.into_inner()).into_iter().collect();
        let (v, at) = best.into_inner().unwrap_or_else(|e| e.into_inner());
        match values {
            Some(values) if all && !self.aborted && at < moves.len() => Some((moves[at], v, values[0], values)),
            _ => {
                self.aborted = true;
                None
            }
        }
    }

    /// The score of `m` played in `b`; the patch's globals are put back after it.
    fn child(&mut self, b: &Battle, m: MaxMove, depth: u32, ply: u32, alpha: i64, beta: i64) -> i64 {
        let globals = patch_globals();
        let mut c = copy_into(self.scratch.get_mut(ply as usize).and_then(Option::take), b);
        c.play_max_move(m);
        let v = self.value(&c, depth, ply, alpha, beta);
        if self.scratch.len() <= ply as usize {
            self.scratch.resize_with(ply as usize + 1, || None);
        }
        self.scratch[ply as usize] = Some(c);
        set_patch_globals(globals);
        v
    }

    /// Alpha-beta, fail-soft: the exact score when it lies strictly between `alpha` and
    /// `beta`, else a bound past the one it crossed. The table's bounds of the same depth and
    /// leaf class (or of an uncut search) settle a position when they meet or cross the
    /// window, and its best move is tried first; neither changes a score inside the window,
    /// so the root's choice is the same with the table or without.
    fn value(&mut self, b: &Battle, depth: u32, ply: u32, mut alpha: i64, mut beta: i64) -> i64 {
        self.nodes += 1;
        if self.out_of_time() {
            return 0;
        }
        let Some(id) = b.active() else { return self.leaf(b, ply) };
        if depth == 0 {
            self.cut = true;
            return self.leaf(b, ply);
        }
        let cls = class(ply, depth);
        let d8 = depth.min(u8::MAX as u32) as u8;
        let key = self.tables.map(|_| b.max_key(self.side));
        let mut first = None;
        if let (Some(t), Some(k)) = (self.tables, key) {
            if let Some(e) = t.tt_get(k) {
                if let Some(bd) = e.bound(d8, cls) {
                    let v = if bd.lower >= beta || bd.lower == bd.upper {
                        Some(bd.lower)
                    } else if bd.upper <= alpha {
                        Some(bd.upper)
                    } else {
                        None
                    };
                    if let Some(v) = v {
                        self.tt_hits += 1;
                        self.cut |= bd.cut;
                        return v;
                    }
                }
                first = e.best;
            }
        }
        let (alpha0, beta0) = (alpha, beta);
        let outer_cut = std::mem::take(&mut self.cut);
        let maxing = b.fighters[id].team == self.side;
        let mut best = if maxing { -INF } else { INF };
        let mut best_move = None;
        let mut order = b.max_order(id);
        if let Some(i) = first.and_then(|f| order.iter().position(|&m| m == f)) {
            let m = order.remove(i);
            order.insert(0, m);
        }
        for m in order {
            let v = self.child(b, m, depth - 1, ply + 1, alpha, beta);
            if self.aborted {
                return 0;
            }
            if (maxing && v > best) || (!maxing && v < best) {
                best = v;
                best_move = Some(m);
            }
            if maxing {
                alpha = alpha.max(v);
            } else {
                beta = beta.min(v);
            }
            if alpha >= beta {
                break;
            }
        }
        let cut = self.cut;
        self.cut |= outer_cut;
        if let (Some(t), Some(k)) = (self.tables, key) {
            let (lower, upper) = if best <= alpha0 {
                (-INF, best)
            } else if best >= beta0 {
                (best, INF)
            } else {
                (best, best)
            };
            t.tt_add(k, Bound { depth: d8, class: cls, cut, lower, upper }, best_move);
        }
        best
    }

    /// A leaf: the rest of the battle by the original's AI on both sides, then scored; deep
    /// in the tree, or with the battle over, scored as it stands. Rollouts are deterministic,
    /// so the cache keeps each one's score under the key of its start and of its first few
    /// positions (another rollout passing through one of them ends the same).
    fn leaf(&mut self, b: &Battle, ply: u32) -> i64 {
        if b.outcome() != Outcome::Ongoing || ply > ROLLOUT_PLIES {
            return b.max_eval(self.side);
        }
        // The root's children keep full rollouts: the first iteration's safety rule weighs
        // each action by the original's own play to the end.
        let short = self.short && ply >= 2;
        let salt = if short { SHORT_SALT } else { 0 };
        let mut chain: [StateKey; ROLLOUT_KEYS] = [0; ROLLOUT_KEYS];
        let mut keyed = 0;
        if let Some(t) = self.tables {
            let k = b.max_key(self.side) ^ salt;
            if let Some(v) = t.rollout_get(k) {
                self.rollout_hits += 1;
                return v;
            }
            chain[0] = k;
            keyed = 1;
        }
        self.rollouts += 1;
        let mut r = copy_into(self.roll.take(), b);
        let v = self.roll_out(&mut r, &mut chain, &mut keyed, short);
        self.roll = Some(r);
        let Some(v) = v else { return 0 };
        if let Some(t) = self.tables {
            for &k in &chain[..keyed] {
                t.rollout_add(k, v);
            }
        }
        v
    }

    /// Plays the rollout `r` to its end (`None` if the time ran out), looking up the cache
    /// for its first positions and noting their keys in `chain`.
    fn roll_out(&mut self, r: &mut Battle, chain: &mut [StateKey; ROLLOUT_KEYS], keyed: &mut usize, short: bool) -> Option<i64> {
        let mut known = None;
        let r0 = r.round;
        let salt = if short { SHORT_SALT } else { 0 };
        for i in 0..AUTO_PLAY_STEPS {
            if r.outcome() != Outcome::Ongoing {
                break;
            }
            if i % CLOCK_EVERY == 0 && self.time_up() {
                self.aborted = true;
                return None;
            }
            r.auto_step();
            if short && r.round >= r0 + SHORT_ROUNDS && r.outcome() == Outcome::Ongoing {
                if let Some(v) = r.max_static(self.side) {
                    known = Some(v);
                    break;
                }
            }
            if let Some(t) = self.tables.filter(|_| *keyed == i + 1 && *keyed < ROLLOUT_KEYS) {
                let k = r.max_key(self.side) ^ salt;
                if let Some(v) = t.rollout_get(k) {
                    known = Some(v);
                    break;
                }
                chain[*keyed] = k;
                *keyed += 1;
            }
        }
        Some(known.unwrap_or_else(|| r.max_eval(self.side)))
    }
}

#[cfg(test)]
mod tests;
