//! The land (newmap.md §7–§9): terrain classes, the coast band split, mountains, hills and
//! the forest, on the map cells.

use super::ext::round_div;
use super::relief::{self, gi, Bounds};
use super::{Ctx, Stage, Step, Stop};

/// The 12 neighbours, in the original's order (data 0x5bb1fc, 0x5bb208): the four
/// orthogonal steps, then eight knight moves.
pub const DX: [i32; 12] = [1, 0, -1, 0, 1, 1, 2, -2, -1, -1, 2, -2];
pub const DY: [i32; 12] = [0, 1, 0, -1, 2, -2, 1, 1, 2, -2, -1, -1];

/// Markers the generator writes into the mark byte.
pub const HILL: i8 = -1;
pub const MOUNTAIN: i8 = -5;
pub const TREES: i8 = -9;
pub const THICKET: i8 = -11;

/// The draws `Random(n)` can give: at most 32768 values (the generator has 15 bits), and
/// `Random(0)` gives 0, so list entry 0 is read even from an empty list.
fn reach(n: usize) -> usize {
    n.clamp(1, 32768)
}

/// Which listing pass clears flags.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Clear {
    /// Every cell of the map, as the pass goes.
    All,
    /// The listed cells.
    Listed,
    None,
}

/// Lists the cells for which `pick` holds into the point list, row by row (from the bottom
/// row up with `bottom_first`); returns how many.
fn list(c: &mut Ctx, clear: Clear, bottom_first: bool, pick: impl Fn(&Ctx, usize) -> bool) -> Step<usize> {
    let mut n = 0;
    for yy in 0..c.h {
        let y = if bottom_first { c.h - 1 - yy } else { yy };
        for x in 0..c.w {
            let i = c.cells.idx(x, y);
            if clear == Clear::All {
                c.cells.flag[i] = 0;
            }
            if pick(c, i) {
                if clear == Clear::Listed {
                    c.cells.flag[i] = 0;
                }
                c.pts.put(n, x, y)?;
                n += 1;
            }
        }
    }
    Ok(n)
}

/// The listed cell `j`.
fn at(c: &Ctx, j: usize) -> usize {
    let (x, y) = c.pts.at(j);
    c.cells.idx(x, y)
}

/// One sweep over list entries `0..n` (§8): a cell whose flag is clear tries its strictly
/// inside neighbours in order until `hit(ctx, cell, neighbour)` flags it. Returns the hits.
fn sweep(c: &mut Ctx, n: usize, mut hit: impl FnMut(&mut Ctx, usize, usize) -> bool) -> usize {
    let mut hits = 0;
    for j in 0..n {
        let (px, py) = c.pts.at(j);
        let p = c.cells.idx(px, py);
        for k in 0..12 {
            if c.cells.flag[p] != 0 {
                break;
            }
            let (nx, ny) = (px + DX[k], py + DY[k]);
            if !(0 < ny && ny < c.h - 1 && 0 < nx && nx < c.w - 1) {
                continue;
            }
            let nb = c.cells.idx(nx, ny);
            if hit(c, p, nb) {
                c.cells.flag[p] = 1;
                hits += 1;
            }
        }
    }
    hits
}

/// Every flagged cell of the map takes mark `code`.
fn flagged_to(c: &mut Ctx, code: i8) {
    for (m, f) in c.cells.mark.iter_mut().zip(&c.cells.flag) {
        if *f != 0 {
            *m = code;
        }
    }
}

/// The terrain from the final cut (§7, §8).
pub fn terrain(c: &mut Ctx, b: &Bounds) -> Step<()> {
    classes(c, b);
    band(c)?;
    Ok(())
}

/// §7: every cell from its height; a later class overwrites what it writes, the plain
/// writes nothing. The tree slot is not cleared.
pub fn classes(c: &mut Ctx, b: &Bounds) {
    for y in 0..c.h {
        for x in 0..c.w {
            let i = c.cells.idx(x, y);
            let h = c.grid.h[gi(x, y)];
            let cl = &mut c.cells;
            cl.flag[i] = 0;
            cl.ground[i] = 6;
            cl.mark[i] = 6;
            cl.obj[i] = 0;
            for k in 1..=8 {
                if !b.holds(k, h) {
                    continue;
                }
                let (mark, ground) = match k {
                    1 => (2, Some(2)),
                    2 => (1, Some(1)),
                    3 => (0, Some(0)),
                    4 => (8, Some(8)),
                    5 => continue,
                    6 => (HILL, None),
                    7 => (MOUNTAIN, Some(12)),
                    _ => (12, Some(12)),
                };
                cl.mark[i] = mark;
                if let Some(g) = ground {
                    cl.ground[i] = g;
                }
            }
        }
    }
}

/// Draws list entries until one passes `ok`; `None` when no entry a draw can reach does
/// (the original loops for ever there).
fn draw_until(c: &mut Ctx, n: usize, ok: impl Fn(&Ctx, usize) -> bool) -> Option<usize> {
    if !(0..reach(n)).any(|j| ok(c, at(c, j))) {
        return None;
    }
    loop {
        let j = c.rng.random(n as i32) as usize;
        if ok(c, at(c, j)) {
            return Some(j);
        }
    }
}

/// §8: the coast band into sand, lowland, marsh and swamp.
pub fn band(c: &mut Ctx) -> Step<()> {
    let s = c.o.shares;
    let n = list(c, Clear::None, false, |c, i| c.cells.mark[i] == 8)? as i64;
    let sand_target = round_div(s.sl1 as i64 * n, 100);
    let low_target = round_div(s.sl2() as i64 * n, 100);
    let mut m = n - low_target - sand_target;

    // 1. Lowland from the water, growing within the sweep.
    let mut grown = 0;
    loop {
        let hits = sweep(c, n as usize, |c, p, nb| {
            let v = c.cells.mark[nb];
            if v >= 0 && (v < 3 || v == 5) {
                c.cells.mark[p] = 5;
                true
            } else {
                false
            }
        });
        grown += hits as i64;
        if hits == 0 {
            break;
        }
    }
    m = m - n + grown;

    // 2. More lowland until the marsh share is 0.
    if m < 0 {
        let n2 = list(c, Clear::Listed, false, |c, i| c.cells.mark[i] == 8)?;
        while m != 0 {
            let before = m;
            sweep(c, n2, |c, _, nb| {
                let v = c.cells.mark[nb];
                if v >= 0 && (v == 5 || v == 6) && m < 0 {
                    m += 1;
                    true
                } else {
                    false
                }
            });
            flagged_to(c, 5);
            c.check()?;
            if m == before {
                return Err(Stop::Hang(Stage::MoreLowland));
            }
        }
    }

    // 3. Sand from the shallows, two sweeps.
    let n3 = list(c, Clear::All, false, |c, i| c.cells.mark[i] == 5)?;
    let mut sand = 0i64;
    for _ in 0..2 {
        sand += sweep(c, n3, |c, _, nb| matches!(c.cells.mark[nb], 0 | 10)) as i64;
        flagged_to(c, 10);
    }

    // 4. Erosion, three sweeps, always.
    let n4 = list(c, Clear::All, false, |c, i| c.cells.mark[i] == 10)?;
    for _ in 0..3 {
        sand -= sweep(c, n4, |c, _, nb| c.cells.mark[nb] == 5) as i64;
        flagged_to(c, 5);
    }

    // 5. Sand to its target.
    if sand < sand_target {
        let n5 = list(c, Clear::All, true, |c, i| c.cells.mark[i] == 5)?;
        loop {
            let hits = sweep(c, n5, |c, _, nb| {
                if c.cells.mark[nb] == 10 && sand < sand_target {
                    sand += 1;
                    true
                } else {
                    false
                }
            });
            flagged_to(c, 10);
            c.check()?;
            if sand == sand_target || hits == 0 {
                break;
            }
        }
    } else {
        let n5 = list(c, Clear::All, false, |c, i| c.cells.mark[i] == 10)?;
        loop {
            let mut grew = sweep(c, n5, |c, _, nb| {
                if c.cells.mark[nb] == 5 && sand_target < sand {
                    sand -= 1;
                    true
                } else {
                    false
                }
            }) > 0;
            flagged_to(c, 5);
            c.check()?;
            if !grew && sand_target < sand {
                let j = draw_until(c, n5, |c, i| c.cells.mark[i] != 5).ok_or(Stop::Hang(Stage::Sand))?;
                let i = at(c, j);
                c.cells.mark[i] = 5;
                c.cells.flag[i] = 1;
                sand -= 1;
                grew = true;
            }
            if sand == sand_target || !grew {
                break;
            }
        }
    }

    // 6. Marsh from lowland seeds.
    if m > 0 {
        let n6 = list(c, Clear::All, false, |c, i| c.cells.mark[i] == 5)?;
        while m != 0 {
            let j = draw_until(c, n6, |c, i| c.cells.mark[i] != 8).ok_or(Stop::Hang(Stage::Marsh))?;
            let i = at(c, j);
            c.cells.mark[i] = 8;
            c.cells.flag[i] = 1;
            m -= 1;
            c.check()?;
            loop {
                let hits = sweep(c, n6, |c, _, nb| {
                    if c.cells.mark[nb] == 8 && m > 0 {
                        m -= 1;
                        true
                    } else {
                        false
                    }
                });
                flagged_to(c, 8);
                if hits == 0 {
                    break;
                }
            }
        }
    }

    // 7. Impassable swamp.
    let last = swamp(c, m)?;

    // 8. Ground codes; the flags stay (a mistaken index clears only the last drawn cell's).
    for i in 0..c.cells.mark.len() {
        let v = c.cells.mark[i];
        if v == 5 || v == 9 || v == 10 {
            c.cells.ground[i] = v as u8;
        }
    }
    let i = at(c, last);
    c.cells.flag[i] = 0;
    Ok(())
}

/// §8 step 7: impassable swamp by the water first, or only by the water. `m` is what is
/// left of the marsh share (0 after step 6). Returns the list entry the last draw took.
pub fn swamp(c: &mut Ctx, m: i64) -> Step<usize> {
    let n7 = list(c, Clear::All, false, |c, i| c.cells.mark[i] == 8)?;
    let mut q = round_div(c.o.shares.sw2() as i64 * (m + n7 as i64), 100);
    let mut by_water = 0i64;
    for _ in 0..2 {
        by_water += sweep(c, n7, |c, _, nb| (0..3).contains(&c.cells.mark[nb])) as i64;
    }
    if by_water < q {
        for j in 0..n7 {
            let i = at(c, j);
            if c.cells.flag[i] != 0 {
                c.cells.mark[i] = 9;
            }
        }
        q -= by_water;
    } else {
        for j in 0..n7 {
            let i = at(c, j);
            c.cells.flag[i] ^= 1;
        }
    }
    // At least one draw, even for nothing left to place: a clear cell it hits becomes swamp.
    let mut last;
    loop {
        last = c.rng.random(n7 as i32) as usize;
        let i = at(c, last);
        if c.cells.flag[i] == 0 {
            c.cells.flag[i] = 1;
            c.cells.mark[i] = 9;
            q -= 1;
        }
        if q <= 0 {
            break;
        }
        if !(0..reach(n7)).any(|j| c.cells.flag[at(c, j)] == 0) {
            return Err(Stop::Hang(Stage::Swamp));
        }
    }

    Ok(last)
}

/// §9: mountains, hills and the forest.
pub fn objects(c: &mut Ctx) -> Step<()> {
    mountains(c);
    hills(c)?;
    forest(c)
}

/// Whether the `k × k` block with top-left `(x, y)`: (hill markers, clear flags).
fn block(c: &Ctx, x: i32, y: i32, k: i32, marker: i8) -> (i32, i32) {
    let (mut m, mut free) = (0, 0);
    for yy in y..y + k {
        for xx in x..x + k {
            let i = c.cells.idx(xx, yy);
            m += (c.cells.mark[i] == marker) as i32;
            free += (c.cells.flag[i] == 0) as i32;
        }
    }
    (m, free)
}

fn flag_block(c: &mut Ctx, x: i32, y: i32, k: i32) {
    for yy in y..y + k {
        for xx in x..x + k {
            let i = c.cells.idx(xx, yy);
            c.cells.flag[i] = 1;
        }
    }
}

/// §9.2: mountain blocks of 5 down to 1, the object on the bottom-right cell.
pub fn mountains(c: &mut Ctx) {
    for k in (1..=5).rev() {
        for y in 0..=c.h - k {
            for x in 0..=c.w - k {
                if block(c, x, y, k, MOUNTAIN) == (k * k, k * k) {
                    flag_block(c, x, y, k);
                    let v = c.rng.random(c.sprites.mountains(k));
                    let i = c.cells.idx(x + k - 1, y + k - 1);
                    c.cells.obj[i] = (0x500 + 10 * k + v) as u16;
                }
            }
        }
    }
    for i in 0..c.cells.mark.len() {
        if c.cells.mark[i] == MOUNTAIN && c.cells.flag[i] == 0 {
            c.cells.mark[i] = 6;
        }
    }
}

/// §9.3: random hills of 6 and 5, then a scan of 4 down to 1 that takes nearly full blocks.
pub fn hills(c: &mut Ctx) -> Step<()> {
    let n = list(c, Clear::None, false, |c, i| c.cells.mark[i] == HILL)?;
    for k in [6, 5] {
        for _ in 0..c.w / (7 - k) {
            let j = c.rng.random(n as i32) as usize;
            let (px, py) = c.pts.at(j);
            // Blocks touching row or column 0 are never tried.
            if py < k || px < k {
                continue;
            }
            let (x, y) = (px - k + 1, py - k + 1);
            if block(c, x, y, k, HILL) == (k * k, k * k) {
                flag_block(c, x, y, k);
                let v = c.rng.random(c.sprites.hills(k));
                let i = c.cells.idx(px, py);
                c.cells.obj[i] = (0x100 + 10 * k + v) as u16;
            }
        }
    }
    for k in (1..=4).rev() {
        for y in 0..=c.h - k {
            for x in 0..=c.w - k {
                let (m, free) = block(c, x, y, k, HILL);
                let take = if k == 1 {
                    m == 1 && free == 1
                } else {
                    // k ÷ 2 + count ≥ k² in single precision: three of four, 8 of 9, 14 of 16.
                    let half = k as f32 / 2.0;
                    !(half + (free as f32) < (k * k) as f32 || half + (m as f32) < (k * k) as f32)
                };
                if take {
                    flag_block(c, x, y, k);
                    let v = c.rng.random(c.sprites.hills(k));
                    // On the bottom-right cell, hill or not.
                    let i = c.cells.idx(x + k - 1, y + k - 1);
                    c.cells.obj[i] = (0x100 + 10 * k + v) as u16;
                }
            }
        }
    }
    let cl = &mut c.cells;
    for i in 0..cl.mark.len() {
        if cl.mark[i] == HILL && cl.flag[i] == 0 {
            cl.mark[i] = 6;
        }
    }
    for i in 0..cl.mark.len() {
        if cl.mark[i] == 6 && cl.flag[i] != 0 {
            cl.mark[i] = HILL;
        }
    }
    Ok(())
}

/// The tree a flagged pool cell asks for (§9.4 step 4): the class and the family, from the
/// cluster's family `f`, the cell's forest class and its ground, with the draws the ground
/// takes.
fn tree_kind(c: &mut Ctx, f: u8, forest: i32, ground: u8) -> (u8, u8) {
    let mut cls = if forest == 3 {
        11
    } else if f < 8 {
        9
    } else {
        10
    };
    let mut v = f;
    match ground {
        8 => {
            v = match c.rng.random(6) {
                0..=2 => 9,
                3 | 4 => 5,
                _ => 4,
            }
        }
        5 => v = if c.rng.random(3) < 2 { 4 } else { 5 },
        10 => v = if c.rng.random(3) < 2 { 5 } else { 4 },
        _ => {
            if f == 4 || f == 5 {
                v = 0;
            }
        }
    }
    if v == 4 {
        cls = 9;
    }
    (cls, v)
}

/// Whether a pool cell of forest class `forest` on `ground` can ever get a loaded sprite.
fn placeable(c: &Ctx, families: &[u8], forest: i32, ground: u8) -> bool {
    let fams: Vec<u8> = if families.is_empty() { vec![0] } else { families.iter().map(|&f| if f == 6 || f == 7 { 0 } else { f }).collect() };
    let vs: &[u8] = match ground {
        8 => &[9, 5, 4],
        5 | 10 => &[4, 5],
        _ => &[],
    };
    fams.iter().any(|&f| {
        let base = if forest == 3 {
            11
        } else if f < 8 {
            9
        } else {
            10
        };
        let choices: Vec<u8> = if vs.is_empty() { vec![if f == 4 || f == 5 { 0 } else { f }] } else { vs.to_vec() };
        choices.into_iter().any(|v| {
            let cls = if v == 4 { 9 } else { base };
            let n = c.sprites.plants(cls, v).max(1);
            (0..n).any(|r| c.sprites.loaded(((cls as u16) << 8).wrapping_add(12 * v as u16 + r as u16)))
        })
    })
}

/// A cluster run that placed nothing this many times in a row is taken for the original's
/// endless loop (cells whose draws all miss, out of reach of a 15-bit draw).
const FOREST_STALL: u32 = 1000;

/// §9.4: the forest field, then clusters of trees until the pool is empty.
pub fn forest(c: &mut Ctx) -> Step<()> {
    forest_field(c)?;
    clusters(c)
}

/// §9.4 steps 1–2: the forest field over the relief, and the forest class of each cell
/// that can hold trees (in the working values).
pub fn forest_field(c: &mut Ctx) -> Step<()> {
    for y in 0..c.h {
        for x in 0..c.w {
            c.grid.v[gi(x, y)] = 0;
        }
    }
    relief::midpoint(c, 0, 0, c.w - 1, c.h - 1, 2, 1)?;
    // The relief is lost: the forest field takes its place in the grid (§10.1).
    relief::copy_values(c);
    let b = relief::levels(c, 2)?;
    for y in 0..c.h {
        for x in 0..c.w {
            let g = c.cells.ground[c.cells.idx(x, y)];
            if (5..=8).contains(&g) || g == 10 {
                let i = gi(x, y);
                for k in 1..=3 {
                    if b.holds(k, c.grid.h[i]) {
                        c.grid.v[i] = k as i32;
                    }
                }
            }
        }
    }
    Ok(())
}

/// §9.4 steps 3–6: clusters of trees until the pool is empty.
pub fn clusters(c: &mut Ctx) -> Step<()> {
    let families = c.sprites.families();
    // Which (forest class, ground) cells can ever get a loaded sprite.
    let mut can = [[false; 16]; 4];
    for (fc, row) in can.iter_mut().enumerate().skip(2) {
        for (g, v) in row.iter_mut().enumerate() {
            *v = placeable(c, &families, fc as i32, g as u8);
        }
    }
    // The pool: forest classes 2 and 3, row by row. The original lists it again after every
    // cluster; only placed cells ever leave it, so Razdor keeps it as a counted set in that
    // order (the draws are the same, without a scan of the map per cluster).
    let n0 = list(c, Clear::Listed, false, |c, i| c.grid.v[gi(i as i32 % c.w, i as i32 / c.w)] > 1)?;
    let cells_of: Vec<usize> = (0..n0).map(|j| at(c, j)).collect();
    let mut pos_of = vec![u32::MAX; c.cells.mark.len()];
    for (j, &i) in cells_of.iter().enumerate() {
        pos_of[i] = j as u32;
    }
    let mut pool = Counted::full(n0);
    let fc_of = |c: &Ctx, i: usize| c.grid.v[gi(i as i32 % c.w, i as i32 / c.w)];
    let mut open = cells_of.iter().filter(|&&i| can[fc_of(c, i).clamp(0, 3) as usize][(c.cells.ground[i] & 15) as usize]).count();
    let mut n = n0;
    // A divisor of 0 (the reuse path's stale one) fails here, before the first cluster.
    if *c.divisor == 0 {
        return Err(Stop::SmallForestPool);
    }
    let q = n as i32 / *c.divisor;
    let mut stalled = 0;
    while n > 0 {
        let j = c.rng.random(n as i32) as usize;
        let seed = cells_of[pool.nth(j)];
        c.cells.flag[seed] = 1;
        let mut budget = c.w;
        // One sweep in pool order (it would repeat only after growing with the budget spent,
        // and then grows nothing): a flagged cell flags its neighbours, and those later in
        // the pool spread in the same sweep.
        let mut flagged = vec![seed];
        let mut next = std::collections::BinaryHeap::from([std::cmp::Reverse(seed)]);
        while let Some(std::cmp::Reverse(p)) = next.pop() {
            let (px, py) = (p as i32 % c.w, p as i32 / c.w);
            for k in 0..12 {
                let (nx, ny) = (px + DX[k], py + DY[k]);
                if !(0 < ny && ny < c.h - 1 && 0 < nx && nx < c.w - 1) {
                    continue;
                }
                let nb = c.cells.idx(nx, ny);
                if c.cells.flag[nb] == 0 && c.grid.v[gi(nx, ny)] > 1 && budget > 0 {
                    c.cells.flag[nb] = 1;
                    budget -= 1;
                    flagged.push(nb);
                    if nb > p {
                        next.push(std::cmp::Reverse(nb));
                    }
                }
            }
        }
        // An empty family list reads a stale byte; Razdor takes family 0.
        let mut f = if families.is_empty() {
            c.rng.random(0);
            0
        } else {
            families[c.rng.random(families.len() as i32) as usize]
        };
        if f == 6 || f == 7 {
            f = 0;
        }
        flagged.sort_unstable();
        let mut placed = 0;
        for &i in &flagged {
            let (px, py) = (i as i32 % c.w, i as i32 / c.w);
            let fc = c.grid.v[gi(px, py)];
            let (cls, v) = tree_kind(c, f, fc, c.cells.ground[i]);
            let r = c.rng.random(c.sprites.plants(cls, v));
            let id = ((cls as u16) << 8).wrapping_add(12 * v as u16).wrapping_add(r as u16);
            if !c.sprites.loaded(id) {
                // Nothing drawn: the cell stays in the pool.
                c.cells.flag[i] = 0;
                continue;
            }
            c.cells.tree[i] = id;
            if c.rng.random(5) == 0 && c.sprites.loaded(id.wrapping_add(120)) {
                c.cells.tree[i] = id.wrapping_add(120);
            }
            c.cells.mark[i] = if fc == 2 { TREES } else { THICKET };
            c.grid.v[gi(px, py)] = 0;
            // A placed cell keeps its flag (the pool listing clears only pool cells).
            pool.remove(pos_of[i] as usize);
            placed += 1;
        }
        // The progress step: a division by 0 after the first cluster stops the run.
        if q == 0 {
            return Err(Stop::SmallForestPool);
        }
        if (n as i32 + 1) / q < *c.divisor {
            *c.divisor -= 1;
        }
        n -= placed;
        open -= placed;
        c.check()?;
        stalled = if placed > 0 { 0 } else { stalled + 1 };
        if n > 0 && (open == 0 || stalled >= FOREST_STALL) {
            return Err(Stop::Hang(Stage::Forest));
        }
    }
    Ok(())
}

/// A set of positions `0..n` with the `j`-th present one found by halving (a Fenwick tree).
struct Counted {
    tree: Vec<i32>,
}

impl Counted {
    fn full(n: usize) -> Counted {
        let mut tree = vec![0; n + 1];
        for i in 1..=n {
            tree[i] += 1;
            let up = i + (i & i.wrapping_neg());
            if up <= n {
                tree[up] += tree[i];
            }
        }
        Counted { tree }
    }

    fn remove(&mut self, pos: usize) {
        let mut i = pos + 1;
        while i < self.tree.len() {
            self.tree[i] -= 1;
            i += i & i.wrapping_neg();
        }
    }

    /// The position of the `j`-th (from 0) present entry.
    fn nth(&self, j: usize) -> usize {
        let n = self.tree.len() - 1;
        let (mut pos, mut rest) = (0usize, j as i32 + 1);
        let mut step = n.next_power_of_two();
        while step > 0 {
            if pos + step <= n && self.tree[pos + step] < rest {
                pos += step;
                rest -= self.tree[pos];
            }
            step >>= 1;
        }
        pos
    }
}
