//! PROTOTYPE (branch `proto/planner-a`, not for `main`): approximate planners selected by env
//! vars, to measure speed, memory and how far they move the AI from the exact planner. With
//! nothing set (`RAZDOR_PLANNER` unset or `exact`, no `RAZDOR_PLANNER_LOD`, no shadow) every
//! hook here is the exact planner and the AI is unchanged.
//!
//! - `RAZDOR_PLANNER=exact|window|window:N|astar`, `RAZDOR_PLANNER_R` (window radius, 48).
//! - `RAZDOR_PLANNER_LOD=off|countdown|keep|both`, `RAZDOR_PLANNER_LOD_D` (30),
//!   `RAZDOR_PLANNER_LOD_K` (3): armies beyond D cells (octile) from the hero.
//! - `RAZDOR_PLANNER_SHADOW=window:32,astar,...`: at every plan, also compute these modes and
//!   tally how they agree with the exact plan (the game still follows the active mode).
use std::cell::RefCell;
use std::sync::OnceLock;

use crate::rules::map::{Tile, TileMap, ORTHOGONAL_WEIGHT};

/// The flood the planner runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The original's planner, as shipped: one flood from every seed.
    Exact,
    /// A square window of this radius around the army; outside seeds moved to its border.
    Window(i32),
    /// The best seed by estimate, then A* to that one cell.
    AStar,
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
        "astar" => Some(Mode::AStar),
        "window" => Some(Mode::Window(r)),
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
    Cfg { mode, lod, lod_d, lod_k, shadow }
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
    }
}

thread_local! {
    static TALLY: RefCell<Tally> = RefCell::new(Tally::default());
}

/// One AI army step.
pub fn count_step() {
    TALLY.with(|t| t.borrow_mut().steps += 1);
}

/// One party trigger of a re-plan that the LOD rule suppressed.
pub fn count_lod_skip() {
    TALLY.with(|t| t.borrow_mut().lod_skips += 1);
}

/// The tally so far, which starts again from zero (read by the ignored free run).
#[cfg_attr(not(test), allow(dead_code))]
pub fn take_tally() -> Tally {
    TALLY.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

fn record_shadow(k: usize, exact: &Option<Vec<Tile>>, alt: &Option<Vec<Tile>>) {
    TALLY.with(|t| {
        let mut tally = t.borrow_mut();
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

/// The planner's path for one plan (the inputs of `Game::ai_plan` after its seeds): `Some`
/// with the path (empty: stand), `None` with no seed kept. `erased` are the cells of the
/// parties in reach and the cells they step to, which the path never steps onto.
pub fn plan(map: &TileMap, costs: &[u16], mult: &[u16], seeds: &[(Tile, u32)], here: Tile, erased: &[Tile]) -> Option<Vec<Tile>> {
    let c = cfg();
    let run = |m: Mode| match m {
        Mode::Exact => exact_path(map, costs, mult, seeds, here, erased),
        Mode::Window(r) => window_path(map, costs, mult, seeds, here, erased, r),
        Mode::AStar => astar_path(map, costs, mult, seeds, here, erased),
    };
    let path = run(c.mode);
    if !c.shadow.is_empty() {
        let exact = if c.mode == Mode::Exact { path.clone() } else { exact_path(map, costs, mult, seeds, here, erased) };
        for (k, &m) in c.shadow.iter().enumerate() {
            let alt = run(m);
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

fn index(map: &TileMap, t: Tile) -> Option<usize> {
    (t.0 >= 0 && t.1 >= 0 && t.0 < map.w && t.1 < map.h).then(|| (t.1 * map.w + t.0) as usize)
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
        if index(map, t).is_none() {
            continue;
        }
        if inside(t) {
            out.push((t, v));
            continue;
        }
        let mut b = (t.0.clamp(x0, x1), t.1.clamp(y0, y1));
        if index(map, b).is_some_and(|j| costs[j] == 0) {
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
        if index(map, b).is_some_and(|j| costs[j] > 0) {
            let d = map.grid.octile(b, t);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, b));
            }
        }
    }
    best.map(|(_, b)| b)
}

/// The cost of stepping onto `to` for A*: `cost × multiplier`, 0 (closed) is refused unless
/// `closed_ok` (the target: the flood places a seed on a closed cell and expands from it, so
/// a path may end on one), and an erased cell is refused.
fn step_cost(map: &TileMap, costs: &[u16], mult: &[u16], erased: &[Tile], to: Tile, closed_ok: &dyn Fn(Tile) -> bool) -> Option<u16> {
    let j = index(map, to)?;
    if erased.contains(&to) {
        return None;
    }
    let c = costs[j].wrapping_mul(mult[j]);
    if c != 0 {
        return Some(c);
    }
    closed_ok(to).then_some(1)
}

/// Choose the target first: the seed with the least `value + octile × average_step` (a seed on
/// the army's cell, on a cell it cannot enter or on an erased cell is not a candidate), then A*
/// (the octile heuristic in step weights, the same cell costs and multipliers) to that one cell.
/// When that cell is cut off, the nearest candidate by Dijkstra. Empty path: stand. `None`: no
/// candidate.
pub fn astar_path(map: &TileMap, costs: &[u16], mult: &[u16], seeds: &[(Tile, u32)], here: Tile, erased: &[Tile]) -> Option<Vec<Tile>> {
    let avg = average_step(costs);
    let candidate = |t: Tile| t != here && index(map, t).is_some_and(|j| costs[j] != 0) && !erased.contains(&t);
    let mut best: Option<(u64, Tile)> = None;
    for &(t, v) in seeds {
        if !candidate(t) {
            continue;
        }
        let est = v as u64 + map.grid.octile(here, t) as u64 * avg as u64;
        match best {
            Some((e, _)) if e <= est => {}
            _ => best = Some((est, t)),
        }
    }
    let (_, target) = best?;
    // The start cell erased: the exact descent reads nothing from it and stands.
    if erased.contains(&here) {
        return Some(Vec::new());
    }
    let n = costs.len();
    let p = map.path_by(here, target, n, &|_, to| step_cost(map, costs, mult, erased, to, &|t| t == target));
    if !p.is_empty() {
        return Some(p);
    }
    let mut cand = vec![false; n];
    for &(t, _) in seeds {
        if candidate(t) {
            if let Some(j) = index(map, t) {
                cand[j] = true;
            }
        }
    }
    let is_cand = |t: Tile| index(map, t).is_some_and(|j| cand[j]);
    let p = map.path_to_any(here, &is_cand, n, &|_, to| step_cost(map, costs, mult, erased, to, &is_cand));
    (!p.is_empty()).then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_names_parse() {
        assert_eq!(parse_mode("exact", 48), Some(Mode::Exact));
        assert_eq!(parse_mode("window", 48), Some(Mode::Window(48)));
        assert_eq!(parse_mode("window:32", 48), Some(Mode::Window(32)));
        assert_eq!(parse_mode("astar", 48), Some(Mode::AStar));
        assert_eq!(parse_mode("nope", 48), None);
    }
}
