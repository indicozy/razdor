//! Planner prototypes (`RAZDOR_PLANNER=exact|fields|hpa`), beside the exact planner of
//! `ai_plan` (ai.md §7.5: the flood from every seed, the path read by steepest descent).
//!
//! `exact` is the default and is the flood itself, unchanged. `fields` keeps, for each seed
//! cell, the distance field of the static cost map from that cell (a flood from it with
//! multipliers 1), picks the seed with the least seed value plus field distance at the army's
//! cell, and walks down that field. `hpa` cuts the static map into clusters, keeps the walks
//! inside each cluster between its entrance cells, searches that abstract graph from the army
//! and refines each leg with the army's own multipliers. Both fall back to the exact flood when
//! their approximation meets an obstacle they do not see (a closed cell, a multiplier other
//! than 1 next to the walk, a cell a party erases).
//!
//! Prototype: it changes what the AI does where it is used (`Mode`); it is not behind a
//! player-facing switch yet. Measured by `bigmap_ai_bench` and `planner_proto_run.rs`.
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use std::time::Instant;

use crate::rules::map::{Tile, TileMap};

/// "No route" in a `u16` field (the flood's own unreached value).
const UNREACHED: u16 = u16::MAX;
/// "No value" in a `u32` search.
const NO_VALUE: u32 = u32::MAX;
/// "No parent" in the abstract search.
const NONE: u32 = u32::MAX;
const MB: usize = 1 << 20;

/// Which planner drives the AI's paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Exact,
    Fields,
    Hpa,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Exact => "exact",
            Mode::Fields => "fields",
            Mode::Hpa => "hpa",
        }
    }
}

/// What the prototype counted (numbers only; never read by the rules).
#[derive(Clone, Debug, Default)]
pub struct ProtoStats {
    /// Plans made (`plan` calls).
    pub plans: u64,
    /// Seeds over those plans (after merging), for the mean.
    pub seeds: u64,
    /// Plans the mode left to the exact flood (an obstacle it does not see, or no route).
    pub fallbacks: u64,
    /// Cells walked by AI armies (the step of `Game::move_armies`).
    pub steps: u64,
    /// Field builds (fields), cache hits, evictions by the memory budget.
    pub field_builds: u64,
    pub field_hits: u64,
    pub field_evicted: u64,
    /// Peak bytes and count of fields held at once.
    pub field_peak_bytes: usize,
    pub field_peak_count: usize,
    pub field_build_ms: f64,
    /// HPA precompute: milliseconds, bytes of the walks, abstract nodes.
    pub hpa_precompute_ms: f64,
    pub hpa_bytes: usize,
    pub hpa_nodes: usize,
    /// Legs refined with the army's multipliers.
    pub hpa_legs: u64,
    /// Milliseconds in the mode's own planning, and in the exact flood (fallbacks, shadow).
    pub plan_ms: f64,
    pub exact_ms: f64,
    /// Shadow comparison, over the plans the mode did not leave to the exact flood: plans
    /// compared, the exact planner had no path (`cmp_none`), the same first step, the same
    /// target (last cell), the same path; the sum and count of length ratios (mode / exact).
    pub cmp: u64,
    pub cmp_none: u64,
    pub same_first: u64,
    pub same_target: u64,
    pub same_path: u64,
    pub ratio_sum: f64,
    pub ratio_n: u64,
}

/// The prototype's state, one per game (`Game::proto`, not saved).
#[derive(Clone, Debug)]
pub struct Proto {
    pub mode: Mode,
    /// Run the exact planner (its path drives the AI) and compare the mode's plan with it.
    pub shadow: bool,
    /// HPA cluster side in cells.
    pub cluster: i32,
    /// Memory budget of the field cache, in bytes.
    pub field_budget: usize,
    fields: FieldCache,
    /// The abstract graph of the land map (index 0) and of the water map (1), built on use.
    hpa: [Option<Hpa>; 2],
    pub stats: ProtoStats,
}

impl Default for Proto {
    /// From the environment: `RAZDOR_PLANNER` (`fields` or `hpa`, else exact),
    /// `RAZDOR_PLANNER_SHADOW=1`, `RAZDOR_HPA_CLUSTER` (cells, 32), `RAZDOR_FIELD_MB` (64).
    fn default() -> Proto {
        let var = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string());
        let mode = match var("RAZDOR_PLANNER").as_deref() {
            Some("fields") => Mode::Fields,
            Some("hpa") => Mode::Hpa,
            _ => Mode::Exact,
        };
        let shadow = var("RAZDOR_PLANNER_SHADOW").as_deref() == Some("1");
        let cluster = var("RAZDOR_HPA_CLUSTER").and_then(|v| v.parse::<i32>().ok()).filter(|&c| c >= 2).unwrap_or(32);
        let mb = var("RAZDOR_FIELD_MB").and_then(|v| v.parse::<usize>().ok()).unwrap_or(64);
        Proto::new(mode, shadow, cluster, mb * MB)
    }
}

impl Proto {
    pub fn new(mode: Mode, shadow: bool, cluster: i32, field_budget: usize) -> Proto {
        Proto { mode, shadow, cluster, field_budget, fields: FieldCache::default(), hpa: [None, None], stats: ProtoStats::default() }
    }

    /// Bytes the caches hold now: the fields and the abstract graphs.
    pub fn bytes(&self) -> usize {
        self.fields.bytes + self.hpa.iter().flatten().map(|h| h.bytes).sum::<usize>()
    }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn costs_of(map: &TileMap, water: bool) -> &[u16] {
    if water {
        map.water_costs()
    } else {
        map.land_costs()
    }
}

/// The exact planner (`ai_plan` before the prototype): one flood from all seeds, the cells in
/// `erased` taken out, the path read by steepest descent (`None` when no seed was kept).
pub(crate) fn exact_path(map: &TileMap, water: bool, here: Tile, seeds: &[(Tile, u32)], mult: &[u16], erased: &[Tile]) -> Option<Vec<Tile>> {
    let mut field = map.flood_maps(costs_of(map, water), mult, seeds, here);
    for &t in erased {
        field.erase(t);
    }
    (field.kept > 0).then(|| map.descend(&field, here))
}

/// Plans the path of one army: the mode's own path, or the exact flood where the mode leaves
/// it (or in shadow mode, the exact path, with the mode's path compared with it).
#[allow(clippy::too_many_arguments)]
pub fn plan(p: &mut Proto, map: &TileMap, water: bool, here: Tile, seeds: &[(Tile, u32)], mult: &[u16], erased: &[Tile]) -> Option<Vec<Tile>> {
    p.stats.plans += 1;
    if p.mode == Mode::Exact {
        let t0 = Instant::now();
        let path = exact_path(map, water, here, seeds, mult, erased);
        p.stats.exact_ms += ms(t0);
        return path;
    }
    let t0 = Instant::now();
    let own = match p.mode {
        Mode::Fields => fields_path(p, map, water, here, seeds, mult, erased),
        Mode::Hpa => hpa_path(p, map, water, here, seeds, mult, erased),
        Mode::Exact => None,
    };
    p.stats.plan_ms += ms(t0);
    if p.shadow {
        let t1 = Instant::now();
        let exact = exact_path(map, water, here, seeds, mult, erased);
        p.stats.exact_ms += ms(t1);
        match own {
            Some(m) => compare(&mut p.stats, &m, &exact),
            None => p.stats.fallbacks += 1,
        }
        return exact;
    }
    match own {
        Some(m) => Some(m),
        None => {
            p.stats.fallbacks += 1;
            let t1 = Instant::now();
            let path = exact_path(map, water, here, seeds, mult, erased);
            p.stats.exact_ms += ms(t1);
            path
        }
    }
}

fn compare(st: &mut ProtoStats, own: &[Tile], exact: &Option<Vec<Tile>>) {
    st.cmp += 1;
    let Some(e) = exact else {
        st.cmp_none += 1;
        return;
    };
    if own.first() == e.first() {
        st.same_first += 1;
    }
    if own.last() == e.last() {
        st.same_target += 1;
    }
    if own == e.as_slice() {
        st.same_path += 1;
    }
    if !own.is_empty() && !e.is_empty() {
        st.ratio_sum += own.len() as f64 / e.len() as f64;
        st.ratio_n += 1;
    }
}

/// The seeds on cells the static map lets a walk enter, with the seed's own cell (the army's)
/// dropped, one per cell (the lowest value), sorted by cell.
fn merged_seeds(map: &TileMap, costs: &[u16], here_i: usize, seeds: &[(Tile, u32)]) -> Vec<(usize, u32)> {
    let mut out: Vec<(usize, u32)> = seeds
        .iter()
        .filter_map(|&(t, v)| {
            let i = map.index(t)?;
            (costs[i] > 0 && i != here_i).then_some((i, v.min(32_766)))
        })
        .collect();
    out.sort_unstable();
    out.dedup_by_key(|e| e.0);
    out
}

/// True when no multiplier other than 1 and no erased cell lies in the 3×3 block around `t`:
/// the static field is then the field the exact flood would see there.
fn clear(map: &TileMap, mult: &[u16], erased: &[Tile], (x, y): Tile) -> bool {
    for dy in -1..=1 {
        for dx in -1..=1 {
            let t = (x + dx, y + dy);
            let Some(i) = map.index(t) else { continue };
            if mult[i] != 1 || erased.contains(&t) {
                return false;
            }
        }
    }
    true
}

// ------------------------------------------------------------------------------------------
// fields
// ------------------------------------------------------------------------------------------

/// The static distance field of a flood from `src` with multipliers 1: the value of a cell is
/// the walk's sum of entered costs times the step weights (the flood's own, `flood_eff`), so a
/// seed of value `v` reaches cell `x` at `v + 1 + field[x]` in the exact flood.
fn build_field(map: &TileMap, costs: &[u16], src: usize) -> Vec<u16> {
    let mut dist = vec![UNREACHED; costs.len()];
    dist[src] = 0;
    let mut heap = BinaryHeap::new();
    heap.push(Reverse((0u32, src as u32)));
    while let Some(Reverse((d, i))) = heap.pop() {
        let i = i as usize;
        if d > dist[i] as u32 {
            continue;
        }
        for (dir, j) in map.neighbour_indices(i).into_iter().enumerate() {
            let Some(j) = j else { continue };
            let c = costs[j] as u32;
            if c == 0 {
                continue;
            }
            let nd = d + c * map.grid.direction_weight(dir);
            if nd < UNREACHED as u32 && nd < dist[j] as u32 {
                dist[j] = nd as u16;
                heap.push(Reverse((nd, j as u32)));
            }
        }
    }
    dist
}

/// Fields held by key (water, seed cell), least recently used first out, within a byte budget.
#[derive(Clone, Debug, Default)]
struct FieldCache {
    fields: HashMap<(bool, usize), (u64, Vec<u16>)>,
    order: BTreeMap<u64, (bool, usize)>,
    clock: u64,
    bytes: usize,
}

impl FieldCache {
    /// Marks `key` as used; false when it is not held.
    fn touch(&mut self, key: (bool, usize)) -> bool {
        let Some(entry) = self.fields.get_mut(&key) else { return false };
        self.clock += 1;
        self.order.remove(&entry.0);
        entry.0 = self.clock;
        self.order.insert(self.clock, key);
        true
    }

    fn data(&self, key: (bool, usize)) -> &[u16] {
        &self.fields[&key].1
    }

    fn insert(&mut self, key: (bool, usize), data: Vec<u16>, budget: usize, stats: &mut ProtoStats) -> &[u16] {
        let size = data.len() * 2;
        while self.bytes + size > budget {
            let Some((&stamp, &old)) = self.order.iter().next() else { break };
            self.order.remove(&stamp);
            if let Some((_, v)) = self.fields.remove(&old) {
                self.bytes -= v.len() * 2;
            }
            stats.field_evicted += 1;
        }
        self.clock += 1;
        self.order.insert(self.clock, key);
        self.bytes += size;
        self.fields.insert(key, (self.clock, data));
        stats.field_peak_bytes = stats.field_peak_bytes.max(self.bytes);
        stats.field_peak_count = stats.field_peak_count.max(self.fields.len());
        self.data(key)
    }
}

/// The cached field of seed `src` (built on a miss).
fn field<'a>(fields: &'a mut FieldCache, budget: usize, stats: &mut ProtoStats, map: &TileMap, costs: &[u16], water: bool, src: usize) -> &'a [u16] {
    let key = (water, src);
    if fields.touch(key) {
        stats.field_hits += 1;
        return fields.data(key);
    }
    let t0 = Instant::now();
    let data = build_field(map, costs, src);
    stats.field_build_ms += ms(t0);
    stats.field_builds += 1;
    fields.insert(key, data, budget, stats)
}

/// The `fields` planner: the seed of least `value + 1 + field(here)` and its field walked down
/// from `here`. `None` when no seed reaches `here` or the walk meets an obstacle.
pub(crate) fn fields_path(p: &mut Proto, map: &TileMap, water: bool, here: Tile, seeds: &[(Tile, u32)], mult: &[u16], erased: &[Tile]) -> Option<Vec<Tile>> {
    let costs = costs_of(map, water);
    let here_i = map.index(here)?;
    let uniq = merged_seeds(map, costs, here_i, seeds);
    let Proto { fields, field_budget, stats, .. } = p;
    stats.seeds += uniq.len() as u64;
    let mut best: Option<(u64, usize)> = None;
    for &(si, v) in &uniq {
        let f = field(fields, *field_budget, stats, map, costs, water, si);
        let g = f[here_i];
        if g == UNREACHED {
            continue;
        }
        let total = v as u64 + 1 + g as u64;
        match best {
            Some((t, _)) if t <= total => {}
            _ => best = Some((total, si)),
        }
    }
    let (_, si) = best?;
    let f = field(fields, *field_budget, stats, map, costs, water, si);
    descend_field(map, f, here_i, here, mult, erased)
}

/// Steepest descent on a field from `here` (the flood's rule: the first strict minimum in
/// direction order); `None` when a step or the start touches an obstacle.
fn descend_field(map: &TileMap, f: &[u16], here_i: usize, here: Tile, mult: &[u16], erased: &[Tile]) -> Option<Vec<Tile>> {
    if f[here_i] == UNREACHED || !clear(map, mult, erased, here) {
        return None;
    }
    let mut at = here_i;
    let mut d = f[at];
    let mut path = Vec::new();
    while d != 0 {
        let mut best = None;
        let mut best_d = d;
        for j in map.neighbour_indices(at).into_iter().flatten() {
            let dj = f[j];
            if dj != UNREACHED && dj < best_d {
                best = Some(j);
                best_d = dj;
            }
        }
        let j = best?;
        let t = map.tile_of(j);
        if !clear(map, mult, erased, t) {
            return None;
        }
        path.push(t);
        at = j;
        d = best_d;
    }
    Some(path)
}

// ------------------------------------------------------------------------------------------
// hpa
// ------------------------------------------------------------------------------------------

/// A rectangle of cells: one cluster's box (clipped at the map's edge).
#[derive(Clone, Copy, Debug)]
struct Bx {
    x0: i32,
    y0: i32,
    bw: i32,
    bh: i32,
}

impl Bx {
    fn contains(&self, (x, y): Tile) -> bool {
        x >= self.x0 && y >= self.y0 && x < self.x0 + self.bw && y < self.y0 + self.bh
    }

    fn local(&self, (x, y): Tile) -> usize {
        ((y - self.y0) * self.bw + (x - self.x0)) as usize
    }

    fn len(&self) -> usize {
        (self.bw * self.bh) as usize
    }
}

/// The abstract graph of one cost map. Nodes are entrance cells: a pair of neighbouring
/// passable cells across a cluster border (one pair per run of pairs of up to six, else the
/// two ends of the run). Node `n` pairs with `n ^ 1` (the other side of its border). Each
/// node keeps the walks to every cell of its own cluster's box, so a walk through a cluster
/// is a lookup.
#[derive(Clone, Debug)]
pub struct Hpa {
    c: i32,
    cw: i32,
    w: i32,
    h: i32,
    node_tile: Vec<Tile>,
    node_cluster: Vec<u32>,
    cluster_nodes: Vec<Vec<u32>>,
    /// Per node: the walk from every cell of its cluster's box to the node (`UNREACHED`
    /// where none), the flood's own entering costs (static map).
    dense: Vec<Vec<u16>>,
    bytes: usize,
}

impl Hpa {
    fn nodes(&self) -> usize {
        self.node_tile.len()
    }

    fn cluster_of(&self, (x, y): Tile) -> usize {
        ((y / self.c) * self.cw + x / self.c) as usize
    }

    fn bx(&self, k: usize) -> Bx {
        let (kx, ky) = (k as i32 % self.cw, k as i32 / self.cw);
        let (x0, y0) = (kx * self.c, ky * self.c);
        Bx { x0, y0, bw: self.c.min(self.w - x0), bh: self.c.min(self.h - y0) }
    }

    fn build(map: &TileMap, costs: &[u16], c: i32) -> Hpa {
        let (w, h) = (map.w, map.h);
        let cw = (w + c - 1) / c;
        let ch = (h + c - 1) / c;
        let mut hp = Hpa {
            c,
            cw,
            w,
            h,
            node_tile: Vec::new(),
            node_cluster: Vec::new(),
            cluster_nodes: vec![Vec::new(); (cw * ch).max(0) as usize],
            dense: Vec::new(),
            bytes: 0,
        };
        // Passable neighbour pairs across a border, grouped by the two clusters, in cell order.
        let mut groups: BTreeMap<(usize, usize), Vec<(usize, usize)>> = BTreeMap::new();
        for i in 0..costs.len() {
            if costs[i] == 0 {
                continue;
            }
            let ci = hp.cluster_of(map.tile_of(i));
            for j in map.neighbour_indices(i).into_iter().flatten() {
                if j <= i || costs[j] == 0 {
                    continue;
                }
                let cj = hp.cluster_of(map.tile_of(j));
                if cj != ci {
                    groups.entry((ci.min(cj), ci.max(cj))).or_default().push((i, j));
                }
            }
        }
        for pairs in groups.values() {
            let mut k = 0;
            while k < pairs.len() {
                let mut e = k + 1;
                while e < pairs.len() && adjacent(map, pairs[e - 1].0, pairs[e].0) && adjacent(map, pairs[e - 1].1, pairs[e].1) {
                    e += 1;
                }
                let run = &pairs[k..e];
                let picks: Vec<(usize, usize)> = if run.len() <= 6 { vec![run[run.len() / 2]] } else { vec![run[0], run[run.len() - 1]] };
                for (i, j) in picks {
                    for cell in [i, j] {
                        let t = map.tile_of(cell);
                        hp.node_tile.push(t);
                        hp.node_cluster.push(hp.cluster_of(t) as u32);
                    }
                }
                k = e;
            }
        }
        for n in 0..hp.node_tile.len() {
            hp.cluster_nodes[hp.node_cluster[n] as usize].push(n as u32);
        }
        for v in 0..hp.node_tile.len() {
            let bx = hp.bx(hp.node_cluster[v] as usize);
            let src = map.index(hp.node_tile[v]).unwrap_or(0);
            let d = box_reverse(map, bx, src, usize::MAX, &|i| costs[i] as u32);
            hp.dense.push(d.into_iter().map(to_u16).collect());
        }
        hp.bytes = hp.dense.iter().map(|d| d.len() * 2).sum::<usize>() + hp.node_tile.len() * 16 + hp.cluster_nodes.iter().map(|c| c.len() * 4).sum::<usize>();
        hp
    }
}

fn adjacent(map: &TileMap, i: usize, j: usize) -> bool {
    map.grid.distance(map.tile_of(i), map.tile_of(j)) == 1
}

fn to_u16(v: u32) -> u16 {
    if v >= UNREACHED as u32 {
        UNREACHED
    } else {
        v as u16
    }
}

/// The reverse walk search inside a box, from `src`: `g[x]` is the least sum over the walk
/// `x → … → src` of the entered cells' `eff` times their step weights, `src` itself not
/// charged. A cell is walked through (charged when left, in the reverse search) only when its
/// `eff` is not 0, or it is `exempt` (the army's own cell, which the flood also lets walks
/// start from).
fn box_reverse(map: &TileMap, bx: Bx, src: usize, exempt: usize, eff: &dyn Fn(usize) -> u32) -> Vec<u32> {
    let mut g = vec![NO_VALUE; bx.len()];
    let st = map.tile_of(src);
    g[bx.local(st)] = 0;
    let mut heap = BinaryHeap::new();
    heap.push(Reverse((0u32, src as u32)));
    while let Some(Reverse((d, y))) = heap.pop() {
        let y = y as usize;
        let yt = map.tile_of(y);
        if d > g[bx.local(yt)] {
            continue;
        }
        let ey = eff(y);
        if ey == 0 && y != exempt {
            continue;
        }
        for (dir, x) in map.neighbour_indices(y).into_iter().enumerate() {
            let Some(x) = x else { continue };
            let xt = map.tile_of(x);
            if !bx.contains(xt) {
                continue;
            }
            let nd = d.saturating_add(ey.saturating_mul(map.grid.direction_weight(dir)));
            let l = bx.local(xt);
            if nd < g[l] {
                g[l] = nd;
                heap.push(Reverse((nd, x as u32)));
            }
        }
    }
    g
}

/// The walk from `x` to `y` read off a [`box_reverse`] field of `y`: from each cell the
/// neighbour with the least `g + eff × weight` (the first strict minimum), until `y`.
fn trace(map: &TileMap, bx: Bx, g: &[u32], x: Tile, y: Tile, exempt: usize, eff: &dyn Fn(usize) -> u32) -> Option<Vec<Tile>> {
    let mut out = Vec::new();
    let mut c = x;
    for _ in 0..=bx.len() {
        if c == y {
            return Some(out);
        }
        let ci = map.index(c)?;
        let mut best: Option<(u32, Tile)> = None;
        for (dir, z) in map.neighbour_indices(ci).into_iter().enumerate() {
            let Some(z) = z else { continue };
            let zt = map.tile_of(z);
            if !bx.contains(zt) {
                continue;
            }
            let gz = g[bx.local(zt)];
            if gz == NO_VALUE {
                continue;
            }
            let ez = eff(z);
            if ez == 0 && z != exempt {
                continue;
            }
            let cand = gz.saturating_add(ez.saturating_mul(map.grid.direction_weight(dir)));
            match best {
                Some((b, _)) if b <= cand => {}
                _ => best = Some((cand, zt)),
            }
        }
        let (_, zt) = best?;
        out.push(zt);
        c = zt;
    }
    None
}

/// One leg of the refined walk, `x` to `y` (`y` on the walk's side nearer the army), appended
/// to `out` without `x`. Neighbouring cells are one step; otherwise both lie in one cluster
/// and the leg is searched inside its box with the army's own costs.
#[allow(clippy::too_many_arguments)]
fn leg(map: &TileMap, hp: &Hpa, eff: &dyn Fn(usize) -> u32, here: Tile, here_i: usize, x: Tile, y: Tile, out: &mut Vec<Tile>) -> Option<()> {
    // `here` is the army's cell: the one cell a walk may enter although its own cost is 0.
    let yi = map.index(y)?;
    if map.grid.distance(x, y) == 1 {
        if y != here && eff(yi) == 0 {
            return None;
        }
        out.push(y);
        return Some(());
    }
    let cy = hp.cluster_of(y);
    if hp.cluster_of(x) != cy {
        return None;
    }
    let bx = hp.bx(cy);
    let g = box_reverse(map, bx, yi, here_i, eff);
    out.extend(trace(map, bx, &g, x, y, here_i, eff)?);
    Some(())
}

/// Where the abstract route starts: the seed reached through its own cluster's box from `here`
/// (`Direct`), or through an entrance node `u` of its cluster.
#[derive(Clone, Copy, Debug)]
enum Route {
    Direct,
    Via(usize),
}

/// The `hpa` planner. The abstract search runs from `here` over the entrance nodes with the
/// static costs; each seed's value is its least walk into the graph; the chosen route is
/// refined leg by leg with the army's multipliers and erased cells. `None` (a fallback) when
/// no seed is reached or a leg cannot be walked.
pub(crate) fn hpa_path(p: &mut Proto, map: &TileMap, water: bool, here: Tile, seeds: &[(Tile, u32)], mult: &[u16], erased: &[Tile]) -> Option<Vec<Tile>> {
    let costs = costs_of(map, water);
    let here_i = map.index(here)?;
    let k = water as usize;
    if p.hpa[k].is_none() {
        let t0 = Instant::now();
        let hp = Hpa::build(map, costs, p.cluster);
        p.stats.hpa_precompute_ms += ms(t0);
        p.stats.hpa_bytes += hp.bytes;
        p.stats.hpa_nodes += hp.nodes();
        p.hpa[k] = Some(hp);
    }
    let Proto { hpa, stats, .. } = p;
    let hp = hpa[k].as_ref()?;
    let uniq = merged_seeds(map, costs, here_i, seeds);
    stats.seeds += uniq.len() as u64;

    let x0 = hp.cluster_of(here);
    let bx_here = hp.bx(x0);
    let ga = box_reverse(map, bx_here, here_i, here_i, &|i| costs[i] as u32);

    let nn = hp.nodes();
    let mut g = vec![NO_VALUE; nn];
    let mut parent = vec![NONE; nn];
    let mut heap = BinaryHeap::new();
    for &u in &hp.cluster_nodes[x0] {
        let v = ga[bx_here.local(hp.node_tile[u as usize])];
        if v != NO_VALUE {
            g[u as usize] = v;
            heap.push(Reverse((v, u)));
        }
    }
    while let Some(Reverse((d, v))) = heap.pop() {
        let v = v as usize;
        if d > g[v] {
            continue;
        }
        let cv = hp.node_cluster[v] as usize;
        let bxv = hp.bx(cv);
        for &u in &hp.cluster_nodes[cv] {
            let u = u as usize;
            if u == v {
                continue;
            }
            let w = hp.dense[v][bxv.local(hp.node_tile[u])];
            if w == UNREACHED {
                continue;
            }
            let nd = d + w as u32;
            if nd < g[u] {
                g[u] = nd;
                parent[u] = v as u32;
                heap.push(Reverse((nd, u as u32)));
            }
        }
        // The other side of the border: the walk enters `v` from there.
        let q = v ^ 1;
        let tv = hp.node_tile[v];
        let vi = map.index(tv).unwrap_or(0);
        let nd = d.saturating_add(costs[vi] as u32 * map.grid.weight(tv, hp.node_tile[q]));
        if nd < g[q] {
            g[q] = nd;
            parent[q] = v as u32;
            heap.push(Reverse((nd, q as u32)));
        }
    }

    let mut best: Option<(u64, usize, Route)> = None;
    for (si, &(s_i, v)) in uniq.iter().enumerate() {
        let st = map.tile_of(s_i);
        let cs = hp.cluster_of(st);
        let ls = hp.bx(cs).local(st);
        let mut bg = NO_VALUE;
        let mut route = Route::Direct;
        if cs == x0 {
            let d = ga[bx_here.local(st)];
            if d != NO_VALUE {
                bg = d;
            }
        }
        for &u in &hp.cluster_nodes[cs] {
            let u = u as usize;
            if g[u] == NO_VALUE {
                continue;
            }
            let w = hp.dense[u][ls];
            if w == UNREACHED {
                continue;
            }
            let t = w as u32 + g[u];
            if t < bg {
                bg = t;
                route = Route::Via(u);
            }
        }
        if bg == NO_VALUE {
            continue;
        }
        let total = v as u64 + 1 + bg as u64;
        match best {
            Some((t, _, _)) if t <= total => {}
            _ => best = Some((total, si, route)),
        }
    }
    let (_, si, route) = best?;
    let s = map.tile_of(uniq[si].0);
    let mut keys = vec![s];
    if let Route::Via(u) = route {
        keys.push(hp.node_tile[u]);
        let mut v = u;
        while parent[v] != NONE {
            v = parent[v] as usize;
            keys.push(hp.node_tile[v]);
        }
    }
    keys.push(here);

    let eff = |i: usize| -> u32 {
        let t = map.tile_of(i);
        if erased.contains(&t) {
            0
        } else {
            costs[i] as u32 * mult[i] as u32
        }
    };
    let mut walk = vec![s];
    for pair in keys.windows(2) {
        if pair[0] == pair[1] {
            continue;
        }
        stats.hpa_legs += 1;
        leg(map, hp, &eff, here, here_i, pair[0], pair[1], &mut walk)?;
    }
    if walk.last() != Some(&here) {
        return None;
    }
    walk.pop();
    walk.reverse();
    Some(walk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::Surface;
    use crate::rules::map::Grid;

    /// A fixed-seed generator for the test maps (not the game's).
    struct Lcg(u32);

    impl Lcg {
        fn below(&mut self, n: u32) -> u32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (self.0 >> 8) % n
        }
    }

    /// Random small maps with blocked and costly cells, closed and heavy multipliers, seeds,
    /// erased cells: each prototype path is a walk of passable cells, one step at a time, that
    /// ends on a seed and touches no closed or erased cell; the planners agree with the exact
    /// one on the target most of the time.
    #[test]
    fn prototype_paths_are_valid_walks_to_a_seed() {
        let mut rng = Lcg(7);
        let (mut checked, mut same_target, mut fellback) = (0u32, 0u32, 0u32);
        for case in 0..300 {
            let (w, h) = (4 + rng.below(30) as i32, 4 + rng.below(30) as i32);
            let n = (w * h) as usize;
            let codes = vec![Surface::GrassPlain as u8; n];
            let mut map = TileMap::from_codes(Grid::Square8, w, h, &codes, Vec::new());
            for i in 0..n {
                let t = (i as i32 % w, i as i32 / w);
                match rng.below(8) {
                    0 => map.block(t),
                    1 => map.open(t, 1 + rng.below(9) as u16),
                    _ => {}
                }
            }
            let mult: Vec<u16> = (0..n)
                .map(|_| match rng.below(12) {
                    0 => 0,
                    1 => 3,
                    _ => 1,
                })
                .collect();
            let open: Vec<Tile> = (0..n).map(|i| (i as i32 % w, i as i32 / w)).filter(|&t| map.cost(t).is_some()).collect();
            if open.len() < 3 {
                continue;
            }
            let pick = |rng: &mut Lcg| open[rng.below(open.len() as u32) as usize];
            let here = pick(&mut rng);
            let seeds: Vec<(Tile, u32)> = (0..1 + rng.below(4)).map(|_| (pick(&mut rng), rng.below(40))).filter(|&(t, _)| t != here).collect();
            if seeds.is_empty() {
                continue;
            }
            let erased: Vec<Tile> = (0..rng.below(3)).map(|_| pick(&mut rng)).filter(|&t| t != here).collect();
            let exact = exact_path(&map, false, here, &seeds, &mult, &erased);
            for mode in [Mode::Fields, Mode::Hpa] {
                let mut p = Proto::new(mode, false, 2 + (case % 7) as i32, 1 << 26);
                let own = match mode {
                    Mode::Fields => fields_path(&mut p, &map, false, here, &seeds, &mult, &erased),
                    _ => hpa_path(&mut p, &map, false, here, &seeds, &mult, &erased),
                };
                let Some(path) = own else {
                    fellback += 1;
                    continue;
                };
                assert!(!path.is_empty(), "case {case} {mode:?}: empty path");
                let mut prev = here;
                let last = path.len() - 1;
                for (k, &t) in path.iter().enumerate() {
                    assert_eq!(map.grid.distance(prev, t), 1, "case {case} {mode:?}: not a step");
                    assert!(map.cost(t).is_some(), "case {case} {mode:?}: a blocked cell");
                    // A seed may stand on a closed cell (the flood keeps it as a seed); the walk
                    // up to it may not.
                    let i = map.index(t).unwrap();
                    assert!(k == last || (mult[i] != 0 && !erased.contains(&t)), "case {case} {mode:?}: a closed or erased cell");
                    prev = t;
                }
                assert!(seeds.iter().any(|&(s, _)| Some(&s) == path.last()), "case {case} {mode:?}: does not end on a seed");
                checked += 1;
                if exact.as_ref().and_then(|e| e.last()) == path.last() {
                    same_target += 1;
                }
            }
        }
        println!("prototype paths: checked {checked}, same target as exact {same_target}, fell back {fellback}");
        assert!(checked > 200, "too few prototype paths checked: {checked}");
    }
}
