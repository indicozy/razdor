//! PROTOTYPE (branch `proto/planner-c`, not for `main`): approximate planners selected by env
//! vars, to measure speed, memory and how far they move the AI from the exact planner. With
//! nothing set (`RAZDOR_PLANNER` unset or `exact`, no `RAZDOR_PLANNER_LOD`, no shadow) every
//! hook here is the exact planner and the AI is unchanged.
//!
//! - `RAZDOR_PLANNER=exact|window[:N]|astar|astar-alt|astar-dist|astar-dist-alt|astar-distmult`
//!   (`RAZDOR_PLANNER_R` is the window radius, 48; `RAZDOR_PLANNER_L` the landmark count, 16).
//!   - `astar`: target by seed value + octile x average step (planner-a), octile A*.
//!   - `astar-alt`: target by seed value + the lower bound max(octile, landmarks); A* with the
//!     landmark (ALT) heuristic.
//!   - `astar-dist`: target by seed value + the exact static distance (a Dijkstra from the army
//!     over the static cost map, stopped once no candidate can win); octile A*.
//!   - `astar-dist-alt`: as `astar-dist`, with the ALT heuristic.
//!   - `astar-distmult`: an oracle: the target by seed value + the real distance (multipliers
//!     included), octile A*. Used by `RAZDOR_PLANNER_DIAG` to measure what the static choice loses.
//! - `RAZDOR_PLANNER_LOD=off|countdown|keep|both`, `RAZDOR_PLANNER_LOD_D` (30),
//!   `RAZDOR_PLANNER_LOD_K` (3): armies beyond D cells (octile) from the hero.
//! - `RAZDOR_PLANNER_SHADOW=window:32,astar-alt,...`: at every plan, also compute these modes and
//!   tally how they agree with the exact plan (the game still follows the active mode).
//! - `RAZDOR_PLANNER_DIAG=1`: at every target plan, tally how often the static choice differs
//!   from the multiplier-aware choice (`astar-distmult`).
//!
//! The static distances of the landmark modes are the cost map's, without the per-army
//! multipliers (obstacles, cones): the multipliers apply in the A* search only. Every
//! lower bound used here is admissible for the search's metric (entering costs, step weights).
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::rules::map::{hex_distance, Grid, Tile, TileMap, DIAGONAL_WEIGHT, ORTHOGONAL_WEIGHT};

/// Unreached in a distance field.
const INF: u32 = u32::MAX;

/// The flood the planner runs (or the target and search of an A* mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The original's planner, as shipped: one flood from every seed.
    Exact,
    /// A square window of this radius around the army; outside seeds moved to its border.
    Window(i32),
    /// Target by octile estimate, octile A* (planner-a).
    AStar,
    /// Target by the landmark lower bound, ALT A*.
    Alt,
    /// Target by the exact static distance, octile A*.
    Dist,
    /// Target by the exact static distance, ALT A*.
    DistAlt,
    /// Oracle: target by the real (multiplier-aware) distance, octile A*.
    DistMult,
}

/// How an A* mode picks its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    /// Seed value + octile x average step.
    Octile,
    /// Seed value + max(octile, landmark) lower bound.
    Bound,
    /// Seed value + static distance (early-stop Dijkstra on the static costs).
    Static,
    /// Seed value + real distance (early-stop Dijkstra with the multipliers).
    Mult,
}

/// The A* heuristic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Heur {
    Octile,
    Alt,
}

impl Mode {
    /// Whether this mode reads the landmark tables.
    fn uses_landmarks(self) -> bool {
        matches!(self, Mode::Alt | Mode::DistAlt)
    }
}

/// Distance-based AI detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lod {
    Off,
    /// The re-plan countdown is multiplied by K for armies beyond D from the hero.
    Countdown,
    /// Armies beyond D keep walking their path when a party other than the hero is in reach.
    Keep,
    Both,
}

#[derive(Clone, Debug)]
pub struct Cfg {
    pub mode: Mode,
    pub lod: Lod,
    pub lod_d: i32,
    pub lod_k: i32,
    pub shadow: Vec<Mode>,
    /// Landmarks per cost map (ALT modes).
    pub landmarks: usize,
    /// Tally the static-vs-real target choice at every target plan.
    pub diag: bool,
    /// The distance choice by the plain early-stop Dijkstra (`RAZDOR_PLANNER_CHOICE=dijkstra`)
    /// instead of the multi-candidate A* ([`nearest_candidate_astar`], the default).
    pub choice_dijkstra: bool,
}

/// The prototype's settings, read once from the environment.
pub fn cfg() -> &'static Cfg {
    static CFG: OnceLock<Cfg> = OnceLock::new();
    CFG.get_or_init(from_env)
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty())
}

fn parse_mode(s: &str, r: i32) -> Option<Mode> {
    match s {
        "exact" => Some(Mode::Exact),
        "window" => Some(Mode::Window(r)),
        "astar" => Some(Mode::AStar),
        "astar-alt" => Some(Mode::Alt),
        "astar-dist" => Some(Mode::Dist),
        "astar-dist-alt" => Some(Mode::DistAlt),
        "astar-distmult" => Some(Mode::DistMult),
        _ => s.strip_prefix("window:").and_then(|n| n.parse().ok()).map(Mode::Window),
    }
}

fn from_env() -> Cfg {
    let r = env("RAZDOR_PLANNER_R").and_then(|s| s.parse().ok()).unwrap_or(48);
    let mode = env("RAZDOR_PLANNER").and_then(|s| parse_mode(&s, r)).unwrap_or(Mode::Exact);
    let lod = match env("RAZDOR_PLANNER_LOD").as_deref() {
        Some("countdown") => Lod::Countdown,
        Some("keep") => Lod::Keep,
        Some("both") => Lod::Both,
        _ => Lod::Off,
    };
    let lod_d = env("RAZDOR_PLANNER_LOD_D").and_then(|s| s.parse().ok()).unwrap_or(30);
    let lod_k = env("RAZDOR_PLANNER_LOD_K").and_then(|s| s.parse().ok()).unwrap_or(3);
    let shadow = env("RAZDOR_PLANNER_SHADOW").map(|s| s.split(',').filter_map(|m| parse_mode(m.trim(), r)).collect()).unwrap_or_default();
    let landmarks = env("RAZDOR_PLANNER_L").and_then(|s| s.parse().ok()).unwrap_or(16);
    let diag = env("RAZDOR_PLANNER_DIAG").is_some_and(|s| s != "0");
    let choice_dijkstra = env("RAZDOR_PLANNER_CHOICE").is_some_and(|s| s == "dijkstra");
    Cfg { mode, lod, lod_d, lod_k, shadow, landmarks, diag, choice_dijkstra }
}

/// Whether any mode in use (active or shadow) reads the landmark tables.
pub fn needs_landmarks() -> bool {
    let c = cfg();
    c.mode.uses_landmarks() || c.shadow.iter().any(|m| m.uses_landmarks())
}

/// Agreement of one shadow mode with the exact plan, over the plans made.
#[derive(Clone, Debug, Default)]
pub struct Agreement {
    pub plans: u64,
    pub same_first: u64,
    pub same_target: u64,
    pub ratio_sum: f64,
    pub ratio_n: u64,
    pub alt_empty: u64,
    pub exact_empty: u64,
}

/// What the prototype counted since the last [`take_tally`].
#[derive(Clone, Debug, Default)]
pub struct Tally {
    pub shadow: Vec<Agreement>,
    /// Steps taken by AI armies (a step that moved an army).
    pub steps: u64,
    /// Party triggers of a re-plan that the LOD rule suppressed.
    pub lod_skips: u64,
    /// Target plans with [`Cfg::diag`] on, and those whose static choice differs from the
    /// multiplier-aware one.
    pub diag_plans: u64,
    pub diag_mismatch: u64,
}

impl Tally {
    /// Adds `o` to `self` (the ignored free run sums its maps).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn add(&mut self, o: &Tally) {
        if self.shadow.len() < o.shadow.len() {
            self.shadow.resize(o.shadow.len(), Agreement::default());
        }
        for (s, t) in self.shadow.iter_mut().zip(&o.shadow) {
            s.plans += t.plans;
            s.same_first += t.same_first;
            s.same_target += t.same_target;
            s.ratio_sum += t.ratio_sum;
            s.ratio_n += t.ratio_n;
            s.alt_empty += t.alt_empty;
            s.exact_empty += t.exact_empty;
        }
        self.steps += o.steps;
        self.lod_skips += o.lod_skips;
        self.diag_plans += o.diag_plans;
        self.diag_mismatch += o.diag_mismatch;
    }
}

/// Global (not thread-local): floods run on the planner's worker threads too.
static TALLY: Mutex<Tally> = Mutex::new(Tally { shadow: Vec::new(), steps: 0, lod_skips: 0, diag_plans: 0, diag_mismatch: 0 });

fn with_tally<R>(f: impl FnOnce(&mut Tally) -> R) -> R {
    f(&mut TALLY.lock().unwrap_or_else(|e| e.into_inner()))
}

/// One AI army step.
pub fn count_step() {
    with_tally(|t| t.steps += 1);
}

/// One party trigger of a re-plan that the LOD rule suppressed.
pub fn count_lod_skip() {
    with_tally(|t| t.lod_skips += 1);
}

/// The tally so far, which starts again from zero (read by the ignored free run).
#[cfg_attr(not(test), allow(dead_code))]
pub fn take_tally() -> Tally {
    with_tally(std::mem::take)
}

fn record_shadow(k: usize, exact: &Option<Vec<Tile>>, alt: &Option<Vec<Tile>>) {
    with_tally(|tally| {
        if tally.shadow.len() <= k {
            tally.shadow.resize(k + 1, Agreement::default());
        }
        let s = &mut tally.shadow[k];
        let e = exact.as_deref().unwrap_or(&[]);
        let a = alt.as_deref().unwrap_or(&[]);
        s.plans += 1;
        if e.first() == a.first() {
            s.same_first += 1;
        }
        if e.last() == a.last() {
            s.same_target += 1;
        }
        if e.is_empty() {
            s.exact_empty += 1;
        }
        if a.is_empty() {
            s.alt_empty += 1;
        }
        if !e.is_empty() && !a.is_empty() {
            s.ratio_sum += a.len() as f64 / e.len() as f64;
            s.ratio_n += 1;
        }
    });
}

/// The landmark tables of one cost map: for each landmark, the distance field from it (`fwd`:
/// the cost of walking from the landmark to the cell) and to it (`back`: from the cell to the
/// landmark), both in the search's metric (entering cost × step weight), [`INF`] where unreached.
#[derive(Debug)]
pub struct Landmarks {
    pub cells: Vec<usize>,
    pub fwd: Vec<Vec<u32>>,
    pub back: Vec<Vec<u32>>,
}

impl Landmarks {
    /// Bytes of the tables (u32 per cell per landmark per direction).
    pub fn bytes(&self) -> usize {
        self.fwd.iter().chain(&self.back).map(|v| v.len() * 4).sum()
    }

    /// Landmarks for `costs` (the static cost map of `map`): the first passable cell, then
    /// farthest-point selection on the round-trip distance to the chosen ones.
    pub fn build(map: &TileMap, costs: &[u16], count: usize) -> Landmarks {
        let mut lm = Landmarks { cells: Vec::new(), fwd: Vec::new(), back: Vec::new() };
        let n = costs.len();
        let Some(first) = costs.iter().position(|&c| c > 0) else { return lm };
        let rt = |src: usize| -> Vec<u32> {
            let f = sssp(map, costs, src, false);
            let b = sssp(map, costs, src, true);
            f.iter().zip(&b).map(|(&x, &y)| sat_add(x, y)).collect()
        };
        let mut score = rt(first);
        for _ in 0..count {
            // The farthest passable cell from the chosen landmarks (ties: the first cell).
            let next = (0..n).filter(|&i| costs[i] > 0 && score[i] != INF).max_by_key(|&i| (score[i], Reverse(i)));
            let Some(c) = next else { break };
            let f = sssp(map, costs, c, false);
            let b = sssp(map, costs, c, true);
            let rtc: Vec<u32> = f.iter().zip(&b).map(|(&x, &y)| sat_add(x, y)).collect();
            if lm.cells.is_empty() {
                score = rtc;
            } else {
                for (s, r) in score.iter_mut().zip(&rtc) {
                    *s = (*s).min(*r);
                }
            }
            lm.cells.push(c);
            lm.fwd.push(f);
            lm.back.push(b);
        }
        lm
    }
}

fn sat_add(a: u32, b: u32) -> u32 {
    if a == INF || b == INF {
        INF
    } else {
        a.saturating_add(b)
    }
}

fn idx(map: &TileMap, t: Tile) -> Option<usize> {
    (t.0 >= 0 && t.1 >= 0 && t.0 < map.w && t.1 < map.h).then(|| (t.1 * map.w + t.0) as usize)
}

fn tile_of(map: &TileMap, i: usize) -> Tile {
    (i as i32 % map.w, i as i32 / map.w)
}

/// Full Dijkstra from `src` over `costs` (0 = impassable). Forward: `d[j]` is the cost of
/// walking from `src` to `j`, every cell entered charged at its cost times the step weight.
/// Backward: `d[j]` is the cost of walking from `j` to `src` (the same charges, along that walk).
fn sssp(map: &TileMap, costs: &[u16], src: usize, backward: bool) -> Vec<u32> {
    let mut dist = vec![INF; costs.len()];
    dist[src] = 0;
    let mut heap = BinaryHeap::from([Reverse((0u32, src as u32))]);
    while let Some(Reverse((d, i))) = heap.pop() {
        let i = i as usize;
        if d > dist[i] {
            continue;
        }
        let here = tile_of(map, i);
        for nb in map.grid.neighbours(here) {
            let Some(j) = idx(map, nb) else { continue };
            if costs[j] == 0 {
                continue;
            }
            let w = map.grid.weight(here, nb);
            let nd = if backward { d.saturating_add(costs[i] as u32 * w) } else { d.saturating_add(costs[j] as u32 * w) };
            if nd < dist[j] {
                dist[j] = nd;
                heap.push(Reverse((nd, j as u32)));
            }
        }
    }
    dist
}

/// ALT lower bound on the cost of walking from `x` to `t`: for each landmark, the triangle
/// inequality both ways (from it, and to it). 0 when no landmark bounds it.
fn alt_bound(lm: &Landmarks, x: usize, t: usize) -> u32 {
    let mut best = 0;
    for (f, b) in lm.fwd.iter().zip(&lm.back) {
        let (fx, ft) = (f[x], f[t]);
        if fx != INF && ft != INF && ft > fx {
            best = best.max(ft - fx);
        }
        let (bx, bt) = (b[x], b[t]);
        if bx != INF && bt != INF && bx > bt {
            best = best.max(bx - bt);
        }
    }
    best
}

/// Lower bound of the step weights between two cells (octile in weight units, as the A* uses).
fn weight_bound(grid: Grid, a: Tile, b: Tile) -> u32 {
    match grid {
        Grid::HexOddR => ORTHOGONAL_WEIGHT * hex_distance(a, b) as u32,
        Grid::Square8 => {
            let (dx, dy) = ((a.0 - b.0).unsigned_abs(), (a.1 - b.1).unsigned_abs());
            ORTHOGONAL_WEIGHT * dx.max(dy) + (DIAGONAL_WEIGHT - ORTHOGONAL_WEIGHT) * dx.min(dy)
        }
    }
}

/// The landmark tables for the cost map `sails` picks, built on first use of the map and dropped
/// when a cell's cost changes ([`TileMap::planner_landmarks`]). None when no mode reads them.
pub fn landmarks_for(map: &TileMap, sails: bool) -> Option<Arc<Landmarks>> {
    if !needs_landmarks() {
        return None;
    }
    let count = cfg().landmarks;
    Some(map.planner_landmarks(sails, || {
        let costs = if sails { map.water_costs() } else { map.land_costs() };
        Arc::new(Landmarks::build(map, costs, count))
    }))
}

/// Thread-local search buffers, with an epoch so nothing is cleared per search.
#[derive(Default)]
struct Scratch {
    dist: Vec<u32>,
    parent: Vec<u32>,
    stamp: Vec<u32>,
    epoch: u32,
}

thread_local! {
    static SCRATCH: RefCell<Scratch> = RefCell::new(Scratch::default());
}

impl Scratch {
    fn begin(&mut self, n: usize) {
        if self.dist.len() < n {
            self.dist.resize(n, INF);
            self.parent.resize(n, INF);
            self.stamp.resize(n, 0);
        }
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.epoch = 1;
        }
    }

    fn get(&self, j: usize) -> u32 {
        if self.stamp[j] == self.epoch {
            self.dist[j]
        } else {
            INF
        }
    }

    fn set(&mut self, j: usize, d: u32, parent: u32) {
        self.stamp[j] = self.epoch;
        self.dist[j] = d;
        self.parent[j] = parent;
    }
}

/// The candidate targets: the seeds on cells the army may aim at (passable by the static cost,
/// not its own cell, not erased), the least value per cell, sorted by cell.
fn candidates(map: &TileMap, costs: &[u16], seeds: &[(Tile, u32)], here: Tile, erased: &[Tile]) -> Vec<(usize, u32)> {
    let here_i = idx(map, here);
    let mut v: Vec<(usize, u32)> = seeds
        .iter()
        .filter_map(|&(t, val)| {
            let j = idx(map, t)?;
            (Some(j) != here_i && costs[j] != 0 && !erased.contains(&t)).then_some((j, val))
        })
        .collect();
    v.sort_unstable();
    v.dedup_by_key(|e| e.0);
    v
}

fn cand_value(cands: &[(usize, u32)], j: usize) -> Option<u32> {
    cands.binary_search_by_key(&j, |e| e.0).ok().map(|k| cands[k].1)
}

/// The step cost of entering `j` in the search: its cost times its multiplier (`mult`, or the
/// static cost without one), 0 when closed. A closed cell is enterable only when `closed_ok`
/// (a seed on a closed cell is a target: the flood expands from it).
fn entering(costs: &[u16], mult: Option<&[u16]>, j: usize) -> u32 {
    match mult {
        Some(m) => costs[j].wrapping_mul(m[j]) as u32,
        None => costs[j] as u32,
    }
}

/// The early-stop Dijkstra of the static (or multiplier-aware) distance from `from` to each
/// candidate: the candidate with the least value + distance. A popped distance at or above the
/// best total ends the search: no candidate not yet popped can win.
fn nearest_candidate(map: &TileMap, costs: &[u16], mult: Option<&[u16]>, erased: &[Tile], from: Tile, cands: &[(usize, u32)]) -> Option<usize> {
    let start = idx(map, from)?;
    let n = costs.len();
    SCRATCH.with(|s| {
        let s = &mut *s.borrow_mut();
        s.begin(n);
        s.set(start, 0, INF);
        let mut heap = BinaryHeap::from([Reverse((0u32, start as u32))]);
        let mut best: Option<(u32, usize)> = None;
        while let Some(Reverse((d, i))) = heap.pop() {
            let i = i as usize;
            if d > s.get(i) {
                continue;
            }
            if best.is_some_and(|(b, _)| d >= b) {
                break;
            }
            if i != start {
                if let Some(v) = cand_value(cands, i) {
                    let total = v.saturating_add(d);
                    if best.is_none_or(|(b, _)| total < b) {
                        best = Some((total, i));
                    }
                }
            }
            let here = tile_of(map, i);
            for nb in map.grid.neighbours(here) {
                let Some(j) = idx(map, nb) else { continue };
                if erased.contains(&nb) {
                    continue;
                }
                let e = match entering(costs, mult, j) {
                    0 if cand_value(cands, j).is_some() => 1,
                    0 => continue,
                    e => e,
                };
                let nd = d.saturating_add(e * map.grid.weight(here, nb));
                if nd < s.get(j) {
                    s.set(j, nd, i as u32);
                    heap.push(Reverse((nd, j as u32)));
                }
            }
        }
        best.map(|(_, j)| j)
    })
}

/// The same choice as [`nearest_candidate`] (the least value + distance, the same exact stop), by
/// one A* over all the candidates at once: a cell's heuristic is the least, over the candidates,
/// of value + a lower bound of the walk from the cell to it (the landmark bound with `lm`, else
/// the octile one). Each bound is consistent, so the heuristic is too; a cell's f is then a lower
/// bound of every total through it, and the search ends when the least f reaches the best total.
fn nearest_candidate_astar(map: &TileMap, costs: &[u16], mult: Option<&[u16]>, erased: &[Tile], from: Tile, cands: &[(usize, u32)], lm: Option<&Landmarks>) -> Option<usize> {
    let start = idx(map, from)?;
    let n = costs.len();
    let grid = map.grid;
    let h = |x: usize| -> u32 {
        let xt = tile_of(map, x);
        let mut least = INF;
        for &(c, v) in cands {
            let mut lb = weight_bound(grid, xt, tile_of(map, c));
            if let Some(lm) = lm {
                lb = lb.max(alt_bound(lm, x, c));
            }
            least = least.min(v.saturating_add(lb));
        }
        least
    };
    SCRATCH.with(|s| {
        let s = &mut *s.borrow_mut();
        s.begin(n);
        s.set(start, 0, INF);
        let mut open = BinaryHeap::from([Reverse((h(start), 0u32, start as u32))]);
        let mut best: Option<(u32, usize)> = None;
        while let Some(Reverse((f, g, i))) = open.pop() {
            let i = i as usize;
            if g > s.get(i) {
                continue;
            }
            if best.is_some_and(|(b, _)| f >= b) {
                break;
            }
            if i != start {
                if let Some(v) = cand_value(cands, i) {
                    let total = v.saturating_add(g);
                    if best.is_none_or(|(b, _)| total < b) {
                        best = Some((total, i));
                    }
                }
            }
            let here = tile_of(map, i);
            for nb in grid.neighbours(here) {
                let Some(j) = idx(map, nb) else { continue };
                if erased.contains(&nb) {
                    continue;
                }
                let e = match entering(costs, mult, j) {
                    0 if cand_value(cands, j).is_some() => 1,
                    0 => continue,
                    e => e,
                };
                let ng = g.saturating_add(e * grid.weight(here, nb));
                if ng < s.get(j) {
                    s.set(j, ng, i as u32);
                    open.push(Reverse((ng.saturating_add(h(j)), ng, j as u32)));
                }
            }
        }
        best.map(|(_, j)| j)
    })
}

/// A* from `from` to the first cell `goal` accepts, the step costs as [`entering`] (a closed
/// cell is entered only when `closed_ok`, at cost 1), `h` a lower bound on the rest of the walk.
/// Empty when none is reached.
fn astar(map: &TileMap, costs: &[u16], mult: &[u16], erased: &[Tile], from: Tile, goal: &dyn Fn(usize) -> bool, closed_ok: &dyn Fn(usize) -> bool, h: &dyn Fn(usize) -> u32) -> Vec<Tile> {
    let Some(start) = idx(map, from) else { return Vec::new() };
    let n = costs.len();
    SCRATCH.with(|s| {
        let s = &mut *s.borrow_mut();
        s.begin(n);
        s.set(start, 0, INF);
        let mut open = BinaryHeap::from([Reverse((h(start), 0u32, start as u32))]);
        while let Some(Reverse((_, g, i))) = open.pop() {
            let i = i as usize;
            if g > s.get(i) {
                continue;
            }
            if i != start && goal(i) {
                let mut path = Vec::new();
                let mut cur = i;
                while cur != start {
                    path.push(tile_of(map, cur));
                    cur = s.parent[cur] as usize;
                }
                path.reverse();
                return path;
            }
            let here = tile_of(map, i);
            for nb in map.grid.neighbours(here) {
                let Some(j) = idx(map, nb) else { continue };
                if erased.contains(&nb) {
                    continue;
                }
                let e = match entering(costs, Some(mult), j) {
                    0 if closed_ok(j) => 1,
                    0 => continue,
                    e => e,
                };
                let ng = g.saturating_add(e * map.grid.weight(here, nb));
                if ng < s.get(j) {
                    s.set(j, ng, i as u32);
                    open.push(Reverse((ng.saturating_add(h(j)), ng, j as u32)));
                }
            }
        }
        Vec::new()
    })
}

/// The target the choice picks: the candidate with the least value + estimate.
fn choose(choice: Choice, map: &TileMap, costs: &[u16], mult: &[u16], seeds: &[(Tile, u32)], here: Tile, erased: &[Tile], lm: Option<&Landmarks>) -> Option<usize> {
    let cands = candidates(map, costs, seeds, here, erased);
    let here_i = idx(map, here)?;
    match choice {
        Choice::Octile | Choice::Bound => {
            let avg = average_step(costs) as u64;
            let mut best: Option<(u64, usize)> = None;
            for &(j, v) in &cands {
                let t = tile_of(map, j);
                let oct = map.grid.octile(here, t) as u64;
                let lb = match (choice, lm) {
                    (Choice::Bound, Some(lm)) => (weight_bound(map.grid, here, t) as u64).max(alt_bound(lm, here_i, j) as u64),
                    _ => oct * avg,
                };
                let est = v as u64 + lb;
                if best.is_none_or(|(e, _)| est < e) {
                    best = Some((est, j));
                }
            }
            best.map(|(_, j)| j)
        }
        Choice::Static if cfg().choice_dijkstra => nearest_candidate(map, costs, None, erased, here, &cands),
        Choice::Static => nearest_candidate_astar(map, costs, None, erased, here, &cands, lm),
        Choice::Mult if cfg().choice_dijkstra => nearest_candidate(map, costs, Some(mult), erased, here, &cands),
        Choice::Mult => nearest_candidate_astar(map, costs, Some(mult), erased, here, &cands, None),
    }
}

/// The path to the chosen target, and when it is cut off, to the nearest candidate (Dijkstra).
fn path_to(map: &TileMap, costs: &[u16], mult: &[u16], seeds: &[(Tile, u32)], here: Tile, erased: &[Tile], target: usize, heur: Heur, lm: Option<&Landmarks>) -> Vec<Tile> {
    let grid = map.grid;
    let t = tile_of(map, target);
    let h = |x: usize| -> u32 {
        let xt = tile_of(map, x);
        let w = weight_bound(grid, xt, t);
        match (heur, lm) {
            (Heur::Alt, Some(lm)) => w.max(alt_bound(lm, x, target)),
            _ => w,
        }
    };
    let p = astar(map, costs, mult, erased, here, &|j| j == target, &|j| j == target, &h);
    if !p.is_empty() {
        return p;
    }
    let cands = candidates(map, costs, seeds, here, erased);
    let is_cand = |j: usize| cand_value(&cands, j).is_some();
    astar(map, costs, mult, erased, here, &is_cand, &is_cand, &|_| 0)
}

/// The mode's plan: the path (`Some` with the path, empty: stand; `None` with no seed kept) and
/// the target an A* mode aimed at.
fn run(mode: Mode, map: &TileMap, costs: &[u16], mult: &[u16], seeds: &[(Tile, u32)], here: Tile, erased: &[Tile], lm: Option<&Landmarks>) -> (Option<Vec<Tile>>, Option<usize>) {
    let (choice, heur) = match mode {
        Mode::Exact => return (exact_path(map, costs, mult, seeds, here, erased), None),
        Mode::Window(r) => return (window_path(map, costs, mult, seeds, here, erased, r), None),
        Mode::AStar => (Choice::Octile, Heur::Octile),
        Mode::Alt => (Choice::Bound, Heur::Alt),
        Mode::Dist => (Choice::Static, Heur::Octile),
        Mode::DistAlt => (Choice::Static, Heur::Alt),
        Mode::DistMult => (Choice::Mult, Heur::Octile),
    };
    // Without the tables an ALT mode is the octile one (never reached: `landmarks_for` builds them).
    let (choice, heur) = match lm {
        Some(_) => (choice, heur),
        None => (if choice == Choice::Bound { Choice::Octile } else { choice }, if heur == Heur::Alt { Heur::Octile } else { heur }),
    };
    let lm_choice = if heur == Heur::Alt { lm } else { None };
    let Some(target) = choose(choice, map, costs, mult, seeds, here, erased, lm_choice) else { return (None, None) };
    if erased.contains(&here) {
        return (Some(Vec::new()), Some(target));
    }
    let p = path_to(map, costs, mult, seeds, here, erased, target, heur, lm);
    ((!p.is_empty()).then_some(p), Some(target))
}

/// The planner's path for one plan (the inputs of `Game::ai_plan` after its seeds): `Some`
/// with the path (empty: stand), `None` with no seed kept. `erased` are the cells of the
/// parties in reach and the cells they step to, which the path never steps onto. `lm`: the
/// landmark tables of this cost map when a mode reads them ([`landmarks_for`]).
pub fn plan(map: &TileMap, costs: &[u16], mult: &[u16], seeds: &[(Tile, u32)], here: Tile, erased: &[Tile], lm: Option<&Landmarks>) -> Option<Vec<Tile>> {
    let c = cfg();
    let (path, target) = run(c.mode, map, costs, mult, seeds, here, erased, lm);
    if c.diag && matches!(c.mode, Mode::AStar | Mode::Alt | Mode::Dist | Mode::DistAlt) {
        if let Some(t) = target {
            let oracle = choose(Choice::Mult, map, costs, mult, seeds, here, erased, lm);
            with_tally(|tl| {
                tl.diag_plans += 1;
                if oracle != Some(t) {
                    tl.diag_mismatch += 1;
                }
            });
        }
    }
    if !c.shadow.is_empty() {
        let exact = if c.mode == Mode::Exact { path.clone() } else { exact_path(map, costs, mult, seeds, here, erased) };
        for (k, &m) in c.shadow.iter().enumerate() {
            let alt = run(m, map, costs, mult, seeds, here, erased, lm).0;
            record_shadow(k, &exact, &alt);
        }
    }
    path
}

/// The original's planner: one flood from all seeds, the erased cells, the descent.
pub fn exact_path(map: &TileMap, costs: &[u16], mult: &[u16], seeds: &[(Tile, u32)], here: Tile, erased: &[Tile]) -> Option<Vec<Tile>> {
    let mut field = map.flood_maps(costs, mult, seeds, here);
    for &t in erased {
        field.erase(t);
    }
    (field.kept > 0).then(|| map.descend(&field, here))
}

/// The median passable cost of the map (a stride sample of at most about 4096 cells), times the
/// planner's orthogonal step weight. The median ignores the impassable cells and resists the
/// swamps and forests that a mean would be dragged up by. One cell of octile distance costs about
/// this much to walk: a diagonal step is 1.5 cells and weighs 3, an orthogonal one weighs 2.
pub fn average_step(costs: &[u16]) -> u32 {
    let stride = (costs.len() / 4096).max(1);
    let mut v: Vec<u16> = costs.iter().step_by(stride).copied().filter(|&c| c > 0).collect();
    if v.is_empty() {
        return ORTHOGONAL_WEIGHT;
    }
    v.sort_unstable();
    ORTHOGONAL_WEIGHT * v[v.len() / 2] as u32
}

/// Window of radius `r` around `here` (clipped to the map): the flood is closed outside it,
/// every seed outside is moved to the window cell nearest to it (the clamp to the window's box,
/// or the nearest open border cell when that one is impassable) and valued at its value plus
/// its octile distance to the seed times [`average_step`].
pub fn window_path(map: &TileMap, costs: &[u16], mult: &[u16], seeds: &[(Tile, u32)], here: Tile, erased: &[Tile], r: i32) -> Option<Vec<Tile>> {
    let (w, h) = (map.w, map.h);
    let (x0, x1) = ((here.0 - r).max(0), (here.0 + r).min(w - 1));
    let (y0, y1) = ((here.1 - r).max(0), (here.1 + r).min(h - 1));
    let inside = |t: Tile| t.0 >= x0 && t.0 <= x1 && t.1 >= y0 && t.1 <= y1;
    let mut m = mult.to_vec();
    for y in 0..h {
        for x in 0..w {
            if !inside((x, y)) {
                m[(y * w + x) as usize] = 0;
            }
        }
    }
    let avg = average_step(costs);
    let mut out: Vec<(Tile, u32)> = Vec::with_capacity(seeds.len());
    for &(t, v) in seeds {
        if idx(map, t).is_none() {
            continue;
        }
        if inside(t) {
            out.push((t, v));
            continue;
        }
        let mut b = (t.0.clamp(x0, x1), t.1.clamp(y0, y1));
        if idx(map, b).is_some_and(|j| costs[j] == 0) {
            match nearest_open_border(map, costs, t, (x0, x1, y0, y1)) {
                Some(nb) => b = nb,
                None => continue,
            }
        }
        let est = v + map.grid.octile(b, t) as u32 * avg;
        out.push((b, est));
    }
    let mut field = map.flood_maps(costs, &m, &out, here);
    for &t in erased {
        field.erase(t);
    }
    (field.kept > 0).then(|| map.descend(&field, here))
}

/// The open (passable) window border cell nearest to `t` by octile distance.
fn nearest_open_border(map: &TileMap, costs: &[u16], t: Tile, (x0, x1, y0, y1): (i32, i32, i32, i32)) -> Option<Tile> {
    let mut best: Option<(i32, Tile)> = None;
    let ring = (x0..=x1).flat_map(|x| [(x, y0), (x, y1)]).chain((y0..=y1).flat_map(|y| [(x0, y), (x1, y)]));
    for b in ring {
        if idx(map, b).is_some_and(|j| costs[j] > 0) {
            let d = map.grid.octile(b, t);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, b));
            }
        }
    }
    best.map(|(_, b)| b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::Surface;
    use crate::rules::map::Grid;

    #[test]
    fn mode_names_parse() {
        assert_eq!(parse_mode("exact", 48), Some(Mode::Exact));
        assert_eq!(parse_mode("window", 48), Some(Mode::Window(48)));
        assert_eq!(parse_mode("window:32", 48), Some(Mode::Window(32)));
        assert_eq!(parse_mode("astar", 48), Some(Mode::AStar));
        assert_eq!(parse_mode("astar-alt", 48), Some(Mode::Alt));
        assert_eq!(parse_mode("astar-dist", 48), Some(Mode::Dist));
        assert_eq!(parse_mode("astar-dist-alt", 48), Some(Mode::DistAlt));
        assert_eq!(parse_mode("astar-distmult", 48), Some(Mode::DistMult));
        assert_eq!(parse_mode("nope", 48), None);
    }

    /// A walled grid with a lake in the middle: distances from landmarks bound real distances.
    fn walled(seed: u32) -> TileMap {
        let (w, h) = (60i32, 40i32);
        let mut codes = vec![Surface::GrassPlain as u8; (w * h) as usize];
        let mut state = seed;
        for c in codes.iter_mut() {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            match (state >> 8) % 9 {
                0 => *c = Surface::ImpassableSwamp as u8,
                1 => *c = Surface::Marsh as u8,
                _ => {}
            }
        }
        TileMap::from_codes(Grid::Square8, w, h, &codes, Vec::new())
    }

    /// The landmark bound never exceeds the real static distance (the search's metric).
    #[test]
    fn alt_bound_is_admissible() {
        let map = walled(7);
        let costs = map.land_costs().to_vec();
        let lm = Landmarks::build(&map, &costs, 6);
        assert_eq!(lm.cells.len(), 6);
        let n = costs.len();
        for &(a, b) in &[(0usize, n - 1), (5, n / 2), (n / 3, 17), (101, 900)] {
            if costs[a] == 0 || costs[b] == 0 {
                continue;
            }
            let real = sssp(&map, &costs, a, false)[b];
            if real == INF {
                continue;
            }
            assert!(alt_bound(&lm, a, b) <= real, "bound above the real distance");
        }
    }

    /// Full Dijkstra from `from` in the choice's metric (static, or with `mult`), erased cells
    /// not entered, closed candidates entered at cost 1: the distance to every cell.
    fn full_dist(map: &TileMap, costs: &[u16], mult: Option<&[u16]>, erased: &[Tile], from: Tile, cands: &[(usize, u32)]) -> Vec<u32> {
        let n = costs.len();
        let mut dist = vec![INF; n];
        let start = idx(map, from).unwrap();
        dist[start] = 0;
        let mut heap = BinaryHeap::from([Reverse((0u32, start as u32))]);
        while let Some(Reverse((d, i))) = heap.pop() {
            let i = i as usize;
            if d > dist[i] {
                continue;
            }
            let here = tile_of(map, i);
            for nb in map.grid.neighbours(here) {
                let Some(j) = idx(map, nb) else { continue };
                if erased.contains(&nb) {
                    continue;
                }
                let e = match entering(costs, mult, j) {
                    0 if cand_value(cands, j).is_some() => 1,
                    0 => continue,
                    e => e,
                };
                let nd = d + e * map.grid.weight(here, nb);
                if nd < dist[j] {
                    dist[j] = nd;
                    heap.push(Reverse((nd, j as u32)));
                }
            }
        }
        dist
    }

    /// The multi-candidate A* picks a candidate with the same total as the early-stop Dijkstra,
    /// on static and multiplier-aware costs, with and without the landmark bound.
    #[test]
    fn astar_choice_matches_dijkstra_choice() {
        for seed in 0..6u32 {
            let map = walled(100 + seed);
            let costs = map.land_costs().to_vec();
            let mut state = seed.wrapping_mul(7919) + 1;
            let mut next = |m: u32| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) % m
            };
            let mult: Vec<u16> = (0..costs.len()).map(|_| if next(15) == 0 { 0 } else { 1 + next(3) as u16 }).collect();
            let lm = Landmarks::build(&map, &costs, 8);
            for case in 0..4 {
                let here = (next(60) as i32, next(40) as i32);
                let seeds: Vec<(Tile, u32)> = (0..10).map(|_| ((next(60) as i32, next(40) as i32), next(400))).collect();
                let erased = [(next(60) as i32, next(40) as i32)];
                let cands = candidates(&map, &costs, &seeds, here, &erased);
                if cands.is_empty() {
                    continue;
                }
                for (m, lm_opt) in [(None, None), (None, Some(&lm)), (Some(&mult[..]), None)] {
                    let dist = full_dist(&map, &costs, m, &erased, here, &cands);
                    let total = |j: usize| cand_value(&cands, j).unwrap() as u64 + dist[j] as u64;
                    let by_dijkstra = nearest_candidate(&map, &costs, m, &erased, here, &cands);
                    let by_astar = nearest_candidate_astar(&map, &costs, m, &erased, here, &cands, lm_opt);
                    assert_eq!(by_dijkstra.is_some(), by_astar.is_some(), "seed {seed} case {case}: reachability");
                    if let (Some(a), Some(b)) = (by_dijkstra, by_astar) {
                        assert_eq!(total(a), total(b), "seed {seed} case {case}: totals differ");
                    }
                }
            }
        }
    }

    /// The early-stop choice finds the same target as a full search over all candidates.
    #[test]
    fn nearest_candidate_matches_full_dijkstra() {
        let map = walled(11);
        let costs = map.land_costs().to_vec();
        let here = (30, 20);
        let seeds: Vec<(Tile, u32)> = (0..12).map(|k| ((k * 5 + 1, (k * 7) % 40), (k as u32 * 37) % 300)).collect();
        let cands = candidates(&map, &costs, &seeds, here, &[]);
        let full = sssp(&map, &costs, idx(&map, here).unwrap(), false);
        let best = cands.iter().filter(|&&(j, _)| full[j] != INF).map(|&(j, v)| (v as u64 + full[j] as u64, j)).min();
        let got = nearest_candidate(&map, &costs, None, &[], here, &cands);
        if let Some((total, _)) = best {
            let j = got.expect("a reachable candidate");
            let v = cand_value(&cands, j).unwrap() as u64;
            assert_eq!(v + full[j] as u64, total);
        }
    }
}
