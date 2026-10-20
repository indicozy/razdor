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
//! its own evaluation rates below the original's choice, and ties stay predictable. Within a
//! tenth of a unit's worth of the best (and not below the original's), the root plays the move
//! that takes the most worth at once ([`Battle::tie_pick`]; the tempo experiment, the flow doc).
//!
//! The battle's [`MaxTables`] (a transposition table and a rollout cache, `tables.rs`) only
//! save work: the choice of a search to a given depth is the same with them or without, and
//! with whatever they hold. [`MaxThinker`] (`ponder.rs`) runs the searches on a worker
//! thread ahead of the need: the next decision as soon as it is known, or the one after the
//! player's predicted reply.
//!
//! What the battle is fought for changes how Max weighs a few positions ([`MaxStakes`],
//! Razdor's rule, the owner's decision of 2026-10-19): in a campaign Max never gives up the
//! original's choice for one its search scores lost on every line, and only a quick fight
//! lets it sacrifice its units for a win it has proven.

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
/// as long). Big armies that fight long took far more (#18: 10 s and more on a player's
/// 7800X3D, 23 fighters for 24 rounds 19 s on an E-core), nearly all of it in rollouts: they
/// are played ahead on the other cores ([`MaxBudget::prefetch`]), the same choices.
pub const MAX_QUICK_BUDGET: MaxBudget = MaxBudget { prefetch: true, ..MaxBudget::fixed(4, 600) };
/// Iterative deepening stops at this many actions ahead.
const MAX_DEPTH: u32 = 24;
/// Leaves more than this many actions ahead are scored without a rollout.
const ROLLOUT_PLIES: u32 = 8;
/// A battle won or lost outweighs any strength left.
const WIN: i64 = 1 << 40;
const INF: i64 = i64::MAX / 4;
/// A unit's worth is its tactical value × this × its share of HP left.
const WORTH_SCALE: i64 = 1000;
/// The root's tie-break toward damage ([`Battle::tie_pick`]): the moves within this many
/// permille of the smallest unit's worth of the best score are tied, and the one that deals
/// the most worth at once is played. 100‰ (a tenth of a unit's worth; 10‰ and 30% of a unit's
/// worth were measured too, the first left Max idle more, the last gave up more worth).
const TIE_PERMILLE: i64 = 100;
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
/// Tells the table entries of the player model's searches from the original's ([`MaxBudget::key_salt`]).
const MODEL_SALT: StateKey = 0x4d4f_4445_4c20_4f50_504f_4e45_4e54_2020;
/// A quick fight's proof of a win ([`Search::forced_win`], [`MaxStakes::Quick`]): at most
/// this many actions ahead and this many positions, or `FORCED_SHARE` times a node budget's
/// (no rollouts: about 25 ms at most on a loaded E-core, 3 ms for the quick battle's
/// decisions), the same on any machine; only searched when the normal search already scores
/// the battle won.
const FORCED_DEPTH: u32 = 12;
const FORCED_NODES: u64 = 20_000;
const FORCED_SHARE: u64 = 4;
/// A forced end's score moves this much toward 0 per action it takes, more than any worth
/// left (and 24 of them less than [`WIN`]): the fastest forced win scores highest.
const FORCED_PLY: i64 = 1 << 32;

/// What the battle is fought for, as «Максимум» weighs it (Razdor's rule, the owner's
/// decision of 2026-10-19: "if it's just a quick fight and not the campaign, there's no need
/// to avoid sacrificing units for the sake of a quicker finish, if 100% sure"). The same for
/// both sides, so for the enemy and for the player's auto-battle (Q, W) alike.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MaxStakes {
    /// A battle on the map (the default): the armies go on after it, so their units matter.
    /// Max's score already counts every unit kept and its HP, and a win is ranked by the
    /// worth it keeps; on top of that, when the search scores every line lost against the
    /// best reply (e.g. the turn limit a few actions ahead), Max does not pick among the
    /// losses by the worth left against that reply (which may leave a unit unhealed or throw
    /// it away) but keeps the original's choice (the 2026-10-18 report: a Heresiarch now
    /// keeps healing its Death Knight to the last turn, as the original's AI does).
    #[default]
    Campaign,
    /// «Свой бой» (`rules::custom`), nothing carried over: when a search with no rollouts
    /// proves a win within its horizon whatever the other side does, Max plays the fastest
    /// such win, whatever units it costs. Without that proof it plays as in a campaign
    /// battle, minus the fallback on a lost position.
    Quick,
}

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
    /// The rollouts of a node's leaves are played ahead on [`max_threads`] threads while
    /// the search takes the leaves in turn ([`Prefetch`], #18): the search itself stays on
    /// one thread and finds every score as before, so a node budget's choices do not change;
    /// only its time does. Needs the tables (the rollouts land in their cache).
    pub prefetch: bool,
    /// The player model (the opponent's side in the rollouts plays the focus-fire policy,
    /// [`Battle::greedy_move`], instead of the original's AI; the minimax plies are unchanged).
    /// Its table entries are kept apart ([`MaxBudget::key_salt`]).
    pub player_model: bool,
}

impl MaxBudget {
    /// The salt of this budget's table keys: 0 without the player model.
    fn key_salt(&self) -> StateKey {
        if self.player_model { MODEL_SALT } else { 0 }
    }

    /// The battle on screen: [`MAX_AI_BUDGET`] per decision.
    pub const GAME: MaxBudget = MaxBudget { time: Some(MAX_AI_BUDGET), max_depth: MAX_DEPTH, max_nodes: u64::MAX, short_rollouts: false, parallel: true, prefetch: false, player_model: false };

    /// No clock: at most `max_depth` actions ahead and `max_nodes` positions, the same
    /// choice on any machine.
    pub const fn fixed(max_depth: u32, max_nodes: u64) -> MaxBudget {
        MaxBudget { time: None, max_depth, max_nodes, short_rollouts: false, parallel: false, prefetch: false, player_model: false }
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

#[cfg(test)]
impl MaxMove {
    /// The action, `None` for the skip of the unit's turn.
    pub(super) fn plan(self) -> Option<Plan> {
        self.0
    }
}

/// What a decision found.
#[derive(Clone, Copy, Debug)]
pub struct MaxReport {
    pub choice: MaxMove,
    /// The original AI's choice in the same position.
    pub original: MaxMove,
    /// The deepest completed iteration's scores of the choice and of the original's (0 and
    /// 0 when not even depth 1 completed: the original's choice is kept; a quick fight's
    /// proven win: the proof's scores, `depth` its depth).
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

    /// What the battle is fought for, as Max weighs it ([`MaxStakes`]; a campaign battle
    /// unless the custom battle says otherwise).
    pub fn set_max_stakes(&mut self, stakes: MaxStakes) {
        self.max_stakes = stakes;
    }

    pub fn max_stakes(&self) -> MaxStakes {
        self.max_stakes
    }

    /// Every action the active unit `id` may take: its cells' actions, its steps, a pass.
    /// Every choice of the active unit, for either side (the search's own side and the
    /// opponent's model). No step into the reserve: the original's AI never enters it
    /// (battle.md §2, `row30_ai_leaves_the_reserve_and_never_enters_it`), and a unit there
    /// cannot be hit, so the worth left scores it well while its turns are lost (the
    /// 2026-10-20 report, #1034). Leaving the reserve stays a step like any other.
    ///
    /// For `own` (the searching side's unit), while a damaging strike is open, a step from the
    /// front row to the back row is dropped too (a retreat, the 2026-10-20 report #1037: the
    /// leaves score a full-HP unit out of reach well, and the strike it gives up is not seen).
    /// The opponent's model keeps every step.
    fn max_moves(&self, id: usize, own: bool) -> Vec<Plan> {
        let mut v: Vec<Plan> = self.all_options(id).into_iter().map(|(t, k)| Plan::Act(t, k)).collect();
        let retreat_barred = own && self.damaging_open(id);
        let from_front = self.fighters[id].slot.row == Row::Front;
        v.extend(
            self.moves(id)
                .into_iter()
                .filter(|s| s.row != Row::Reserve)
                .filter(|s| !(retreat_barred && from_front && s.row == Row::Back))
                .map(Plan::Move),
        );
        v.push(Plan::Pass);
        v
    }

    /// A damaging strike is open to the active unit `id`: a hostile action that deals damage
    /// (the benchmark's "idle with a damaging strike", `bench.rs`).
    fn damaging_open(&self, id: usize) -> bool {
        self.all_options(id).into_iter().any(|(t, k)| self.damaging(id, MaxMove(Some(Plan::Act(t, k)))))
    }

    /// Whether `m`, an action of `id`, is a hostile action that deals damage.
    fn damaging(&self, id: usize, m: MaxMove) -> bool {
        matches!(m.0, Some(Plan::Act(t, k)) if k.is_hostile() && matches!(self.preview(id, t, k), Preview::Damage(d) if d > 0))
    }

    /// The original AI's decision as the auto-battle carries it out: its plan, or a skip
    /// when the rules would refuse that plan.
    fn original_move(&self, legal: &[Plan]) -> MaxMove {
        MaxMove(self.plan(false).filter(|p| legal.contains(p)))
    }

    /// The order to search `id`'s actions in: the original's first, then the most damage
    /// (in tactical worth, a kill counting its whole worth again), heals, the rest of the
    /// cells' actions, steps, a pass.
    fn max_order(&self, id: usize, own: bool) -> Vec<MaxMove> {
        let legal = self.max_moves(id, own);
        // The original's own pick, even where the retreat rule of `max_moves` drops it: it stays
        // the first move and the floor of the root's choice (Safety: the `max_beats_the_original_on_*` tests).
        let original = if own { self.original_move(&self.max_moves(id, false)) } else { self.original_move(&legal) };
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

    /// The player model's move for the active unit `id` (the opponent's side in the rollouts
    /// with [`MaxBudget::player_model`]): the damaging action that takes the most worth now
    /// ([`Battle::worth_taken`], a kill counting the target's worth again). Ties go to the
    /// earliest in `max_order` (the original's pick first). With no damaging action, the
    /// original's pick. Deterministic.
    fn greedy_move(&self, id: usize) -> MaxMove {
        let order = self.max_order(id, false);
        let mut best: Option<(i64, MaxMove)> = None;
        for &m in &order {
            if self.damaging(id, m) {
                let taken = self.worth_taken(id, m);
                if best.is_none_or(|(b, _)| taken > b) {
                    best = Some((taken, m));
                }
            }
        }
        best.map_or(order[0], |(_, m)| m)
    }

    /// One step of a rollout: the original's AI (`auto_step`) for every unit, except that with
    /// `model` the units not on `side` play [`Battle::greedy_move`].
    fn rollout_step(&mut self, side: Team, model: bool) {
        if model {
            if let Some(id) = self.active() {
                if self.fighters[id].team != side {
                    let m = self.greedy_move(id);
                    self.play_max_move(m);
                    return;
                }
            }
        }
        self.auto_step();
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

    /// The root's tie-break slack: [`TIE_PERMILLE`] of the smallest unit's worth (a unit's worth
    /// is its tactical value × [`WORTH_SCALE`] at full HP; a tactical value below 1 counts as 1).
    fn tie_epsilon(&self) -> i64 {
        self.unit_worth() * TIE_PERMILLE / 1000
    }

    /// One unit's worth: the smallest unit's tactical value × [`WORTH_SCALE`].
    fn unit_worth(&self) -> i64 {
        let smallest = self.fighters.iter().map(|f| f.tactical.max(1) as i64).min().unwrap_or(1);
        smallest * WORTH_SCALE
    }

    /// Whether the root's move `m` (of the active unit) is a damaging strike.
    fn damaging_root(&self, m: MaxMove) -> bool {
        self.active().is_some_and(|id| self.damaging(id, m))
    }

    /// The alpha of the root's move `m` (not the original's) with a strike open: a damaging
    /// strike is searched above the best strike so far less the tie's slack, a step above the
    /// bar that strike sets (the best strike plus a unit's worth) and above the best so far less
    /// the slack. A score at or below it cannot change [`Battle::root_pick`]'s choice, and one
    /// above it is exact.
    fn root_alpha(&self, m: MaxMove, best: i64, strike: i64, eps: i64) -> i64 {
        if self.damaging_root(m) {
            strike - eps - 1
        } else {
            (strike + self.unit_worth()).max(best - eps) - 1
        }
    }

    /// The root's choice among `moves` (the original's first) with exact scores `values`:
    /// the tie-break toward damage ([`Battle::tie_pick`]) over the moves the rule below
    /// allows. With `strike_open` (a damaging strike is among `moves`), a step or a pass is
    /// allowed only when it beats the best damaging strike's score by more than one unit's
    /// worth (the 2026-10-20 report #1037, candidate B). Returns the index and its exact score.
    fn root_pick(&self, moves: &[MaxMove], values: &[i64], eps: i64, strike_open: bool) -> (usize, i64) {
        let Some(id) = self.active() else { return (0, values[0]) };
        let allowed: Vec<i64> = if strike_open {
            let best_strike = moves.iter().zip(values).filter(|(m, _)| self.damaging(id, **m)).map(|(_, &v)| v).max().unwrap_or(-INF);
            let bar = best_strike + self.unit_worth();
            moves.iter().zip(values).enumerate().map(|(i, (&m, &v))| if i == 0 || self.damaging(id, m) || v > bar { v } else { -INF }).collect()
        } else {
            values.to_vec()
        };
        let best = allowed.iter().copied().max().unwrap_or(values[0]);
        let k = self.tie_pick(moves, &allowed, best, eps);
        (k, values[k])
    }

    /// The worth `m` (an action of the active unit `id`) takes at once: the damage it deals in
    /// worth (the target's tactical value × its share of HP per point), a kill counting the
    /// target's whole worth again, as `max_order` counts it. A heal or any other move takes
    /// nothing.
    fn worth_taken(&self, id: usize, m: MaxMove) -> i64 {
        let Some(Plan::Act(t, k)) = m.0 else { return 0 };
        let tf = &self.fighters[t];
        let worth = (tf.tactical.max(1) as i64) * WORTH_SCALE / tf.max_hp().max(1) as i64;
        match self.preview(id, t, k) {
            Preview::Damage(d) => d.max(0) as i64 * worth + if d >= tf.hp { tf.tactical.max(1) as i64 * WORTH_SCALE } else { 0 },
            _ => 0,
        }
    }

    /// The root's tie-break toward damage (the owner's tempo experiment, `docs/flows/battle-ai.md`
    /// §8): among the root's moves (`moves`, the original's first) with exact scores `values`
    /// (the best is `best_v`), those within `eps` of the best and not below the original's
    /// (`values[0]`) are tied, and the one that takes the most worth at once
    /// ([`Battle::worth_taken`]) is played, the earliest (the original's order) on a tie. A move
    /// with a score at or above that floor is exact (the search's windows sit below it), so the
    /// pick is the same whatever the threads' timing.
    fn tie_pick(&self, moves: &[MaxMove], values: &[i64], best_v: i64, eps: i64) -> usize {
        let Some(id) = self.active() else { return 0 };
        let floor = (best_v - eps).max(values[0]);
        let mut pick: Option<(usize, i64)> = None;
        for (i, &m) in moves.iter().enumerate() {
            if values[i] < floor {
                continue;
            }
            let taken = self.worth_taken(id, m);
            if pick.is_none_or(|(_, t)| taken > t) {
                pick = Some((i, taken));
            }
        }
        pick.map_or(0, |(i, _)| i)
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
        let threads = max_threads();
        let side = self.fighters[id].team;
        let prefetch = match tables {
            Some(t) if budget.prefetch && !budget.short_rollouts && threads > 1 => Some(Prefetch::new(side, t, budget)),
            _ => None,
        };
        let Some(p) = prefetch.as_ref() else { return self.max_search(id, budget, tables, control, None) };
        let globals = patch_globals();
        std::thread::scope(|scope| {
            for _ in 1..threads {
                scope.spawn(move || p.work(globals));
            }
            let r = self.max_search(id, budget, tables, control, Some(p));
            p.quit();
            r
        })
    }

    /// [`Battle::max_choice_with`]'s search, with its rollouts played ahead by `prefetch`'s
    /// threads if any.
    fn max_search<'a>(&self, id: usize, budget: MaxBudget, tables: Option<&'a MaxTables>, control: Option<&'a MaxControl>, prefetch: Option<&'a Prefetch<'a>>) -> Option<MaxReport> {
        let t0 = Instant::now();
        let saved = patch_globals();
        let silent = crate::i18n::set_silent(true);
        let mut root = self.clone();
        root.log.clear();
        root.stale_reads.borrow_mut().clear();
        let mut moves = root.max_order(id, true);
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
            forced: false,
            forced_nodes: 0,
            forced_best: Default::default(),
            scratch: Vec::new(),
            roll: None,
            prefetch,
            help: (None, None),
            open_strike: root.damaging_open(id),
            salt: budget.key_salt(),
            model: budget.player_model,
        };
        let all_moves = root.max_order(id, false);
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
        let (mut choice, mut value, mut original_value, mut depth) = done.unwrap_or((original, 0, 0, 0));
        // Razdor's rule (`MaxStakes`, the owner's decision of 2026-10-19).
        match self.max_stakes {
            // A quick fight the search scores won: the fastest win, whatever units it costs,
            // if a search with no rollouts proves it against every reply (it looks at every
            // root action, also those the safety rule left out).
            MaxStakes::Quick if value >= WIN / 2 && all_moves.len() > 1 => {
                if let Some(won) = s.forced_win(&root, &all_moves) {
                    (choice, value, original_value, depth) = won;
                    settled = true;
                }
            }
            // A battle on the map that every line loses against the best reply: the
            // original's choice, not the line with the most worth left. (A proof of the loss
            // by a search with no rollouts would cost too much: the 2026-10-18 report's needs
            // 8 actions and half a million positions.)
            MaxStakes::Campaign if value <= -WIN / 2 => (choice, value) = (original, original_value),
            _ => {}
        }
        set_patch_globals(saved);
        crate::i18n::set_silent(silent);
        Some(MaxReport {
            choice,
            original,
            value,
            original_value,
            depth,
            settled,
            nodes: s.nodes + s.forced_nodes,
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
    /// The forced-win search of a quick fight ([`Search::forced_win`]): no rollouts, no tables
    /// (only the clock's stop from outside); the positions it searched.
    forced: bool,
    forced_nodes: u64,
    /// The proof's best move per position, tried first at the next depth (its scores depend
    /// on the distance from the root, so only the moves are kept).
    forced_best: std::collections::HashMap<StateKey, MaxMove>,
    /// The positions under way by distance from the root, and the rollout's: their buffers
    /// are reused (`Battle::clone_from`), the battle searched is never changed.
    scratch: Vec<Option<Battle>>,
    roll: Option<Battle>,
    /// The threads playing the rollouts of the leaves ahead ([`MaxBudget::prefetch`]).
    prefetch: Option<&'a Prefetch<'a>>,
    /// The buffers the search rolls out other leaves in while it waits ([`Prefetch::claim`]).
    help: (Option<Battle>, Option<Battle>),
    /// The root's unit has a damaging strike open ([`Battle::root_pick`]).
    open_strike: bool,
    /// The table key salt and the player model of the budget ([`MaxBudget`]).
    salt: StateKey,
    model: bool,
}

/// The rollouts of one node's leaves played ahead on other threads ([`MaxBudget::prefetch`],
/// #18). At a node whose children are all leaves to roll out, the search hands the node and
/// its actions in order to the threads; each takes the next action not yet taken, plays it and
/// rolls the leaf out by [`rollout`] into the tables' cache. The search goes on through the
/// children as ever, on its own thread: before each child it takes that child itself if no
/// thread has, or, while a thread has it, rolls out other children not yet taken and then
/// waits for it, and finds its rollout in the cache. A rollout is a function of its position
/// (the cache's premise), so every score is the one the search would have found alone and the
/// nodes it counts are the same: the choice is the same, only sooner. When a cut ends the
/// node early, the children not yet taken are dropped.
struct Prefetch<'t> {
    side: Team,
    tables: &'t MaxTables,
    salt: StateKey,
    model: bool,
    batch: std::sync::Mutex<Batch>,
    changed: std::sync::Condvar,
}

/// A leaf taken to roll out: its node's serial, its index, the action, the node's patch
/// globals and the node copied into a buffer.
struct Taken {
    serial: u64,
    index: usize,
    action: MaxMove,
    globals: PatchGlobals,
    node: Battle,
}

/// The node whose leaves are being rolled out.
#[derive(Default)]
struct Batch {
    /// Bumped by each new node, so a thread's late result does not touch the next one.
    serial: u64,
    base: Option<Battle>,
    globals: Option<PatchGlobals>,
    moves: Vec<MaxMove>,
    jobs: Vec<Job>,
    quit: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Job {
    Waiting,
    /// A thread rolls it out.
    Running,
    /// Rolled out, or taken by the search itself.
    Done,
}

impl<'t> Prefetch<'t> {
    fn new(side: Team, tables: &'t MaxTables, budget: MaxBudget) -> Prefetch<'t> {
        Prefetch { side, tables, salt: budget.key_salt(), model: budget.player_model, batch: Default::default(), changed: Default::default() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Batch> {
        self.batch.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The leaves of `moves` played in `b` (the patch's globals as they stand) to roll out.
    fn submit(&self, b: &Battle, moves: &[MaxMove]) {
        let mut g = self.lock();
        g.serial += 1;
        g.base = Some(copy_into(g.base.take(), b));
        g.globals = Some(patch_globals());
        g.moves.clear();
        g.moves.extend_from_slice(moves);
        g.jobs.clear();
        g.jobs.resize(moves.len(), Job::Waiting);
        drop(g);
        self.changed.notify_all();
    }

    /// The search comes to child `i`: taken by itself if no thread has it, else waited for,
    /// rolling out the children not yet taken meanwhile (in `bufs`; the patch's globals are
    /// put back after each).
    fn claim(&self, i: usize, bufs: &mut (Option<Battle>, Option<Battle>)) {
        let mut g = self.lock();
        loop {
            match g.jobs.get(i) {
                Some(Job::Waiting) => {
                    g.jobs[i] = Job::Done;
                    return;
                }
                Some(Job::Running) => match Self::take(&mut g, &mut bufs.0) {
                    Some(t) => {
                        drop(g);
                        let globals = patch_globals();
                        let (serial, index) = (t.serial, t.index);
                        bufs.0 = Some(self.run(t, &mut bufs.1));
                        set_patch_globals(globals);
                        self.done(serial, index);
                        g = self.lock();
                    }
                    None => g = self.changed.wait(g).unwrap_or_else(|e| e.into_inner()),
                },
                _ => return,
            }
        }
    }

    /// The first leaf no one has taken, marked as taken by a thread.
    fn take(g: &mut Batch, buf: &mut Option<Battle>) -> Option<Taken> {
        let index = g.jobs.iter().position(|&j| j == Job::Waiting)?;
        g.jobs[index] = Job::Running;
        let node = copy_into(buf.take(), g.base.as_ref().expect("a node under way"));
        Some(Taken { serial: g.serial, index, action: g.moves[index], globals: g.globals.expect("a node under way"), node })
    }

    /// Plays the leaf and rolls it out into the cache (in the buffer `roll`); gives the leaf's
    /// buffer back. A panic here is left to the search, which meets the same leaf itself.
    fn run(&self, t: Taken, roll: &mut Option<Battle>) -> Battle {
        let mut c = t.node;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            set_patch_globals(t.globals);
            c.play_max_move(t.action);
            if c.outcome() == Outcome::Ongoing {
                let k = c.max_key(self.side) ^ self.salt;
                if self.tables.rollout_get(k).is_none() {
                    rollout(Some(self.tables), self.side, &c, k, roll, (self.salt, self.model), &mut || false);
                }
            }
        }));
        c
    }

    /// Leaf `index` of node `serial` is in the cache.
    fn done(&self, serial: u64, index: usize) {
        let mut g = self.lock();
        if g.serial == serial {
            g.jobs[index] = Job::Done;
        }
        drop(g);
        self.changed.notify_all();
    }

    /// The node is done (or cut): its children not yet taken are dropped.
    fn finish(&self) {
        let mut g = self.lock();
        g.serial += 1;
        g.jobs.clear();
    }

    fn quit(&self) {
        self.lock().quit = true;
        self.changed.notify_all();
    }

    /// A thread's loop: the next waiting leaf of the node under way, played and rolled out
    /// into the cache, until [`Prefetch::quit`]. `globals`: the search's at its start.
    fn work(&self, globals: PatchGlobals) {
        set_patch_globals(globals);
        crate::i18n::set_silent(true);
        let (mut leaf, mut roll) = (None, None);
        loop {
            let t = {
                let mut g = self.lock();
                loop {
                    if g.quit {
                        return;
                    }
                    if let Some(t) = Self::take(&mut g, &mut leaf) {
                        break t;
                    }
                    g = self.changed.wait(g).unwrap_or_else(|e| e.into_inner());
                }
            };
            let (serial, index) = (t.serial, t.index);
            leaf = Some(self.run(t, &mut roll));
            self.done(serial, index);
        }
    }
}

/// A leaf's full rollout (no `short_rollouts`): the rest of the battle by the original's AI
/// on both sides, scored for `side`; with `tables` (and `key`, the start's key there), cached
/// under the key of its start and of its first positions, and cut short by a position found
/// there. `None` when `stop` says so (asked every [`CLOCK_EVERY`] steps). `roll` is the buffer
/// it plays in.
fn rollout(tables: Option<&MaxTables>, side: Team, b: &Battle, key: StateKey, roll: &mut Option<Battle>, model: (StateKey, bool), stop: &mut dyn FnMut() -> bool) -> Option<i64> {
    // `model`: the table key salt and the player model's switch (`MaxBudget::player_model`).
    let (salt, model) = model;
    let mut chain: [StateKey; ROLLOUT_KEYS] = [0; ROLLOUT_KEYS];
    let mut keyed = 0;
    if tables.is_some() {
        chain[0] = key;
        keyed = 1;
    }
    let mut r = copy_into(roll.take(), b);
    let mut known = None;
    let mut stopped = false;
    for i in 0..AUTO_PLAY_STEPS {
        if r.outcome() != Outcome::Ongoing {
            break;
        }
        if i % CLOCK_EVERY == 0 && stop() {
            stopped = true;
            break;
        }
        r.rollout_step(side, model);
        if let Some(t) = tables.filter(|_| keyed == i + 1 && keyed < ROLLOUT_KEYS) {
            let k = r.max_key(side) ^ salt;
            if let Some(v) = t.rollout_get(k) {
                known = Some(v);
                break;
            }
            chain[keyed] = k;
            keyed += 1;
        }
    }
    let v = known.unwrap_or_else(|| r.max_eval(side));
    *roll = Some(r);
    if stopped {
        return None;
    }
    if let Some(t) = tables {
        for &k in &chain[..keyed] {
            t.rollout_add(k, v);
        }
    }
    Some(v)
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
        if self.forced {
            return self.control.is_some_and(|c| c.stop.load(Ordering::Relaxed));
        }
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
        let (mut best, mut best_v, mut original_value) = (moves[0], -INF, 0);
        let slack = self.tie_slack(b);
        let strike = self.strike_open();
        let mut strike_v = -INF;
        let mut values = Vec::with_capacity(moves.len());
        let ahead = self.prefetch_leaves(b, moves, depth, 0);
        for (i, &m) in moves.iter().enumerate() {
            if ahead {
                self.prefetch.expect("prefetching").claim(i, &mut self.help);
            }
            let alpha = match slack {
                _ if i == 0 => -INF,
                // With a strike open, the strikes and the steps have their own bounds (`root_alpha`).
                Some(eps) if strike => b.root_alpha(m, best_v, strike_v, eps),
                None => best_v,
                // Below the best by the tie's slack: a score within it comes out exact.
                Some(eps) => best_v - eps - 1,
            };
            let v = self.child(b, m, depth - 1, 1, alpha, INF);
            if self.aborted {
                if ahead {
                    self.prefetch.expect("prefetching").finish();
                }
                return None;
            }
            values.push(v);
            if strike && b.damaging_root(m) {
                strike_v = strike_v.max(v);
            }
            if i == 0 {
                (best, best_v, original_value) = (m, v, v);
            } else if v > best_v {
                (best, best_v) = (m, v);
            }
        }
        if ahead {
            self.prefetch.expect("prefetching").finish();
        }
        if let Some(eps) = slack {
            let (k, v) = b.root_pick(moves, &values, eps, strike);
            (best, best_v) = (moves[k], v);
        }
        Some((best, best_v, original_value, values))
    }

    /// The root's tie-break slack ([`Battle::tie_pick`]), `None` in the forced-win search, which
    /// only asks whether a win is proven.
    fn tie_slack(&self, b: &Battle) -> Option<i64> {
        (!self.forced).then(|| b.tie_epsilon())
    }

    /// The root's strike rule ([`Battle::root_pick`]) applies: a normal search whose root unit
    /// has a damaging strike open.
    fn strike_open(&self) -> bool {
        self.open_strike && !self.forced
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
            forced: self.forced,
            forced_nodes: 0,
            forced_best: Default::default(),
            scratch: Vec::new(),
            roll: None,
            prefetch: None,
            help: (None, None),
            open_strike: self.open_strike,
            salt: self.salt,
            model: self.model,
        }
    }

    /// At a node whose children (`moves`, in the order searched) are all leaves to roll out,
    /// their rollouts are handed to the [`Prefetch`] threads; `true` if so.
    fn prefetch_leaves(&self, b: &Battle, moves: &[MaxMove], depth: u32, ply: u32) -> bool {
        let Some(p) = self.prefetch.filter(|_| depth == 1 && ply < ROLLOUT_PLIES && !self.forced && !self.short && moves.len() > 1) else { return false };
        p.submit(b, moves);
        true
    }

    /// A quick fight's proof of a win ([`MaxStakes::Quick`]): iterative deepening to
    /// [`FORCED_DEPTH`] actions and [`FORCED_NODES`] positions in all (less for a small node
    /// budget, `FORCED_SHARE`), every action of both sides, no rollouts and no tables; a line
    /// scores only if the battle ends on it (else 0),
    /// a win the more the sooner it comes, a loss the less ([`FORCED_PLY`]). The first depth
    /// where the best move scores above 0 proves a win against every reply: that move (the
    /// fastest win, then the most worth left), its score, the original's and the depth.
    /// `None` with no proof within the budget. Only the stop of a search on a worker ends it
    /// early, not the clock (the node limit keeps it to a few milliseconds).
    fn forced_win(&mut self, b: &Battle, moves: &[MaxMove]) -> Option<(MaxMove, i64, i64, u32)> {
        let (tables, max_nodes, nodes, deadline) = (self.tables.take(), self.max_nodes, self.nodes, self.deadline.take());
        (self.forced, self.aborted) = (true, false);
        self.max_nodes = nodes.saturating_add(FORCED_NODES.min(max_nodes.saturating_mul(FORCED_SHARE)));
        let mut found = None;
        for depth in 1..=FORCED_DEPTH {
            self.cut = false;
            let Some((best, value, original_value, _)) = self.root(b, moves, depth) else { break };
            if value > 0 {
                found = Some((best, value, original_value, depth));
                break;
            }
            // Every line ended within the depth and none is a forced win: none exists.
            if !self.cut {
                break;
            }
        }
        // Its positions are counted apart from the normal search's.
        self.forced_best.clear();
        self.forced_nodes += self.nodes - nodes;
        (self.tables, self.max_nodes, self.nodes, self.deadline) = (tables, max_nodes, nodes, deadline);
        (self.forced, self.aborted, self.cut) = (false, false, false);
        found
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
        let best = Mutex::new((-INF, usize::MAX, -INF));
        let values = Mutex::new(vec![None; moves.len()]);
        let slack = self.tie_slack(b);
        let strike = self.strike_open();
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
                            let g = *best.lock().unwrap_or_else(|e| e.into_inner());
                            let alpha = match (g, slack) {
                                _ if i == 0 => -INF,
                                ((_, _, strike_v), Some(eps)) if strike => b.root_alpha(moves[i], g.0, strike_v, eps),
                                ((v, _, _), Some(eps)) => v - eps - 1,
                                ((v, at, _), None) if at < i => v,
                                ((v, _, _), None) => v - 1,
                            };
                            let v = s.child(&b, moves[i], depth - 1, 1, alpha, INF);
                            if s.aborted {
                                break;
                            }
                            values.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(v);
                            let mut g = best.lock().unwrap_or_else(|e| e.into_inner());
                            if (i == 0 || v > alpha) && (v > g.0 || (v == g.0 && i < g.1)) {
                                (g.0, g.1) = (v, i);
                            }
                            if strike && b.damaging_root(moves[i]) {
                                g.2 = g.2.max(v);
                            }
                            drop(g);
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
        let (v, at, _) = best.into_inner().unwrap_or_else(|e| e.into_inner());
        match values {
            Some(values) if all && !self.aborted && at < moves.len() => match slack {
                Some(eps) => {
                    let (k, v) = b.root_pick(moves, &values, eps, strike);
                    Some((moves[k], v, values[0], values))
                }
                None => Some((moves[at], v, values[0], values)),
            },
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
        let key = self.tables.map(|_| b.max_key(self.side) ^ self.salt);
        let forced_key = self.forced.then(|| b.max_key(self.side));
        let mut first = forced_key.and_then(|k| self.forced_best.get(&k).copied());
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
        let own = b.fighters[id].team == self.side && !self.forced;
        let mut order = b.max_order(id, own);
        if let Some(i) = first.and_then(|f| order.iter().position(|&m| m == f)) {
            let m = order.remove(i);
            order.insert(0, m);
        }
        let ahead = self.prefetch_leaves(b, &order, depth, ply);
        let done = |s: &Self| {
            if ahead {
                s.prefetch.expect("prefetching").finish();
            }
        };
        for (i, m) in order.into_iter().enumerate() {
            if ahead {
                self.prefetch.expect("prefetching").claim(i, &mut self.help);
            }
            let v = self.child(b, m, depth - 1, ply + 1, alpha, beta);
            if self.aborted {
                done(self);
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
        done(self);
        let cut = self.cut;
        self.cut |= outer_cut;
        if let (Some(k), Some(m)) = (forced_key, best_move) {
            self.forced_best.insert(k, m);
        }
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
        if self.forced {
            // Only the battle's end counts: the sooner a win the better, a loss the later.
            return match b.outcome() {
                Outcome::Ongoing => 0,
                _ => {
                    let v = b.max_eval(self.side);
                    v - v.signum() * ply as i64 * FORCED_PLY
                }
            };
        }
        if b.outcome() != Outcome::Ongoing || ply > ROLLOUT_PLIES {
            return b.max_eval(self.side);
        }
        // The root's children keep full rollouts: the first iteration's safety rule weighs
        // each action by the original's own play to the end.
        let short = self.short && ply >= 2;
        if !short {
            let key = self.tables.map_or(0, |_| b.max_key(self.side) ^ self.salt);
            if let Some(v) = self.tables.and_then(|t| t.rollout_get(key)) {
                self.rollout_hits += 1;
                return v;
            }
            self.rollouts += 1;
            let mut roll = self.roll.take();
            let v = rollout(self.tables, self.side, b, key, &mut roll, (self.salt, self.model), &mut || self.time_up());
            self.roll = roll;
            return v.unwrap_or_else(|| {
                self.aborted = true;
                0
            });
        }
        let salt = if short { SHORT_SALT } else { 0 };
        let mut chain: [StateKey; ROLLOUT_KEYS] = [0; ROLLOUT_KEYS];
        let mut keyed = 0;
        if let Some(t) = self.tables {
            let k = b.max_key(self.side) ^ salt ^ self.salt;
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
            r.rollout_step(self.side, self.model);
            if short && r.round >= r0 + SHORT_ROUNDS && r.outcome() == Outcome::Ongoing {
                if let Some(v) = r.max_static(self.side) {
                    known = Some(v);
                    break;
                }
            }
            if let Some(t) = self.tables.filter(|_| *keyed == i + 1 && *keyed < ROLLOUT_KEYS) {
                let k = r.max_key(self.side) ^ salt ^ self.salt;
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

#[cfg(test)]
mod bench;
