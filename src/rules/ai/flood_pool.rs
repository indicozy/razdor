//! The planner's floods on worker threads (`Game::ai_plan`, the join points in `ai.rs`).
//!
//! A plan's flood depends only on its inputs (the costs, the multiplier map, the seeds, the
//! start cell and the cells to erase), and those are fixed when the planner makes its plan: the
//! serial arrival rules that follow (battles, captures, the wander draws of later arrivals) only
//! change what comes after. So the flood is dispatched at that point and runs on a worker while
//! the arrivals of the same moment go on; its result is written into the army at a join point,
//! before anything reads or writes that army's path (`Game::settle`). The result is the one the
//! serial planner computes, whatever the timing: a flood is a function of its input.
//!
//! Threads: [`threads_for_tick`] picks how many workers take jobs in each AI tick, from the plans
//! the ticks before made ([`policy`]). `RAZDOR_PLAN_THREADS` overrides the count (0 turns them
//! off). The pool starts its workers once, at most [`workers`]; a tick that wants fewer lets the
//! others sleep, so no thread starts or stops per tick. Off with no threads (wasm), on a small map
//! ([`MIN_CELLS`]), or when a worker has not taken a job when its result is needed, the caller runs
//! it itself (a job is never left waiting for a worker that is busy).

use std::any::Any;
use std::collections::VecDeque;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Condvar, Mutex, OnceLock};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{direct, window};
use crate::rules::map::{Tile, TileMap};

/// The most worker threads the planner starts (the game's others take the rest of the cores).
pub const MAX_WORKERS: usize = 7;
/// Maps below this many cells plan on the calling thread: a flood there costs less than a hand-off.
pub const MIN_CELLS: usize = 40_000;

/// How a plan searches: the experimental rules («Быстрый ИИ на больших картах», «Прямой путь к
/// единственной цели»). The default is the original's flood over the whole map.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Search {
    /// The window's radius (`ai::window`), or None for the whole map.
    pub window: Option<i32>,
    /// A plan with one target cell goes by A* (`ai::direct`), when it finds a path.
    pub direct: bool,
}

/// Which search made a plan (the tests read it; the planner does not).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// The flood over the whole map, and its descent.
    Exact,
    /// The flood inside the window.
    Window,
    /// The A* path to the single target.
    Direct,
}

/// Everything a flood reads: the map's geometry, the planner's cost and multiplier maps, its
/// seeds, its start cell, the cells erased from the flood before the path is read, the countdown
/// the path resets to, and the search.
pub struct PlanInput {
    /// The grid and size of the map ([`TileMap::geometry`]).
    pub geom: TileMap,
    pub cost: Vec<u16>,
    pub mult: Vec<u16>,
    pub seeds: Vec<(Tile, u32)>,
    pub from: Tile,
    pub erase: Vec<Tile>,
    /// The re-plan countdown a found path sets (`AIGetPathDistance`).
    pub reach: i32,
    pub search: Search,
}

/// What a plan leaves in its army, as the serial planner would have written it.
pub struct PlanOutput {
    /// The path, `None` when no seed was kept (the army's path is then cleared).
    pub path: Option<Vec<Tile>>,
    /// The planner's step weights along the path (the army's `path_weights`, from index 0).
    pub weights: Vec<u8>,
    /// The re-plan countdown a found path sets.
    pub reach: i32,
    /// The direction of the army's next step from its cell (`stand_facing`).
    pub facing: Option<Tile>,
}

/// The plan's output from its read path (`None`: no seed kept, the army's path is cleared).
fn read(grid: crate::rules::map::Grid, from: Tile, path: Option<Vec<Tile>>, reach: i32) -> PlanOutput {
    let weights = path.as_ref().map_or_else(Vec::new, |p| {
        let mut prev = from;
        p.iter()
            .map(|&t| {
                let w = grid.weight(prev, t) as u8;
                prev = t;
                w
            })
            .collect()
    });
    let facing = path.as_ref().and_then(|p| p.first().map(|t| (t.0 - from.0, t.1 - from.1)));
    PlanOutput { path, weights, reach, facing }
}

/// The flood, the erasures, the read path: the serial planner's work from its inputs. `geom`
/// is the map or its [`TileMap::geometry`]. The search is the experimental one when its rules
/// are on (see [`compute_routed`]); the default is the original's flood.
#[allow(clippy::too_many_arguments)]
pub fn compute_with(geom: &TileMap, cost: &[u16], mult: &[u16], seeds: &[(Tile, u32)], from: Tile, erase: &[Tile], reach: i32, search: Search) -> PlanOutput {
    compute_routed(geom, cost, mult, seeds, from, erase, reach, search).0
}

/// [`compute_with`] with the route that made the plan. A direct plan needs exactly one target
/// cell (the seeds the flood keeps: on an open cell, not the army's own) and a path to it, else
/// the window (`search.window`) or the whole-map flood makes it.
#[allow(clippy::too_many_arguments)]
pub fn compute_routed(geom: &TileMap, cost: &[u16], mult: &[u16], seeds: &[(Tile, u32)], from: Tile, erase: &[Tile], reach: i32, search: Search) -> (PlanOutput, Route) {
    if search.direct {
        if let Some(out) = direct_plan(geom, cost, mult, seeds, from, erase, reach) {
            return (out, Route::Direct);
        }
    }
    if let Some(radius) = search.window {
        return (window_plan(geom, cost, mult, seeds, from, erase, reach, radius), Route::Window);
    }
    let mut field = geom.flood_maps(cost, mult, seeds, from);
    for &t in erase {
        field.erase(t);
    }
    let path = (field.kept > 0).then(|| geom.descend(&field, from));
    (read(geom.grid, from, path, reach), Route::Exact)
}

/// The one target the flood would keep: the distinct seed cells on open cells, not the army's own;
/// `Some` only when there is exactly one.
fn single_target(geom: &TileMap, cost: &[u16], seeds: &[(Tile, u32)], from: Tile) -> Option<Tile> {
    let mut found: Option<Tile> = None;
    for &(t, _) in seeds {
        if !geom.in_bounds(t) || t == from || cost[(t.1 * geom.w + t.0) as usize] == 0 {
            continue;
        }
        match found {
            None => found = Some(t),
            Some(f) if f == t => {}
            Some(_) => return None,
        }
    }
    found
}

/// The direct plan (`ai::direct`): the A* path to the single target, or None when the plan has
/// no single target, the target is erased, or no path exists. An army on an erased cell stands
/// still, as the flood's descent does from an erased start.
fn direct_plan(geom: &TileMap, cost: &[u16], mult: &[u16], seeds: &[(Tile, u32)], from: Tile, erase: &[Tile], reach: i32) -> Option<PlanOutput> {
    let target = single_target(geom, cost, seeds, from)?;
    if erase.contains(&target) {
        return None;
    }
    let path = if erase.contains(&from) { Vec::new() } else { direct::path(geom, cost, mult, target, from, erase) };
    if path.is_empty() && !erase.contains(&from) {
        return None;
    }
    Some(read(geom.grid, from, Some(path), reach))
}

/// The window plan (`ai::window`): the flood inside the window of `radius` around the army, with
/// the seeds outside moved to its border, then the erasures and the descent as the whole-map one.
#[allow(clippy::too_many_arguments)]
fn window_plan(geom: &TileMap, cost: &[u16], mult: &[u16], seeds: &[(Tile, u32)], from: Tile, erase: &[Tile], reach: i32, radius: i32) -> PlanOutput {
    let win = window::bounds(geom.w, geom.h, from, radius);
    let mut closed = mult.to_vec();
    window::close_outside(&mut closed, geom.w, geom.h, win);
    let moved = window::seeds_in(geom, cost, seeds, win, window::step_estimate(cost));
    let mut field = geom.flood_maps(cost, &closed, &moved, from);
    for &t in erase {
        field.erase(t);
    }
    let path = (field.kept > 0).then(|| geom.descend(&field, from));
    read(geom.grid, from, path, reach)
}

/// [`compute_with`] on an owned input (a job's).
pub fn compute(input: PlanInput) -> PlanOutput {
    let PlanInput { geom, cost, mult, seeds, from, erase, reach, search } = input;
    compute_with(&geom, &cost, &mult, &seeds, from, &erase, reach, search)
}

/// A flood on its way to its result: dispatched and not yet taken.
pub enum Ticket {
    /// Computed on the calling thread already.
    Ready(Box<PlanOutput>),
    /// Queued for a worker, or taken back by the caller when its result is needed.
    #[cfg(not(target_arch = "wasm32"))]
    Queued(Arc<Job>),
}

impl Ticket {
    /// The result, computed now if no worker has taken the job.
    pub fn wait(self) -> PlanOutput {
        match self {
            Ticket::Ready(out) => *out,
            #[cfg(not(target_arch = "wasm32"))]
            Ticket::Queued(job) => job.wait(),
        }
    }
}

/// Whether a flood of a map of `cells` cells goes to a worker: this tick has workers ([`set_threads`])
/// and the map is big enough. Otherwise the caller floods it itself, at once ([`compute_with`]).
pub fn offloads(cells: usize) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        cells >= MIN_CELLS && ACTIVE.load(Ordering::Relaxed) > 0
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = cells;
        false
    }
}

/// Starts a flood on a worker (when [`offloads`] says so) or runs it at once.
pub fn dispatch(input: PlanInput) -> Ticket {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if offloads(input.cost.len()) {
            if let Some(queue) = pool() {
                let job = Arc::new(Job { state: Mutex::new(State::Queued(Box::new(input))), done: Condvar::new() });
                queue.push(job.clone());
                return Ticket::Queued(job);
            }
        }
    }
    Ticket::Ready(Box::new(compute(input)))
}

/// How many floods may be in flight at once (each holds its input and its result).
pub fn max_in_flight() -> usize {
    workers().max(1) * 2 + 2
}

/// The unit of the whole-map policy's work (plans × cells): each doubling of the work in these
/// units adds one thread (see [`policy`]).
pub const WORK_UNIT: f64 = 100_000.0;

/// The worker count of a tick, a pure function of its inputs (`cores` is the cores the process
/// may use; the result never changes the AI's choices, only the speed). `plans_per_tick` is a
/// moving average of the plans the earlier ticks made (0 on the first tick), `cells` the map's
/// cells, `side` its larger side, `fast_ai` the «Ускоренный ИИ» switch. Zero below [`MIN_CELLS`]
/// and on one core. At most `min(MAX_WORKERS, cores - 1)`.
///
/// Whole-map flood (`fast_ai` off): the flood costs about 8.8 ns per cell per plan, so the work
/// is `plans × cells` and the count is `round(log2(work / WORK_UNIT))`: the thread-policy bench
/// (2026-11-04/05, 8 logical CPUs, 96 ticks) found 2 threads best below ~0.15 M of work per tick,
/// 3–5 up to ~2 M, 7 above ~7 M; the rule is within 2.4% of the best count on maps of 200–800 cells
/// a side with 10–1000 armies.
///
/// Window flood (`fast_ai` on): a plan costs about 0.35 ms whatever the map, but the hand-off to a
/// worker grows with the map, so the best count falls with the side: 5–7 at 200, 3–5 at 400 and
/// 2 at 800 (blobs; the same bench). The caps are 6, 4 and 2, and a tick gets at most one thread
/// per three plans, so a tick with a few plans does not hand them off. Checked again on the islands
/// map at 800 with 255 armies (2026-11-05): 2 threads take 4.0 s against 5.8 s serial, so the cap
/// holds there too; the rule does not see the terrain.
pub fn policy(plans_per_tick: f64, cells: usize, side: u32, fast_ai: bool, cores: usize) -> usize {
    let max = cores.saturating_sub(1).min(MAX_WORKERS);
    if cells < MIN_CELLS || max == 0 || plans_per_tick <= 0.0 {
        return 0;
    }
    if fast_ai {
        let cap = if side <= 200 {
            6
        } else if side <= 400 {
            4
        } else {
            2
        };
        let by_plans = (plans_per_tick / 3.0).floor() as usize;
        return cap.min(max).min(by_plans);
    }
    let best = (plans_per_tick * cells as f64 / WORK_UNIT).log2().round();
    (best.max(0.0) as usize).min(max)
}

/// The worker count of the tick about to run: [`policy`] on this machine, or `RAZDOR_PLAN_THREADS`
/// when it is set (at most [`MAX_WORKERS`], whatever the policy says). Zero on wasm and below
/// [`MIN_CELLS`]. Set with [`set_threads`] at the tick's start.
pub fn threads_for_tick(plans_per_tick: f64, cells: usize, side: u32, fast_ai: bool) -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (plans_per_tick, cells, side, fast_ai);
        0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if cells < MIN_CELLS {
            return 0;
        }
        env_threads().unwrap_or_else(|| policy(plans_per_tick, cells, side, fast_ai, cores()))
    }
}

/// Sets how many workers take jobs from now on, at most [`workers`]. The others sleep until a
/// later tick wants them: a change wakes the sleepers and starts or stops no thread. Floods already
/// taken by a worker finish; queued ones the caller takes back when it needs them.
pub fn set_threads(n: usize) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = n;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let n = n.min(workers());
        if ACTIVE.swap(n, Ordering::Relaxed) != n {
            // Taking the lock orders the change before a sleeper's next check: no lost wake-up.
            if let Some(Some(queue)) = POOL.get() {
                let _held = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
                queue.ready.notify_all();
            }
        }
    }
}

/// The most worker threads the planner runs (the pool's size): `RAZDOR_PLAN_THREADS` if set (0
/// for none), else the cores left free after one, at most [`MAX_WORKERS`]. Zero on wasm.
pub fn workers() -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        env_threads().unwrap_or_else(|| cores().saturating_sub(1).min(MAX_WORKERS))
    }
}

/// `RAZDOR_PLAN_THREADS`, read once (at most [`MAX_WORKERS`]); None when unset or not a number.
#[cfg(not(target_arch = "wasm32"))]
fn env_threads() -> Option<usize> {
    static ENV: OnceLock<Option<usize>> = OnceLock::new();
    *ENV.get_or_init(|| std::env::var("RAZDOR_PLAN_THREADS").ok().and_then(|v| v.trim().parse::<usize>().ok()).map(|n| n.min(MAX_WORKERS)))
}

/// The cores this process may run on, read once (`available_parallelism`; 1 when unknown).
#[cfg(not(target_arch = "wasm32"))]
fn cores() -> usize {
    static CORES: OnceLock<usize> = OnceLock::new();
    *CORES.get_or_init(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
}

/// The workers that take jobs now (see [`set_threads`]); 0 until the first tick sets it.
#[cfg(not(target_arch = "wasm32"))]
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// The worker pool, once it is started (`None` until then, or when no thread could be started).
#[cfg(not(target_arch = "wasm32"))]
static POOL: OnceLock<Option<&'static Queue>> = OnceLock::new();

#[cfg(not(target_arch = "wasm32"))]
enum State {
    Queued(Box<PlanInput>),
    Running,
    Done(Result<Box<PlanOutput>, Box<dyn Any + Send>>),
    Taken,
}

/// One flood shared by the caller and the worker that takes it.
#[cfg(not(target_arch = "wasm32"))]
pub struct Job {
    state: Mutex<State>,
    done: Condvar,
}

#[cfg(not(target_arch = "wasm32"))]
impl Job {
    /// The caller's side: computes the job if no worker has started it, else waits for it.
    fn wait(self: Arc<Job>) -> PlanOutput {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match std::mem::replace(&mut *st, State::Taken) {
                State::Queued(input) => {
                    *st = State::Running;
                    drop(st);
                    return compute(*input);
                }
                State::Running => {
                    *st = State::Running;
                    st = self.done.wait(st).unwrap_or_else(|e| e.into_inner());
                }
                State::Done(result) => {
                    return match result {
                        Ok(out) => *out,
                        Err(panic) => std::panic::resume_unwind(panic),
                    };
                }
                State::Taken => unreachable!("a planner's result is taken once"),
            }
        }
    }

    /// The worker's side: takes the job if it is still queued, runs it, and publishes it.
    fn run(&self) {
        let input = {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            match std::mem::replace(&mut *st, State::Running) {
                State::Queued(input) => input,
                other => {
                    *st = other;
                    return;
                }
            }
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compute(*input)));
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        *st = State::Done(result.map(Box::new));
        self.done.notify_all();
    }
}

/// The jobs waiting for a worker.
#[cfg(not(target_arch = "wasm32"))]
struct Queue {
    jobs: Mutex<VecDeque<Arc<Job>>>,
    ready: Condvar,
}

#[cfg(not(target_arch = "wasm32"))]
impl Queue {
    fn push(&self, job: Arc<Job>) {
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).push_back(job);
        // All the sleepers, not one: one that is above the active count would take the wake-up and sleep on.
        self.ready.notify_all();
    }

    /// The next job for worker `k`: only the workers below [`ACTIVE`] take jobs.
    fn next(&self, k: usize) -> Arc<Job> {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if k < ACTIVE.load(Ordering::Relaxed) {
                if let Some(job) = jobs.pop_front() {
                    return job;
                }
            }
            jobs = self.ready.wait(jobs).unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// The worker pool, started on first use with [`workers`] threads (None when no thread could be
/// started).
#[cfg(not(target_arch = "wasm32"))]
fn pool() -> Option<&'static Queue> {
    *POOL.get_or_init(|| {
        let n = workers();
        if n == 0 {
            return None;
        }
        // Leaked once: the workers live as long as the process.
        let queue: &'static Queue = Box::leak(Box::new(Queue { jobs: Mutex::new(VecDeque::new()), ready: Condvar::new() }));
        let started = (0..n)
            .filter(|&k| {
                std::thread::Builder::new()
                    .name(format!("razdor-planner-{k}"))
                    .spawn(move || loop {
                        queue.next(k).run();
                    })
                    .is_ok()
            })
            .count();
        (started > 0).then_some(queue)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::Surface;
    use crate::rules::map::Grid;

    /// A square map of grass with a few walls (closed cells) and marsh (dearer cells), and a
    /// seeded multiplier map: the planner's inputs of a big enough map.
    fn input(seed: u32, from: Tile) -> PlanInput {
        let (w, h) = (240i32, 240i32);
        let mut codes = vec![Surface::GrassPlain as u8; (w * h) as usize];
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n
        };
        for c in codes.iter_mut() {
            match next(12) {
                0 => *c = Surface::ImpassableSwamp as u8,
                1 | 2 => *c = Surface::Marsh as u8,
                _ => {}
            }
        }
        let map = TileMap::from_codes(Grid::Square8, w, h, &codes, Vec::new());
        let cost = map.land_costs().to_vec();
        let mult: Vec<u16> = (0..cost.len()).map(|_| if next(20) == 0 { 0 } else { 1 + next(3) as u16 }).collect();
        let seeds: Vec<(Tile, u32)> = (0..6).map(|_| ((next(w as u32) as i32, next(h as u32) as i32), next(40))).collect();
        let erase: Vec<Tile> = (0..4).map(|_| (next(w as u32) as i32, next(h as u32) as i32)).collect();
        PlanInput { geom: map.geometry(), cost, mult, seeds, from, erase, reach: 7, search: Search::default() }
    }

    fn same(a: &PlanOutput, b: &PlanOutput) -> bool {
        a.path == b.path && a.weights == b.weights && a.reach == b.reach && a.facing == b.facing
    }

    /// Floods handed to the pool come back as the calling thread computes them, whichever
    /// order they are taken in, and a job that no worker took is run by its caller.
    #[test]
    fn pooled_floods_are_the_inline_ones_in_any_order() {
        // The tests share the pool's active count: the workers this test wants are on.
        set_threads(workers().max(1));
        let mut jobs = Vec::new();
        for k in 0..6u32 {
            let from = ((k as i32 * 37) % 240, (k as i32 * 53 + 11) % 240);
            let inline = compute(input(k + 1, from));
            jobs.push((dispatch(input(k + 1, from)), inline));
        }
        // Taken last to first: the later ones may be queued or already running on a worker.
        while let Some((ticket, inline)) = jobs.pop() {
            assert!(same(&ticket.wait(), &inline));
        }
    }

    /// The whole-map policy is `round(log2(plans × cells / 1e5))`, from 0 (the first tick, or
    /// the work of a small map) to the cores left after one, at most 7.
    #[test]
    fn the_whole_map_policy_is_log2_of_the_work() {
        let big = 800 * 800;
        assert_eq!(policy(0.0, big, 800, false, 8), 0, "the first tick plans on the calling thread");
        assert_eq!(policy(1.0, big, 800, false, 8), 3, "6.4e5 work: log2 6.4 rounds to 3");
        assert_eq!(policy(10.0, big, 800, false, 8), 6, "6.4e6 work: log2 64");
        assert_eq!(policy(100.0, big, 800, false, 8), 7, "capped at 7 (8 cores, one left)");
        assert_eq!(policy(100.0, big, 800, false, 4), 3, "4 cores: 3 workers");
        assert_eq!(policy(2.0, 200 * 200, 200, false, 8), 0, "8e4 work: log2 0.8 rounds to 0");
        assert_eq!(policy(5.0, 200 * 200, 200, false, 8), 1, "2e5 work: log2 2");
    }

    /// The window policy caps the threads by the side (6, 4, 2) and by one per three plans; a
    /// tick with fewer than three plans, a small map or one core takes none.
    #[test]
    fn the_window_policy_caps_by_side_and_plans() {
        let plans = 30.0;
        assert_eq!(policy(plans, 200 * 200, 200, true, 8), 6);
        assert_eq!(policy(plans, 400 * 400, 400, true, 8), 4);
        assert_eq!(policy(plans, 800 * 800, 800, true, 8), 2);
        assert_eq!(policy(plans, 200 * 200, 200, true, 4), 3, "4 cores: 3 workers");
        assert_eq!(policy(2.0, 800 * 800, 800, true, 8), 0, "fewer than three plans");
        assert_eq!(policy(3.0, 800 * 800, 800, true, 8), 1);
        assert_eq!(policy(8.0, 400 * 400, 400, true, 8), 2, "8 plans give 2, the cap is 4");
        assert_eq!(policy(plans, 800 * 800, 800, true, 1), 0, "one core");
        assert_eq!(policy(plans, MIN_CELLS - 1, 199, true, 8), 0, "a small map");
    }

    /// Whatever the plans, the cells, the side and the switch, the count is at most the cores
    /// left after one and at most [`MAX_WORKERS`], and none on a small map or one core.
    #[test]
    fn the_policy_never_exceeds_the_cores_or_the_pool() {
        for cores in 0..=40 {
            for &cells in &[MIN_CELLS - 1, MIN_CELLS, 640_000, 4_000_000] {
                for &side in &[100u32, 200, 400, 800, 2000] {
                    for fast in [false, true] {
                        for k in 0..=400 {
                            let n = policy(k as f64 / 2.0, cells, side, fast, cores);
                            assert!(n <= cores.saturating_sub(1).min(MAX_WORKERS));
                            if cells < MIN_CELLS || cores < 2 {
                                assert_eq!(n, 0);
                            }
                        }
                    }
                }
            }
        }
    }

    /// An open grass map of `side` cells with every multiplier 1, and the search's inputs.
    fn open(side: i32) -> (TileMap, Vec<u16>, Vec<u16>) {
        let map = TileMap::from_codes(Grid::Square8, side, side, &vec![Surface::GrassPlain as u8; (side * side) as usize], Vec::new());
        let cost = map.land_costs().to_vec();
        let mult = vec![1u16; cost.len()];
        (map, cost, mult)
    }

    /// Both switches off: the flood of the whole map and its descent, as the planner made them
    /// before the switches (`Route::Exact`, the same path).
    #[test]
    fn the_default_search_is_the_whole_map_flood() {
        for k in 0..4u32 {
            let from = ((k as i32 * 61) % 240, (k as i32 * 29 + 5) % 240);
            let p = input(k + 7, from);
            let mut field = p.geom.flood_maps(&p.cost, &p.mult, &p.seeds, from);
            for &t in &p.erase {
                field.erase(t);
            }
            let want = (field.kept > 0).then(|| p.geom.descend(&field, from));
            let (out, route) = compute_routed(&p.geom, &p.cost, &p.mult, &p.seeds, from, &p.erase, p.reach, Search::default());
            assert_eq!(route, Route::Exact);
            assert_eq!(out.path, want);
        }
    }

    /// The window switch plans with the window's flood: its route is `Window`, and every cell of
    /// its path lies inside the window around the army.
    #[test]
    fn the_window_switch_plans_inside_its_box() {
        let p = input(3, (120, 120));
        let search = Search { window: Some(window::RADIUS), direct: false };
        let (out, route) = compute_routed(&p.geom, &p.cost, &p.mult, &p.seeds, p.from, &p.erase, p.reach, search);
        assert_eq!(route, Route::Window);
        let (x0, x1, y0, y1) = window::bounds(240, 240, p.from, window::RADIUS);
        for t in out.path.unwrap_or_default() {
            assert!((x0..=x1).contains(&t.0) && (y0..=y1).contains(&t.1), "{t:?} is outside the window");
        }
    }

    /// The direct switch takes a single target (one cell, seeded twice) by A*: its route is
    /// `Direct` and the path ends on it. Two targets, or an enclosed target, are not direct: the
    /// route falls back (to the window with the window switch, else the whole-map flood).
    #[test]
    fn the_direct_switch_routes_a_single_target_by_a_star() {
        let (map, cost, mult) = open(60);
        let from = (5, 5);
        let target = (40, 33);
        let direct = Search { window: None, direct: true };
        let one = [(target, 3), (target, 9)];
        let (out, route) = compute_routed(&map, &cost, &mult, &one, from, &[], 7, direct);
        assert_eq!(route, Route::Direct);
        assert_eq!(out.path.as_ref().and_then(|p| p.last()), Some(&target));
        assert_eq!(out.reach, 7);
        let two = [(target, 3), ((10, 50), 4)];
        assert_eq!(compute_routed(&map, &cost, &mult, &two, from, &[], 7, direct).1, Route::Exact);
        assert_eq!(compute_routed(&map, &cost, &mult, &two, from, &[], 7, Search { window: Some(64), direct: true }).1, Route::Window);
        // The target walled in by closed cells: no path, so the normal planner makes the plan.
        let mut walled = mult.clone();
        for nb in map.grid.neighbours(target) {
            walled[(nb.1 * 60 + nb.0) as usize] = 0;
        }
        assert_eq!(compute_routed(&map, &cost, &walled, &one, from, &[], 7, direct).1, Route::Exact);
        assert_eq!(compute_routed(&map, &cost, &walled, &one, from, &[], 7, Search { window: Some(64), direct: true }).1, Route::Window);
    }
}
