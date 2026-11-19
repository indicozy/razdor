//! The winding paths' distance field, baked on the CPU at map load (`bake_paths`).
//!
//! A road drawn as a path bends through the centres of its cells instead of showing its
//! cell-shaped strips. This module is pure Rust (no macroquad): it is unit-tested here and dumped
//! to PNG by a debug tool; the ground shader reads one distance per texel later.
//!
//! Steps (the owner's design, 2026-11-19, spec `docs/specs/improved-graphics-2026-11-19.md` §0, §2.2):
//! 1. Nodes are path cells. A link joins two 4-neighbours; a diagonal link only where neither of
//!    the two orthogonal cells between them is a path cell.
//! 2. Chains are cut at pinned nodes: a node whose degree is not 2, and every fill cell. Links left
//!    over form closed loops.
//! 3. Each chain is smoothed with Chaikin corner cutting; its ends stay on their cell centres.
//!    See [`fit`] for how the iteration count is chosen so the invariants hold.
//! 4. Fill: a path cell in an all-path 2×2 block is a fill cell (a wide area, all road). Its texels
//!    store distance 0.
//! 5. Each texel stores the distance to the nearest smoothed segment, over `0..MAX_DIST` cells.
//!
//! Invariants (tested; kept by construction in [`fit`]):
//! - a path cell's centre lies within `MIN_HALF_WIDTH` of a centreline, or the cell is fill;
//! - a non-path cell's centre lies farther than `FAR_FROM_PATH` from every centreline, and is not fill.

/// A texel's distance is stored over `0..MAX_DIST` cells (255 is this far or more).
pub const MAX_DIST: f32 = 1.0;
/// A path cell's centre lies within this distance of a centreline (cells), or in fill. A staircase's centres sit
/// 0.354 off its diagonal, so the limit must be above that for the staircase to read as one straight diagonal.
pub const MIN_HALF_WIDTH: f32 = 0.36;
/// A non-path cell's centre lies farther than this from every centreline (cells), and from every fill (`fill_distance`).
pub const FAR_FROM_PATH: f32 = 0.50;
/// The most Chaikin iterations a chain gets.
const MAX_ITER: u32 = 4;
/// The fill's edge is inset by this much (cells): a fill's distance is 0 at `FILL_INSET` inside its edge... see
/// [`fill_distance`]. Outside the edge it grows from `FILL_INSET`, so a thin path meets a square with a soft edge.
const FILL_INSET: f32 = 0.5;
/// The largest map side (cells) that is baked; a bigger map gets no winding paths (today's look).
pub const MAX_BAKED_SIDE: u32 = 1024;
/// Texels per cell for a map: 8 up to 512 cells a side, else 4 (the texture stays within 4096 texels a side).
pub fn texels_per_cell_for(width: u32, height: u32) -> u32 {
    if width.max(height) <= 512 {
        8
    } else {
        4
    }
}
/// Surface codes that block a road's narrowing: water (0-2), lava (3), impassable swamp (9), snowdrifts (15).
pub fn is_blocked_code(code: u8) -> bool {
    matches!(code, 0..=3 | 9 | 15)
}
/// How far (in parameter units, i.e. node indices) the windowed node check looks along a chain.
const WINDOW: f32 = 3.0;
/// Neighbour steps: 0..4 orthogonal, 4..8 diagonal.
const DIRS: [(i32, i32); 8] = [(1, 0), (0, 1), (-1, 0), (0, -1), (1, 1), (-1, 1), (-1, -1), (1, -1)];

type Pt = (f32, f32);
/// A chain point: x, y, and its parameter along the unsmoothed chain (the node index).
type P3 = [f32; 3];

/// One smoothed centreline.
#[derive(Clone, Debug, PartialEq)]
pub struct Polyline {
    /// Points in cell units (x right, y down). A closed one does not repeat its first point.
    pub points: Vec<Pt>,
    pub closed: bool,
}

/// The baked winding paths of one map.
#[derive(Clone, Debug)]
pub struct PathBake {
    pub cells_w: u32,
    pub cells_h: u32,
    pub texels_per_cell: u32,
    /// One byte per texel, row by row: the distance from the nearest centreline over `0..MAX_DIST`
    /// (0 on a centreline and in fill, 255 at `MAX_DIST` or more).
    pub dist: Vec<u8>,
    /// One flag per texel, row by row: the texel lies in a fill cell (a wide area, all road).
    pub fill: Vec<bool>,
    /// The smoothed centrelines (debugging and tests).
    pub polylines: Vec<Polyline>,
}

impl PathBake {
    /// The texture width in texels.
    fn texel_width(&self) -> usize {
        self.cells_w as usize * self.texels_per_cell as usize
    }

    /// Whether cell (x, y) is a fill cell.
    pub fn is_fill_cell(&self, x: u32, y: u32) -> bool {
        let tpc = self.texels_per_cell as usize;
        let idx = (y as usize * tpc) * self.texel_width() + x as usize * tpc;
        self.fill.get(idx).copied().unwrap_or(false)
    }

    /// The exact distance (cells) from (x, y) to the nearest centreline; infinity without one.
    /// The tests check the invariants with it, since the texture is quantised.
    pub fn distance(&self, x: f32, y: f32) -> f32 {
        self.polylines.iter().fold(f32::INFINITY, |best, line| {
            let pts = &line.points;
            let m = pts.len();
            if m == 0 {
                return best;
            }
            if m == 1 {
                return best.min(seg_dist((x, y), pts[0], pts[0]));
            }
            let count = if line.closed { m } else { m - 1 };
            (0..count).fold(best, |b, i| b.min(seg_dist((x, y), pts[i], pts[(i + 1) % m])))
        })
    }
}

/// Bakes the winding paths of a `width`×`height` map.
///
/// `codes` is one surface code per cell, row by row; a missing code counts as not a path.
/// `is_path` says which codes are road. `texels_per_cell` (at least 1) sets the texture's
/// resolution: the texture is `width * texels_per_cell` by `height * texels_per_cell` texels.
pub fn bake_paths(
    codes: &[u8],
    width: u32,
    height: u32,
    is_path: impl Fn(u8) -> bool,
    texels_per_cell: u32,
) -> PathBake {
    let (w, h) = (width as usize, height as usize);
    let n = w * h;
    let tpc = texels_per_cell.max(1) as usize;
    let path: Vec<bool> = (0..n).map(|i| codes.get(i).is_some_and(|&c| is_path(c))).collect();
    let fill = fill_cells(&path, w, h);
    let links = links(&path, w, h);
    let pinned: Vec<bool> = (0..n).map(|i| path[i] && (links[i].count_ones() != 2 || fill[i])).collect();

    let grid = Grid { path: &path, w, h };
    let mut pieces: Vec<Piece> = Vec::new();
    let mut used = vec![0u8; n];

    // Chains between pinned nodes. Each is walked once: the walk marks the link at both ends.
    for s in (0..n).filter(|&s| pinned[s]) {
        if links[s] == 0 {
            fit(&[centre(s, w)], false, &grid, &mut pieces);
            continue;
        }
        for d in 0..8 {
            if !has(links[s], d) || has(used[s], d) {
                continue;
            }
            mark(&mut used, s, d, w);
            let mut nodes = vec![centre(s, w)];
            let mut cur = step(s, d, w);
            let mut back = opp(d);
            loop {
                nodes.push(centre(cur, w));
                if pinned[cur] {
                    break;
                }
                let e = other_link(links[cur], back);
                mark(&mut used, cur, e, w);
                back = opp(e);
                cur = step(cur, e, w);
            }
            fit(&nodes, false, &grid, &mut pieces);
        }
    }

    // What is left are closed loops of unpinned nodes (no junction, no end).
    for s in (0..n).filter(|&s| path[s] && !pinned[s]) {
        let free = links[s] & !used[s];
        let Some(d0) = (0..8).find(|&d| has(free, d)) else {
            continue;
        };
        mark(&mut used, s, d0, w);
        let mut nodes = vec![centre(s, w)];
        let mut cur = step(s, d0, w);
        let mut back = opp(d0);
        while cur != s {
            nodes.push(centre(cur, w));
            let e = other_link(links[cur], back);
            mark(&mut used, cur, e, w);
            back = opp(e);
            cur = step(cur, e, w);
        }
        fit(&nodes, true, &grid, &mut pieces);
    }

    let mut segs: Vec<Seg> = Vec::new();
    let mut polylines = Vec::with_capacity(pieces.len());
    for piece in &pieces {
        segs.extend(segments(&piece.pts, piece.closed));
        polylines.push(Polyline {
            points: piece.pts.iter().map(|p| (p[0], p[1])).collect(),
            closed: piece.closed,
        });
    }

    // Buckets: the segments that can lie within MAX_DIST of each cell.
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (k, s) in segs.iter().enumerate() {
        let (x0, x1) = cells_between(s.a.0.min(s.b.0) - MAX_DIST, s.a.0.max(s.b.0) + MAX_DIST, w);
        let (y0, y1) = cells_between(s.a.1.min(s.b.1) - MAX_DIST, s.a.1.max(s.b.1) + MAX_DIST, h);
        for cy in y0..y1 {
            for cx in x0..x1 {
                buckets[cy * w + cx].push(k as u32);
            }
        }
    }

    // The fill's soft edges: unit segments between a fill cell and a cell that is neither fill nor road (or the map's
    // outside), each owned by its fill cell, so a texel finds them in its 3×3 cells. A fill next to a road has no edge
    // there: the road runs on into the fill, and the centreline takes over.
    let mut edge_buckets: Vec<Vec<Pt>> = vec![Vec::new(); n];
    let mut near_fill = vec![false; n];
    for y in 0..h {
        for x in 0..w {
            if !fill[y * w + x] {
                continue;
            }
            let not_fill = |dx: i32, dy: i32| {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 || {
                    let j = ny as usize * w + nx as usize;
                    !fill[j] && !path[j]
                }
            };
            let (xf, yf) = (x as f32, y as f32);
            let owner = &mut edge_buckets[y * w + x];
            if not_fill(-1, 0) {
                owner.push((xf, yf));
                owner.push((xf, yf + 1.0));
            }
            if not_fill(1, 0) {
                owner.push((xf + 1.0, yf));
                owner.push((xf + 1.0, yf + 1.0));
            }
            if not_fill(0, -1) {
                owner.push((xf, yf));
                owner.push((xf + 1.0, yf));
            }
            if not_fill(0, 1) {
                owner.push((xf, yf + 1.0));
                owner.push((xf + 1.0, yf + 1.0));
            }
            for dy in -1..=1i32 {
                for dx in -1..=1i32 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32 {
                        near_fill[ny as usize * w + nx as usize] = true;
                    }
                }
            }
        }
    }

    let (tw, th) = (w * tpc, h * tpc);
    let tpcf = tpc as f32;
    let mut dist = vec![0u8; tw * th];
    let mut texel_fill = vec![false; tw * th];
    for ty in 0..th {
        for tx in 0..tw {
            let cell = (ty / tpc) * w + tx / tpc;
            let t = ty * tw + tx;
            let p = ((tx as f32 + 0.5) / tpcf, (ty as f32 + 0.5) / tpcf);
            // A fill is solid road: its texels take the fill's distance only (0 inside, a soft ramp at its edge). Any other
            // texel takes its centreline's distance, and the fill's when a fill is near (the smaller one is the path's).
            let best = if fill[cell] {
                texel_fill[t] = true;
                fill_distance(p, true, cell, w, h, &edge_buckets)
            } else {
                let mut b = buckets[cell].iter().fold(MAX_DIST, |b, &k| {
                    let s = segs[k as usize];
                    b.min(seg_dist(p, s.a, s.b))
                });
                if near_fill[cell] {
                    b = b.min(fill_distance(p, false, cell, w, h, &edge_buckets));
                }
                b
            };
            dist[t] = (best.min(MAX_DIST) / MAX_DIST * 255.0).round() as u8;
        }
    }

    PathBake {
        cells_w: width,
        cells_h: height,
        texels_per_cell: tpc as u32,
        dist,
        fill: texel_fill,
        polylines,
    }
}

/// The baked winding paths of a `width`×`height` map at the map's own resolution (`texels_per_cell_for`), or `None`
/// when a side is over `MAX_BAKED_SIDE` cells: such a map keeps today's road look.
pub fn bake_for_map(codes: &[u8], width: u32, height: u32) -> Option<PathBake> {
    if width == 0 || height == 0 || width > MAX_BAKED_SIDE || height > MAX_BAKED_SIDE {
        return None;
    }
    Some(bake_paths(codes, width, height, |c| c == ROAD_CODE, texels_per_cell_for(width, height)))
}

/// The surface code of a road (`Surface::Road`).
pub const ROAD_CODE: u8 = 4;

/// One byte per cell, row by row: 255 when no cell of the cell's 3×3 block is blocked (`is_blocked_code`), else 0. A road
/// narrows only where its byte is 255, so a road by water, lava or swamp keeps its full width (the shader fades it).
pub fn safe_cells(codes: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as i32, height as i32);
    let blocked = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && codes.get((y * w + x) as usize).is_some_and(|&c| is_blocked_code(c));
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| {
            let near = (-1..=1).any(|dy| (-1..=1).any(|dx| blocked(x + dx, y + dy)));
            if near {
                0
            } else {
                255
            }
        })
        .collect()
}

/// The bake as RGBA8 bytes, one pixel per texel, row by row: red is the distance, green 255 in
/// fill, blue 0, alpha 255. For debugging dumps.
pub fn to_rgba(bake: &PathBake) -> Vec<u8> {
    bake.dist
        .iter()
        .zip(&bake.fill)
        .flat_map(|(&d, &f)| [d, if f { 255 } else { 0 }, 0, 255])
        .collect()
}

/// The bake as a PNG (RGBA, 8 bits): the same pixels as [`to_rgba`].
pub fn to_png_rgba(bake: &PathBake) -> Result<Vec<u8>, png::EncodingError> {
    let tw = bake.cells_w * bake.texels_per_cell;
    let th = bake.cells_h * bake.texels_per_cell;
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, tw, th);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header()?;
        writer.write_image_data(&to_rgba(bake))?;
    }
    Ok(out)
}

/// A chain's smoothed points, after [`fit`].
struct Piece {
    pts: Vec<P3>,
    closed: bool,
}

/// One segment of a smoothed centreline; `t` is its start's parameter (sorted along a chain).
#[derive(Clone, Copy, Debug)]
struct Seg {
    a: Pt,
    b: Pt,
    t: f32,
}

/// The path mask of the map, for the non-path checks.
struct Grid<'a> {
    path: &'a [bool],
    w: usize,
    h: usize,
}

impl Grid<'_> {
    /// Whether a non-path cell's centre lies within `FAR_FROM_PATH` of one of `segs`.
    fn near_path(&self, segs: &[Seg]) -> bool {
        segs.iter().any(|s| {
            let (x0, x1) = (s.a.0.min(s.b.0) - 0.8, s.a.0.max(s.b.0) + 0.8);
            let (y0, y1) = (s.a.1.min(s.b.1) - 0.8, s.a.1.max(s.b.1) + 0.8);
            let (cx0, cx1) = cells_between(x0, x1, self.w);
            let (cy0, cy1) = cells_between(y0, y1, self.h);
            (cy0..cy1).any(|cy| {
                (cx0..cx1).any(|cx| {
                    !self.path[cy * self.w + cx]
                        && seg_dist(((cx as f32) + 0.5, (cy as f32) + 0.5), s.a, s.b) <= FAR_FROM_PATH
                })
            })
        })
    }
}

/// Smooths one chain (`pts`: its node centres, an open chain with pinned ends or a closed loop)
/// into pieces that keep the invariants, and appends them to `out`.
///
/// Iteration counts 3, 2, 1 are tried on the whole chain. A count is kept when every interior
/// node lies within `MIN_HALF_WIDTH` of the curve (checked on the segments near each node's
/// parameter, which can only over-report) and no non-path centre lies within `FAR_FROM_PATH`.
///
/// Corner cutting pulls the curve away from a corner's centre. A right-angle L-turn keeps its
/// centre within 0.177 cells at one and two iterations, and falls outside the limit at three.
/// A sharp corner (for example two diagonal links at 90°, offset 0.25 at one iteration) fails at
/// every count: at one iteration the first failing node is pinned, which splits the chain there
/// (the node becomes an end, so the curve passes through it exactly), and both halves are fitted
/// again. A loop is opened at the pinned node.
///
/// If a non-path centre comes too close at one iteration, the chain keeps its raw segments. Those
/// always pass: they join centres of adjacent cells, so every non-path centre is at least 0.707
/// from them.
fn fit(pts: &[Pt], closed: bool, grid: &Grid<'_>, out: &mut Vec<Piece>) {
    let base: Vec<P3> = pts.iter().enumerate().map(|(i, p)| [p.0, p.1, i as f32]).collect();
    let period = pts.len() as f32;
    for k in (1..=MAX_ITER).rev() {
        let poly = smooth(&base, closed, period, k);
        let segs = segments(&poly, closed);
        if let Some(v) = node_violation(&base, closed, period, &segs) {
            if k == 1 {
                split(pts, closed, v, grid, out);
                return;
            }
            continue;
        }
        if grid.near_path(&segs) {
            continue;
        }
        out.push(Piece { pts: poly, closed });
        return;
    }
    out.push(Piece { pts: base, closed });
}

/// Splits a chain at node `v` (pinned from now on) and fits both halves. A loop becomes an open
/// chain that starts and ends at `v`.
fn split(pts: &[Pt], closed: bool, v: usize, grid: &Grid<'_>, out: &mut Vec<Piece>) {
    if closed {
        let n = pts.len();
        let open: Vec<Pt> = (0..=n).map(|j| pts[(v + j) % n]).collect();
        fit(&open, false, grid, out);
    } else {
        fit(&pts[..=v], false, grid, out);
        fit(&pts[v..], false, grid, out);
    }
}

/// The first interior node (of an open chain) or node (of a loop) whose centre lies farther than
/// `MIN_HALF_WIDTH` from the curve, checked against the segments whose parameter is within
/// `WINDOW` of the node (and, for a loop, one period either way).
fn node_violation(base: &[P3], closed: bool, period: f32, segs: &[Seg]) -> Option<usize> {
    let n = base.len();
    let shifts: Vec<f32> = if closed { vec![-period, 0.0, period] } else { vec![0.0] };
    let interior = if closed { 0..n } else { 1..n.saturating_sub(1) };
    interior.into_iter().find(|&i| {
        let p = (base[i][0], base[i][1]);
        let best = shifts.iter().fold(f32::INFINITY, |best, &sh| {
            let c = i as f32 + sh;
            let lo = segs.partition_point(|s| s.t < c - WINDOW);
            let hi = segs.partition_point(|s| s.t <= c + WINDOW);
            segs[lo..hi].iter().fold(best, |b, s| b.min(seg_dist(p, s.a, s.b)))
        });
        best > MIN_HALF_WIDTH
    })
}

/// One Chaikin step on an open chain: the two ends stay put.
fn step_open(p: &[P3]) -> Vec<P3> {
    let n = p.len();
    if n < 3 {
        return p.to_vec();
    }
    let mut out = vec![p[0]];
    for (i, pair) in p.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        if i > 0 {
            out.push(lerp3(a, b, 0.25));
        }
        if i + 2 < n {
            out.push(lerp3(a, b, 0.75));
        }
    }
    out.push(p[n - 1]);
    out
}

/// One Chaikin step on a closed loop. `period` is the loop's length in parameter units (the
/// original node count): the closing edge runs on into the next turn, so the parameter stays
/// increasing along the loop.
fn step_closed(p: &[P3], period: f32) -> Vec<P3> {
    let n = p.len();
    if n < 3 {
        return p.to_vec();
    }
    let mut out = Vec::with_capacity(2 * n);
    for (i, &a) in p.iter().enumerate() {
        let mut b = p[(i + 1) % n];
        if i + 1 == n {
            b[2] += period;
        }
        out.push(lerp3(a, b, 0.25));
        out.push(lerp3(a, b, 0.75));
    }
    out
}

/// `iterations` Chaikin steps.
fn smooth(base: &[P3], closed: bool, period: f32, iterations: u32) -> Vec<P3> {
    let mut poly = base.to_vec();
    for _ in 0..iterations {
        poly = if closed { step_closed(&poly, period) } else { step_open(&poly) };
    }
    poly
}

fn lerp3(a: P3, b: P3, s: f32) -> P3 {
    [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, a[2] + (b[2] - a[2]) * s]
}

/// The segments of a polyline (one degenerate segment for a single point).
fn segments(poly: &[P3], closed: bool) -> Vec<Seg> {
    let m = poly.len();
    let at = |i: usize| (poly[i][0], poly[i][1]);
    match m {
        0 => Vec::new(),
        1 => vec![Seg { a: at(0), b: at(0), t: poly[0][2] }],
        _ if closed => (0..m).map(|i| Seg { a: at(i), b: at((i + 1) % m), t: poly[i][2] }).collect(),
        _ => (0..m - 1).map(|i| Seg { a: at(i), b: at(i + 1), t: poly[i][2] }).collect(),
    }
}

/// The distance from `p` to the segment `a`–`b` (to `a` when the segment has no length).
fn seg_dist(p: Pt, a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (ex, ey) = (a.0 + dx * t - p.0, a.1 + dy * t - p.1);
    (ex * ex + ey * ey).sqrt()
}

/// The half-open cell range `lo..hi` (in cell units) that a span touches, clamped to `n` cells.
fn cells_between(lo: f32, hi: f32, n: usize) -> (usize, usize) {
    let a = lo.floor().clamp(0.0, n as f32) as usize;
    let b = (hi.floor() + 1.0).clamp(0.0, n as f32) as usize;
    (a, b)
}

/// The distance (cells) that a texel of cell `cell` has from a fill, for the path mask: `FILL_INSET` minus the distance
/// to the fill's edge inside the fill (0 from `FILL_INSET` in), `FILL_INSET` plus the distance outside. So a fill's
/// edge is a soft ramp inside its outermost cells, and a path that runs into a square merges into it.
/// `edges` holds each fill cell's unit edges (`bake_paths`); the search is the texel's 3×3 cells, enough for `MAX_DIST`.
fn fill_distance(p: Pt, inside: bool, cell: usize, w: usize, h: usize, edges: &[Vec<Pt>]) -> f32 {
    let (cx, cy) = ((cell % w) as i32, (cell / w) as i32);
    let mut g = MAX_DIST;
    for dy in -1..=1i32 {
        for dx in -1..=1i32 {
            let (nx, ny) = (cx + dx, cy + dy);
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                continue;
            }
            for e in edges[ny as usize * w + nx as usize].chunks_exact(2) {
                g = g.min(seg_dist(p, e[0], e[1]));
            }
        }
    }
    let f = if inside { FILL_INSET - g } else { FILL_INSET + g };
    f.max(0.0)
}

/// The fill cells: every cell of an all-path 2×2 block.
fn fill_cells(path: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut fill = vec![false; w * h];
    for y in 0..h.saturating_sub(1) {
        for x in 0..w.saturating_sub(1) {
            let i = y * w + x;
            if path[i] && path[i + 1] && path[i + w] && path[i + w + 1] {
                for j in [i, i + 1, i + w, i + w + 1] {
                    fill[j] = true;
                }
            }
        }
    }
    fill
}

/// The link mask of every path cell (bit d set when the neighbour in direction d is linked).
fn links(path: &[bool], w: usize, h: usize) -> Vec<u8> {
    let mut links = vec![0u8; w * h];
    for (i, &on) in path.iter().enumerate() {
        if !on {
            continue;
        }
        let (cx, cy) = ((i % w) as i32, (i / w) as i32);
        for (d, &(dx, dy)) in DIRS.iter().enumerate() {
            let (x, y) = (cx + dx, cy + dy);
            if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                continue;
            }
            let (x, y) = (x as usize, y as usize);
            if !path[y * w + x] {
                continue;
            }
            // A diagonal step is a link only when neither orthogonal cell between the two is road.
            if d >= 4 && (path[(i / w) * w + x] || path[y * w + i % w]) {
                continue;
            }
            links[i] |= 1u8 << d;
        }
    }
    links
}

/// Whether bit `d` of a link mask is set.
fn has(mask: u8, d: usize) -> bool {
    (mask >> d) & 1 == 1
}

/// The direction opposite to `d`.
fn opp(d: usize) -> usize {
    if d < 4 {
        (d + 2) % 4
    } else {
        4 + (d - 4 + 2) % 4
    }
}

/// The cell one step from `i` in direction `d` (the step must stay on the map).
fn step(i: usize, d: usize, w: usize) -> usize {
    let (dx, dy) = DIRS[d];
    let x = (i % w) as i32 + dx;
    let y = (i / w) as i32 + dy;
    y as usize * w + x as usize
}

/// Marks the link from `i` in direction `d` as walked, at both ends.
fn mark(used: &mut [u8], i: usize, d: usize, w: usize) {
    used[i] |= 1u8 << d;
    used[step(i, d, w)] |= 1u8 << opp(d);
}

/// The link of a degree-2 cell other than the one it was entered by (`back`).
fn other_link(mask: u8, back: usize) -> usize {
    (0..8).find(|&d| has(mask, d) && d != back).unwrap_or(back)
}

/// The centre of cell `i`, in cell units.
fn centre(i: usize, w: usize) -> Pt {
    ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROAD: u8 = 4;
    const GRASS: u8 = 5;

    /// The codes of a map drawn as rows of `#` (road) and `.` (grass).
    fn codes_of(rows: &[&str]) -> (Vec<u8>, u32, u32) {
        let codes: Vec<u8> = rows
            .iter()
            .flat_map(|r| r.bytes().map(|b| if b == b'#' { ROAD } else { GRASS }))
            .collect();
        (codes, rows[0].len() as u32, rows.len() as u32)
    }

    fn bake_of(codes: &[u8], w: u32, h: u32) -> PathBake {
        bake_paths(codes, w, h, |c| c == ROAD, 4)
    }

    /// Every cell that breaks the invariants, on the exact centrelines (as messages).
    fn invariant_violations(codes: &[u8], w: u32, h: u32, bake: &PathBake) -> Vec<String> {
        let mut bad = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let d = bake.distance(x as f32 + 0.5, y as f32 + 0.5);
                let fill = bake.is_fill_cell(x, y);
                if codes[(y * w + x) as usize] == ROAD {
                    if !(fill || d <= MIN_HALF_WIDTH) {
                        bad.push(format!("road cell ({x}, {y}) lies {d} from a centreline"));
                    }
                } else if fill || d <= FAR_FROM_PATH {
                    bad.push(format!("grass cell ({x}, {y}) lies {d} from a centreline (fill {fill})"));
                }
            }
        }
        bad
    }

    /// The invariants over every cell, on the exact centrelines.
    fn check_invariants(codes: &[u8], w: u32, h: u32, bake: &PathBake) {
        let bad = invariant_violations(codes, w, h, bake);
        assert!(bad.is_empty(), "{} cells break the invariants, first: {}", bad.len(), bad[0]);
    }

    /// Bakes rows of `#` and `.`, checks the invariants, and returns the bake.
    fn check_rows(rows: &[&str]) -> PathBake {
        let (codes, w, h) = codes_of(rows);
        let bake = bake_of(&codes, w, h);
        check_invariants(&codes, w, h, &bake);
        bake
    }

    fn lcg(seed: &mut u64) -> u64 {
        *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        *seed >> 33
    }

    #[test]
    fn straight_road_is_its_centreline() {
        let bake = check_rows(&["#####", "....."]);
        assert!(bake.distance(2.5, 0.5) < 1e-4);
        assert!(bake.distance(0.5, 0.5) < 1e-4);
        assert!(!bake.is_fill_cell(2, 0));
    }

    #[test]
    fn right_angle_turn_is_a_curve() {
        let bake = check_rows(&["#.", "##"]);
        assert_eq!(bake.polylines.len(), 1);
        assert!(bake.polylines[0].points.len() > 3, "the turn was not smoothed");
        // The corner's centre is off the curve.
        assert!(bake.distance(0.5, 1.5) > 0.05);
    }

    #[test]
    fn staircase_reads_as_a_diagonal() {
        let bake = check_rows(&[
            "##.....",
            ".##....",
            "..##...",
            "...##..",
            "....##.",
            ".....##",
            "......#",
        ]);
        // The link midpoints lie on x - y = 0.5. Raw steps sit 0.354 off it; the curve must stay
        // within 0.2 away from the pinned ends (x + y in 2.5..=10.5).
        for line in &bake.polylines {
            for &(x, y) in &line.points {
                if (2.5..=10.5).contains(&(x + y)) {
                    let dev = (x - y - 0.5).abs() / std::f32::consts::SQRT_2;
                    assert!(dev <= 0.2, "point ({x}, {y}) is {dev} off the diagonal");
                }
            }
        }
    }

    #[test]
    fn t_junction_keeps_the_invariants() {
        check_rows(&["###", ".#.", ".#."]);
    }

    #[test]
    fn cross_keeps_the_invariants() {
        check_rows(&[".#.", "###", ".#."]);
    }

    #[test]
    fn zigzag_dead_ends_keep_the_invariants() {
        check_rows(&["#.", "##", ".#"]);
    }

    #[test]
    fn two_by_two_block_is_filled() {
        let bake = check_rows(&["##.", "##.", "..."]);
        for y in 0..2 {
            for x in 0..2 {
                assert!(bake.is_fill_cell(x, y));
            }
        }
        assert!(!bake.is_fill_cell(2, 0));
        // The block's middle is road (distance 0); its edge is a soft ramp, not a hard one: the texel just inside the
        // edge is part-way, and the texel just outside it is farther from the road.
        let tpc = bake.texels_per_cell as usize;
        let tw = bake.cells_w as usize * tpc;
        let at = |tx: usize, ty: usize| bake.dist[ty * tw + tx];
        assert!(bake.dist.iter().zip(&bake.fill).any(|(&d, &f)| f && d == 0), "the block has a road middle");
        let inside_edge = at(2 * tpc - 1, tpc);
        assert!(inside_edge > 0 && inside_edge < 255, "inside edge {inside_edge}");
        assert!(at(2 * tpc, tpc) > inside_edge, "the ramp rises outside the block");
    }

    #[test]
    fn closed_loop_is_one_closed_polyline() {
        let bake = check_rows(&["###", "#.#", "###"]);
        assert!(bake.polylines.iter().any(|l| l.closed));
    }

    #[test]
    fn diagonal_only_link_stays_a_diagonal() {
        let bake = check_rows(&["#.", ".#"]);
        assert!(bake.distance(1.5, 0.5) > FAR_FROM_PATH);
        // The midpoint of the link, between the centres (0.5, 0.5) and (1.5, 1.5).
        assert!(bake.distance(1.0, 1.0) < 1e-4);
    }

    #[test]
    fn lone_cell_is_a_point() {
        let bake = check_rows(&["...", ".#.", "..."]);
        assert!(bake.distance(1.5, 1.5) < 1e-4);
    }

    #[test]
    fn random_map_keeps_the_invariants() {
        let mut seed = 0x5eed_u64;
        let (w, h) = (40u32, 40u32);
        let codes: Vec<u8> = (0..w * h)
            .map(|_| if lcg(&mut seed) % 100 < 40 { ROAD } else { GRASS })
            .collect();
        let bake = bake_of(&codes, w, h);
        check_invariants(&codes, w, h, &bake);
    }

    #[test]
    fn rgba_dump_has_one_pixel_per_texel() {
        let bake = check_rows(&["##", ".."]);
        let rgba = to_rgba(&bake);
        assert_eq!(rgba.len(), bake.dist.len() * 4);
        assert_eq!(rgba[3], 255);
    }

    /// Terrain colours for the dumps, by surface code.
    const PALETTE: [[u8; 3]; 16] = [
        [70, 140, 200],
        [40, 100, 170],
        [20, 50, 110],
        [220, 80, 20],
        [200, 170, 110],
        [110, 170, 70],
        [150, 190, 90],
        [200, 190, 120],
        [90, 120, 80],
        [50, 70, 50],
        [230, 210, 150],
        [160, 110, 80],
        [130, 130, 130],
        [60, 50, 45],
        [235, 240, 245],
        [190, 200, 220],
    ];

    fn write_png(path: &std::path::Path, w: u32, h: u32, rgb: &[u8]) {
        let file = std::fs::File::create(path).expect("create the dump");
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().expect("png header").write_image_data(rgb).expect("png data");
    }

    /// Two dumps of a bake: `<stem>-terrain.png` (terrain colours, the centrelines in red) and
    /// `<stem>-distance.png` (the distance field in grey, 0 in fill).
    fn dump(dir: &std::path::Path, stem: &str, codes: &[u8], w: u32, h: u32, bake: &PathBake) {
        let tpc = bake.texels_per_cell as usize;
        let (tw, th) = (w as usize * tpc, h as usize * tpc);
        let mut colour: Vec<u8> = (0..tw * th)
            .flat_map(|t| {
                let (tx, ty) = (t % tw, t / tw);
                let code = codes[(ty / tpc) * w as usize + tx / tpc] as usize;
                PALETTE[code.min(15)]
            })
            .collect();
        for line in &bake.polylines {
            let m = line.points.len();
            let count = if line.closed { m } else { m.saturating_sub(1) };
            for i in 0..count {
                let (a, b) = (line.points[i], line.points[(i + 1) % m]);
                let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                let steps = (len * tpc as f32 * 2.0).ceil().max(1.0) as usize;
                for s in 0..=steps {
                    let f = s as f32 / steps as f32;
                    let x = ((a.0 + (b.0 - a.0) * f) * tpc as f32) as usize;
                    let y = ((a.1 + (b.1 - a.1) * f) * tpc as f32) as usize;
                    if x < tw && y < th {
                        colour[(y * tw + x) * 3..][..3].copy_from_slice(&[255, 0, 0]);
                    }
                }
            }
        }
        write_png(&dir.join(format!("{stem}-terrain.png")), tw as u32, th as u32, &colour);
        let grey: Vec<u8> = bake.dist.iter().flat_map(|&d| [d, d, d]).collect();
        write_png(&dir.join(format!("{stem}-distance.png")), tw as u32, th as u32, &grey);
    }

    /// The real maps (skipped unless `RAZDOR_DT_DIR` is set; the maps are read at runtime and never
    /// committed). Every map of the install, and of each install under `RAZDOR_COMMUNITY_DIR`
    /// (`<entry>/install`), bakes its roads at 8 texels per cell and keeps the invariants on every
    /// cell. Each map's bake time is printed (run with `--nocapture`). With `RAZDOR_ROAD_DUMPS` set,
    /// the three named maps and the map with the most roads of the `RAZDOR_DT_DIR` install are
    /// written there as PNG (the community installs are checked, not dumped).
    #[test]
    fn real_maps_keep_the_invariants() {
        use razdor::dt::install::{DtInstall, ENV_VAR};
        use std::path::PathBuf;

        const NAMED: [&str; 3] = ["РК1-Начало пути", "Другой берег", "ДС2-Страшная история"];

        let Some(dir) = std::env::var_os(ENV_VAR) else { return };
        let mut installs = vec![PathBuf::from(dir)];
        if let Some(root) = std::env::var_os("RAZDOR_COMMUNITY_DIR") {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(root)
                .expect("the community folder")
                .flatten()
                .map(|e| e.path().join("install"))
                .filter(|p| p.is_dir())
                .collect();
            installs.append(&mut entries);
        }
        let mut seen = Vec::new();
        installs.retain(|p| {
            let key = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            let new = !seen.contains(&key);
            seen.push(key);
            new
        });
        let dumps = std::env::var_os("RAZDOR_ROAD_DUMPS").map(PathBuf::from);

        let mut maps_baked = 0usize;
        let mut largest = (0u64, String::new());
        let mut slowest = (0f64, String::new());
        let mut failures: Vec<String> = Vec::new();
        for (i, inst) in installs.iter().enumerate() {
            let dt = DtInstall::load(inst).expect("the install loads");
            let maps: Vec<(String, u32, u32, Vec<u8>)> = dt
                .maps
                .iter()
                .map(|m| {
                    let s = m.load().expect("the map loads");
                    (m.name.clone(), s.header.width, s.header.height, s.terrain)
                })
                .collect();
            let most_roads = maps
                .iter()
                .max_by_key(|(_, _, _, codes)| codes.iter().filter(|&&c| c == ROAD).count())
                .map(|m| m.0.clone());
            for (name, w, h, codes) in &maps {
                let roads = codes.iter().filter(|&&c| c == ROAD).count();
                let cells = u64::from(*w) * u64::from(*h);
                if cells > largest.0 {
                    largest = (cells, format!("{} / {name} {w}x{h}", inst.display()));
                }
                let t0 = std::time::Instant::now();
                let bake = bake_paths(codes, *w, *h, |c| c == ROAD, 8);
                let ms = t0.elapsed().as_secs_f64() * 1e3;
                if ms > slowest.0 {
                    slowest = (ms, format!("{name} {w}x{h}"));
                }
                maps_baked += 1;
                let bad = invariant_violations(codes, *w, *h, &bake);
                println!(
                    "{name:<32} {w:>4}x{h:<4} roads {roads:>6} lines {:>5} bake {ms:>9.1} ms {}",
                    bake.polylines.len(),
                    if bad.is_empty() { "ok" } else { "FAIL" }
                );
                if let Some(first) = bad.first() {
                    failures.push(format!("{} / {name}: {} cells, first: {first}", inst.display(), bad.len()));
                }
                if let Some(d) = dumps.as_ref().filter(|_| i == 0) {
                    if NAMED.contains(&name.as_str()) || Some(name) == most_roads.as_ref() {
                        dump(d, &format!("{i}-{name}"), codes, *w, *h, &bake);
                    }
                }
            }
        }
        println!("baked {maps_baked} maps; largest {} cells ({}); slowest bake {:.1} ms ({})", largest.0, largest.1, slowest.0, slowest.1);
        assert!(failures.is_empty(), "invariants broken:\n{}", failures.join("\n"));
    }
}
