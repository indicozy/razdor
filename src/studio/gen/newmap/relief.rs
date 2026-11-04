//! The relief (newmap.md §3–§6): the midpoint fractal, the blur, the cut into height
//! classes, the water of the map type and the rivers.

use std::collections::HashMap;

use super::ext::{self, round_div, Ext};
use super::{Ctx, Shares, Step, Stop, ARCHIPELAGO, COAST, ESTUARY, ISLAND, LAKE, SKERRIES, VALLEY};

/// The work grid's row length (the original allocates 801 × 801 records).
pub const STRIDE: usize = 801;

/// The dialog's work grid: per cell the height, the working value, and the feature radius
/// and kind the stamps write. It lives as long as the dialog.
#[derive(Clone, Debug)]
pub struct Grid {
    pub h: Vec<i32>,
    pub v: Vec<i32>,
    pub r: Vec<i16>,
    pub kind: Vec<u8>,
}

impl Grid {
    pub fn new() -> Grid {
        let n = STRIDE * STRIDE;
        Grid { h: vec![0; n], v: vec![0; n], r: vec![0; n], kind: vec![0; n] }
    }
}

impl Default for Grid {
    fn default() -> Self {
        Grid::new()
    }
}

pub fn gi(x: i32, y: i32) -> usize {
    y as usize * STRIDE + x as usize
}

/// Feature kinds the stamps write.
pub const RIVER: u8 = 2;
pub const SEA: u8 = 3;

/// The class bounds of a cut: records 0–9 of the original, `low` and `high` heights.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    pub low: [i32; 10],
    pub high: [i32; 10],
}

impl Bounds {
    /// Whether height `h` is in class `i`.
    pub fn holds(&self, i: usize, h: i32) -> bool {
        self.low[i] <= h && h <= self.high[i]
    }
}

/// §3–§6 in order; returns the cut the terrain classes use. With `relief` off the heights
/// the grid holds are cut as they are (§10.1): the original jumps past the water and the
/// rivers too (0x51f4d5 to 0x5210da), so the map type does not show in a reused relief.
pub fn relief(c: &mut Ctx, relief: bool) -> Step<Bounds> {
    if !relief {
        return levels(c, 1);
    }
    // §3.1 (the map cells were cleared by the caller) and the forest's progress divisor.
    c.grid.h.iter_mut().for_each(|v| *v = 0);
    c.grid.v.iter_mut().for_each(|v| *v = 0);
    c.grid.r.iter_mut().for_each(|v| *v = 0);
    c.grid.kind.iter_mut().for_each(|v| *v = 0);
    *c.divisor = (c.w / 50) * (c.w / 50) * 30;
    midpoint(c, 0, 0, c.w - 1, c.h - 1, 1, 1)?;
    blur(c, c.o.blur as i32);
    copy_values(c);
    levels(c, 1)?;
    let kind = c.o.kind;
    if kind == LAKE || (ESTUARY..=ARCHIPELAGO).contains(&kind) {
        sea_stamps(c);
        apply_feature(c, SEA, 0)?;
    }
    let b = levels(c, 1)?;
    // §5.2: the lower bound of the coastal water is the rivers' level.
    let t = b.low[2];
    match kind {
        VALLEY => valley(c),
        ESTUARY => estuary(c),
        SKERRIES => skerries(c),
        _ => {}
    }
    if kind == VALLEY || kind == ESTUARY {
        apply_feature(c, RIVER, t)?;
    }
    levels(c, 1)
}

// --- §3.2 the fractal --------------------------------------------------------------------

const AMPLITUDE: [[i32; 8]; 2] = [[0, 200, 400, 400, 600, 600, 600, 600], [100, 200, 300, 200, 100, 0, 0, 0]];

fn level(d: i32) -> usize {
    match d {
        ..=2 => 1,
        3..=5 => 2,
        6..=13 => 3,
        14..=29 => 4,
        30..=59 => 5,
        60..=119 => 6,
        120..=252 => 7,
        _ => 8,
    }
}

/// The midpoint displacement (0x51d7d8) on the working values: mode 1 for the relief, 2 for
/// the forest field.
pub fn midpoint(c: &mut Ctx, x1: i32, y1: i32, x2: i32, y2: i32, mode: usize, depth: u32) -> Step<()> {
    let a = AMPLITUDE[mode - 1][level(x2 - x1) - 1];
    let v = |c: &Ctx, x: i32, y: i32| c.grid.v[gi(x, y)];
    // A draw even for an amplitude of 0.
    let j1 = c.rng.random(a) - (a >> 1);
    let top = j1 + (v(c, x1, y1) + v(c, x2, y1)) / 2;
    let j2 = c.rng.random(a) - (a >> 1);
    let bottom = j2 + (v(c, x1, y2) + v(c, x2, y2)) / 2;
    let j3 = c.rng.random(a) - (a >> 1);
    let left = j3 + (v(c, x1, y1) + v(c, x1, y2)) / 2;
    let j4 = c.rng.random(a) - (a >> 1);
    let right = j4 + (v(c, x2, y1) + v(c, x2, y2)) / 2;
    let centre = (top + bottom + left + right) / 4;
    let (mx, my) = ((x1 + x2) / 2, (y1 + y2) / 2);
    // Only cells still holding 0 (a computed 0 can be overwritten later). One-cell quarters
    // still write, onto their own corners; the map's bottom-right corner is never written.
    for (x, y, val) in [(mx, y1, top), (x1, my, left), (mx, y2, bottom), (x2, my, right), (mx, my, centre)] {
        let i = gi(x, y);
        if c.grid.v[i] == 0 {
            c.grid.v[i] = val;
        }
    }
    if x2 - x1 != 1 && y2 - y1 != 1 {
        if depth == 6 {
            c.check()?;
        }
        midpoint(c, x1, y1, mx, my, mode, depth + 1)?;
        midpoint(c, mx, my, x2, y2, mode, depth + 1)?;
        midpoint(c, x1, my, mx, y2, mode, depth + 1)?;
        midpoint(c, mx, y1, x2, my, mode, depth + 1)?;
    }
    Ok(())
}

// --- §3.3 the blur -------------------------------------------------------------------------

/// One 3 × 3 pass, then `b − 1` passes of 5 × 5, each in place, row by row; a cell's own
/// value counts twice.
pub fn blur(c: &mut Ctx, b: i32) {
    if b >= 1 {
        blur_pass(c, 1);
        for _ in 1..b {
            blur_pass(c, 2);
        }
    }
}

fn blur_pass(c: &mut Ctx, r: i32) {
    let (w, h) = (c.w, c.h);
    for y in 0..h {
        for x in 0..w {
            let mut sum = c.grid.v[gi(x, y)];
            let mut n = 1;
            for dx in -r..=r {
                for dy in -r..=r {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && nx < w && ny >= 0 && ny < h {
                        n += 1;
                        sum += c.grid.v[gi(nx, ny)];
                    }
                }
            }
            c.grid.v[gi(x, y)] = sum / n;
        }
    }
}

/// The working values become the heights.
pub fn copy_values(c: &mut Ctx) {
    for y in 0..c.h {
        for x in 0..c.w {
            let i = gi(x, y);
            c.grid.h[i] = c.grid.v[i];
        }
    }
}

// --- §4 the cut ------------------------------------------------------------------------------

/// The class weights of a cut, lowest heights first.
pub fn weights(s: &Shares, mode: u8) -> Vec<i32> {
    if mode == 1 {
        let (w0, l0, l1) = (s.w0, s.l0(), s.l1());
        vec![
            w0 * s.w3,
            w0 * s.w2(),
            w0 * s.w1,
            s.sl * l0,
            s.ll * l1 * l0 / 97,
            s.lh * l1 * l0 / 97,
            s.lm() * l1 * l0 / 97,
            s.lr * l1 * l0 / 97,
        ]
    } else {
        vec![s.f0 * 100, s.f1() * 100, s.f2 * 100]
    }
}

/// The targets `cumᵢ` of a cut of `n` cells (§4 step 3): `Round((N ÷ S) × wᵢ)` added up in
/// extended precision, only the last one capped at N.
pub fn targets(n: i64, w: &[i32]) -> Step<Vec<i64>> {
    let sum: i64 = w.iter().map(|&v| v as i64).sum();
    let q = Ext::int(n).div(Ext::int(sum)).ok_or(Stop::NoShares)?;
    let mut cum = vec![0i64; w.len() + 1];
    for (i, &wi) in w.iter().enumerate() {
        cum[i + 1] = cum[i] + q.mul(Ext::int(wi as i64)).round_int();
    }
    if cum[w.len()] > n {
        cum[w.len()] = n;
    }
    Ok(cum)
}

/// The class bounds from a 1001-bin histogram and the targets (§4 step 4).
pub fn cut(hist: &[i64], cum: &[i64]) -> Bounds {
    let k = cum.len() - 1;
    let mut b = Bounds::default();
    b.high[0] = -1;
    let (mut prev, mut count) = (0i64, 0i64);
    for (v, &n) in hist.iter().enumerate() {
        if n <= 0 {
            continue;
        }
        count += n;
        for (i, &target) in cum.iter().enumerate().skip(1) {
            if prev <= target && target <= count {
                b.low[i] = b.high[i - 1] + 1;
                b.high[i] = v as i32;
                if b.high[i] < b.low[i] {
                    b.high[i] = b.low[i];
                }
            }
        }
        prev = count;
    }
    b.high[k] = 1000;
    b
}

/// The rescale to 0…999 and the cut (0x51df40): mode 1, eight relief classes; mode 2, three
/// forest classes. Clears the working values and the feature kinds.
pub fn levels(c: &mut Ctx, mode: u8) -> Step<Bounds> {
    let (mut max, mut min) = (-30000, 30000);
    for y in 0..c.h {
        for x in 0..c.w {
            let h = c.grid.h[gi(x, y)];
            max = max.max(h);
            min = min.min(h);
        }
    }
    if max <= min {
        // 0 ÷ 0: the run stops.
        return Err(Stop::FlatField);
    }
    let mut hist = vec![0i64; 1001];
    for y in 0..c.h {
        for x in 0..c.w {
            let i = gi(x, y);
            let h = round_div((c.grid.h[i] - min) as i64 * 999, (max - min) as i64) as i32;
            c.grid.h[i] = h;
            c.grid.v[i] = 0;
            c.grid.kind[i] = 0;
            hist[h as usize] += 1;
        }
    }
    let cum = targets(c.w as i64 * c.h as i64, &weights(&c.o.shares, mode))?;
    Ok(cut(&hist, &cum))
}

// --- §5 water --------------------------------------------------------------------------------

fn stamp(c: &mut Ctx, x: i32, y: i32, kind: u8, r: i32) {
    let i = gi(x, y);
    c.grid.kind[i] = kind;
    c.grid.r[i] = r as i16;
}

/// The line stamp (0x51d1a8): both ends marked, the decision taken on `e` before it moves.
pub fn line(c: &mut Ctx, (x1, y1): (i32, i32), (x2, y2): (i32, i32), kind: u8, r: i32) {
    let (ax, ay) = ((x2 - x1).abs(), (y2 - y1).abs());
    let (sx, sy) = ((x2 - x1).signum(), (y2 - y1).signum());
    let (mut x, mut y) = (x1, y1);
    stamp(c, x, y, kind, r);
    if ay < ax {
        let mut e = ax - 2 * ay;
        loop {
            x += sx;
            if e >= 0 {
                e -= 2 * ay;
            } else {
                e += 2 * ax - 2 * ay;
                y += sy;
            }
            stamp(c, x, y, kind, r);
            if x == x2 {
                break;
            }
        }
    } else {
        let mut e = ay - 2 * ax;
        loop {
            y += sy;
            if e >= 0 {
                e -= 2 * ax;
            } else {
                e += 2 * ay - 2 * ax;
                x += sx;
            }
            stamp(c, x, y, kind, r);
            if y == y2 {
                break;
            }
        }
    }
}

fn edge(c: &Ctx) -> ((i32, i32), (i32, i32)) {
    let (w, h) = (c.w, c.h);
    match c.o.orient {
        0 => ((0, 0), (w - 1, 0)),
        1 => ((0, h - 1), (w - 1, h - 1)),
        2 => ((0, 0), (0, h - 1)),
        _ => ((w - 1, 0), (w - 1, h - 1)),
    }
}

/// The sea stamps of the map type (§5.1).
pub fn sea_stamps(c: &mut Ctx) {
    let (w, h) = (c.w, c.h);
    match c.o.kind {
        LAKE => stamp(c, w >> 1, h >> 1, SEA, w >> 1),
        ESTUARY | COAST => {
            let (a, b) = edge(c);
            line(c, a, b, SEA, w / 5);
        }
        SKERRIES => {
            let (a, b) = edge(c);
            line(c, a, b, SEA, w >> 1);
        }
        ISLAND => {
            // A cell stamped twice keeps the later radius: (0, 0) ends with the edge's.
            line(c, (0, 0), (w - 1, 0), SEA, w / 5);
            stamp(c, 0, 0, SEA, w / 3);
            line(c, (w - 1, 0), (w - 1, h - 1), SEA, w / 5);
            stamp(c, w - 1, 0, SEA, w / 3);
            line(c, (w - 1, h - 1), (0, h - 1), SEA, w / 5);
            stamp(c, w - 1, h - 1, SEA, w / 3);
            line(c, (0, h - 1), (0, 0), SEA, w / 5);
            stamp(c, 0, h - 1, SEA, w / 3);
        }
        ARCHIPELAGO => {
            let n = (w / 50) * (w / 50) + 1;
            for _ in 0..n {
                let x = c.rng.random(w);
                let y = c.rng.random(h);
                stamp(c, x, y, SEA, w);
            }
        }
        _ => {}
    }
}

/// The weight of distance `d` from a point of radius `r` (0x51ee69), in the original's
/// order of extended operations; 0 from `2r` on.
pub fn weight(d: i32, r: i32) -> i64 {
    if d >= 2 * r {
        return 0;
    }
    let big = Ext::int(10_000);
    let dd = Ext::int(10_000 * d as i64);
    let f = Ext::int(1).div(dd.div(Ext::int(10 * r as i64)).unwrap_or(Ext::ZERO).add(big)).unwrap_or(Ext::ZERO);
    big.sub(dd.div(Ext::int(2 * r as i64)).unwrap_or(Ext::ZERO)).mul(f).mul(big).round_int()
}

/// Lowers the cells around the stamps of `kind` (0x51e88c): the sea everywhere, a river
/// only above the level `t`.
pub fn apply_feature(c: &mut Ctx, kind: u8, t: i32) -> Step<()> {
    let (w, h) = (c.w, c.h);
    let mut n = 0;
    for y in 0..h {
        for x in 0..w {
            let i = gi(x, y);
            if c.grid.kind[i] == kind {
                c.pts.put(n, x, y)?;
                c.pts.r[n] = c.grid.r[i];
                n += 1;
            }
        }
    }
    let mut wt = vec![0i64; (w * h) as usize];
    let mut tables: HashMap<i32, Vec<i64>> = HashMap::new();
    for p in 0..n {
        let (px, py) = c.pts.at(p);
        let r = c.pts.r[p] as i32;
        let table = tables.entry(r).or_insert_with(|| (0..(2 * r).max(0)).map(|d| weight(d, r)).collect());
        let (y0, y1) = ((py - 2 * r).max(0), (py + 2 * r).min(h - 1));
        let (x0, x1) = ((px - 2 * r).max(0), (px + 2 * r).min(w - 1));
        for y in y0..=y1 {
            let dy = (y - py).abs();
            let row = &mut wt[(y * w) as usize..((y + 1) * w) as usize];
            for x in x0..=x1 {
                let dx = (x - px).abs();
                let d = dx.max(dy) + dx.min(dy) / 2;
                if let Some(&v) = table.get(d as usize) {
                    if v > row[x as usize] {
                        row[x as usize] = v;
                    }
                }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let v = wt[(y * w + x) as usize];
            if v <= 0 {
                continue;
            }
            let i = gi(x, y);
            let hh = c.grid.h[i];
            if kind == SEA {
                c.grid.h[i] = round_div((10_000 - v) * hh as i64, 10_000) as i32;
            } else if hh > t {
                c.grid.h[i] = t + round_div((10_000 - v) * (hh - t) as i64, 10_000) as i32;
            }
        }
    }
    Ok(())
}

// --- §6 rivers -------------------------------------------------------------------------------

/// One river step from `(x, y)` on heading `a` by `len`: the segment to the end clamped to
/// the map is stamped; returns the clamped end and whether the end was inside.
fn step(c: &mut Ctx, (x, y): (i32, i32), a: i32, len: i32, r: i32) -> ((i32, i32), bool) {
    let (co, si) = ext::cos_sin(a);
    let l = Ext::int(len as i64);
    let nx = x + co.mul(l).round_int() as i32;
    let ny = y + si.mul(l).round_int() as i32;
    let inside = nx >= 0 && nx < c.w && ny >= 0 && ny < c.h;
    let end = (nx.clamp(0, c.w - 1), ny.clamp(0, c.h - 1));
    line(c, (x, y), end, RIVER, r);
    (end, inside)
}

/// The turn a valley river's draw `v` asks for: pending turns and step length.
pub fn valley_turn(v: i32) -> Option<(i32, i32)> {
    let (p, l) = match v.abs() {
        0 => (0, 20),
        1 => (1, 10),
        2 => (2, 5),
        3 | 4 => (4, 4),
        5 | 6 => (6, 4),
        7 => (15, 3),
        8..=36 => (10, 3),
        _ => return None,
    };
    Some((p * v.signum(), l))
}

/// §6.1: three rivers from the centre.
fn valley(c: &mut Ctx) {
    let mut a = c.rng.random(360);
    for _ in 0..3 {
        let mut at = (c.w >> 1, c.h >> 1);
        let base = c.rng.random(80) + a + 60;
        a = base;
        let (mut l, mut pending) = (10, 0);
        loop {
            let drift = (a - base) / 10;
            if pending == 0 {
                let v = c.rng.random(17) - 8 - drift;
                if let Some((p, nl)) = valley_turn(v) {
                    pending = p;
                    l = nl;
                }
            }
            if pending > 0 {
                a += 10;
                pending -= 1;
            } else if pending < 0 {
                a -= 10;
                pending += 1;
            }
            let k = c.rng.random(3) - 1;
            let len = k * (c.rng.random(l) / 3) + l;
            let (end, inside) = step(c, at, a, len, 10);
            at = end;
            if !inside {
                break;
            }
        }
    }
}

/// §6.2: a stem and six rivers from it.
fn estuary(c: &mut Ctx) {
    let (w, h) = (c.w, c.h);
    let (cx, cy) = (w >> 1, h >> 1);
    let o = c.o.orient;
    let stem = match o {
        0 => (cx, cy + h / 7),
        1 => (cx, cy - h / 7),
        2 => (cx + w / 7, cy),
        _ => (cx - w / 7, cy),
    };
    line(c, (cx, cy), stem, RIVER, 20);
    const HEADINGS: [[i32; 6]; 4] = [[90, 225, 240, 270, 300, 315], [-90, 45, 60, 90, 120, 135], [0, 135, 150, 180, 210, 225], [180, 315, 330, 360, 390, 405]];
    for i in 0..6usize {
        let far = i < 2 || i == 5;
        // The axis along the river: y for north and south, x for west and east.
        let (side, full) = if o < 2 { (h, cy) } else { (w, cx) };
        let toward = if o.is_multiple_of(2) { 1 } else { -1 };
        let mut along = if far { full + toward * (side / 7) } else { full };
        if i == 1 || i == 5 {
            along -= toward * c.rng.random(side / 7);
        }
        if i == 2 || i == 4 {
            along += toward * c.rng.random(side / 14);
        }
        let mut at = if o < 2 { (cx, along) } else { (along, cy) };
        let mut a = HEADINGS[o as usize][i];
        let r = if i == 0 || i == 3 { 20 } else { 10 };
        loop {
            a += if i == 0 { 5 * c.rng.random(13) - 30 } else { 5 * c.rng.random(7) - 15 };
            let (end, inside) = step(c, at, a, 20, r);
            at = end;
            if !inside {
                break;
            }
        }
    }
}

/// §6.3: short segments that are never carved (their draws come before the reseed).
fn skerries(c: &mut Ctx) {
    let (w, h) = (c.w, c.h);
    for _ in 0..w / 3 {
        let (x, y) = match c.o.orient {
            0 => {
                let x = c.rng.random(w);
                (x, c.rng.random(h >> 2))
            }
            1 => {
                let x = c.rng.random(w);
                (x, h - 1 - c.rng.random(h >> 2))
            }
            2 => {
                let y = c.rng.random(h);
                (c.rng.random(w >> 2), y)
            }
            _ => {
                let y = c.rng.random(h);
                (w - 1 - c.rng.random(w >> 2), y)
            }
        };
        let (sx, sy) = if c.rng.random(2) == 0 { (x - 5, y) } else { (x, y - 5) };
        let clamp = |(x, y): (i32, i32)| (x.clamp(0, w - 1), y.clamp(0, h - 1));
        line(c, clamp((sx, sy)), clamp((x, y)), RIVER, 20);
    }
}
