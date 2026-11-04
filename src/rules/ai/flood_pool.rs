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
//! Threads: `RAZDOR_PLAN_THREADS` (0 turns them off), else the cores left after one, at most
//! [`MAX_WORKERS`]. Off with no threads (wasm), on a small map ([`MIN_CELLS`]), or when a worker
//! has not taken a job when its result is needed, the caller runs it itself (a job is never
//! left waiting for a worker that is busy).

use std::any::Any;
use std::collections::VecDeque;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Condvar, Mutex, OnceLock};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use super::planner_proto::{self, Landmarks};
use crate::rules::map::{Tile, TileMap};

/// The most worker threads the planner starts (the game's others take the rest of the cores).
pub const MAX_WORKERS: usize = 7;
/// Maps below this many cells plan on the calling thread: a flood there costs less than a hand-off.
pub const MIN_CELLS: usize = 40_000;

/// Everything a flood reads: the map's geometry, the planner's cost and multiplier maps, its
/// seeds, its start cell, the cells erased from the flood before the path is read, and the
/// countdown the path resets to.
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
    /// PROTOTYPE: the landmark tables of the ALT modes ([`planner_proto::landmarks_for`]).
    pub lm: Option<Arc<Landmarks>>,
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

/// The flood, the erasures, the read path: the serial planner's work from its inputs. `geom`
/// is the map or its [`TileMap::geometry`]. PROTOTYPE: the path is the planner mode's
/// ([`planner_proto::plan`]); the exact mode is the flood, as before.
pub fn compute_with(geom: &TileMap, cost: &[u16], mult: &[u16], seeds: &[(Tile, u32)], from: Tile, erase: &[Tile], reach: i32, lm: Option<&Landmarks>) -> PlanOutput {
    let path = planner_proto::plan(geom, cost, mult, seeds, from, erase, lm);
    let grid = geom.grid;
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

/// [`compute_with`] on an owned input (a job's).
pub fn compute(input: PlanInput) -> PlanOutput {
    let PlanInput { geom, cost, mult, seeds, from, erase, reach, lm } = input;
    compute_with(&geom, &cost, &mult, &seeds, from, &erase, reach, lm.as_deref())
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

/// Whether a flood of a map of `cells` cells goes to a worker: there are workers and the map is
/// big enough. Otherwise the caller floods it itself, at once ([`compute_with`]).
pub fn offloads(cells: usize) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        cells >= MIN_CELLS && workers() > 0
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

/// The worker threads the planner runs: `RAZDOR_PLAN_THREADS` if set (0 for none), else the
/// cores left free after one, at most [`MAX_WORKERS`]. None on wasm.
pub fn workers() -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        static WORKERS: OnceLock<usize> = OnceLock::new();
        *WORKERS.get_or_init(|| {
            if let Some(n) = std::env::var("RAZDOR_PLAN_THREADS").ok().and_then(|v| v.trim().parse::<usize>().ok()) {
                return n.min(MAX_WORKERS);
            }
            std::thread::available_parallelism().map_or(1, |n| n.get()).saturating_sub(1).min(MAX_WORKERS)
        })
    }
}

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
        self.ready.notify_one();
    }

    fn next(&self) -> Arc<Job> {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(job) = jobs.pop_front() {
                return job;
            }
            jobs = self.ready.wait(jobs).unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// The worker pool, started on first use (None when no thread could be started).
#[cfg(not(target_arch = "wasm32"))]
fn pool() -> Option<&'static Queue> {
    static POOL: OnceLock<Option<&'static Queue>> = OnceLock::new();
    *POOL.get_or_init(|| {
        let n = workers();
        if n == 0 {
            return None;
        }
        // Leaked once: the workers live as long as the process.
        let queue: &'static Queue = Box::leak(Box::new(Queue { jobs: Mutex::new(VecDeque::new()), ready: Condvar::new() }));
        let started = (0..n)
            .filter(|k| {
                std::thread::Builder::new()
                    .name(format!("razdor-planner-{k}"))
                    .spawn(move || loop {
                        queue.next().run();
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
        PlanInput { geom: map.geometry(), cost, mult, seeds, from, erase, reach: 7, lm: None }
    }

    fn same(a: &PlanOutput, b: &PlanOutput) -> bool {
        a.path == b.path && a.weights == b.weights && a.reach == b.reach && a.facing == b.facing
    }

    /// Floods handed to the pool come back as the calling thread computes them, whichever
    /// order they are taken in, and a job that no worker took is run by its caller.
    #[test]
    fn pooled_floods_are_the_inline_ones_in_any_order() {
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
}
