//! The generator against the spec's numbers, small hand-checked cases of each stage, its
//! quirks, and pinned maps (the same seed and options always give the same map).

use std::sync::atomic::AtomicBool;

use super::land::{self, HILL, MOUNTAIN};
use super::relief::{self, gi, Grid};
use super::*;
use crate::editor::palette::{ObjectKey, Palette};

/// A palette shaped as the install's (dtm-format.md §5): hills, mountains, the tree
/// families 0–5 with some "+120" pictures, dead trees and thickets.
fn palette() -> Palette {
    let mut objects = Vec::new();
    let mut add = |class: u8, ids: &mut dyn Iterator<Item = u8>| objects.extend(ids.map(|sprite| ObjectKey { class, sprite }));
    add(1, &mut (10..=19).chain(20..=27).chain(30..=33).chain(40..=43).chain(50..=53).chain(60..=60));
    add(5, &mut (10..=13).chain(20..=23).chain(30..=33).chain(40..=43).chain(50..=53));
    add(9, &mut (0..=8).chain(12..=20).chain(24..=29).chain(36..=41).chain(48..=50).chain(60..=65).chain(120..=128));
    add(10, &mut (108..=116).chain(228..=236));
    add(11, &mut (0..=8).chain(12..=20).chain(24..=29).chain(36..=41).chain(108..=116));
    objects.sort();
    Palette { objects, buildings: Vec::new(), from_install: true }
}

fn sprites() -> Sprites {
    Sprites::from_palette(&palette())
}

fn options(size: u32, kind: u8) -> Options {
    Options { size, kind, keep: true, rebuild: true, ..Options::default() }
}

fn run_with(g: &mut Generator, o: Options, seed: i32, start: Cells) -> Outcome {
    g.run(Job { options: o, seed, sprites: sprites(), start, clock: 4242 }, &AtomicBool::new(false))
}

fn run(o: Options, seed: i32) -> Outcome {
    run_with(&mut Generator::new(), o, seed, Cells::zero(o.size))
}

/// FNV-1a over everything a map keeps.
fn hash(c: &Cells) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |b: u8| h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    for i in 0..c.mark.len() {
        eat(c.mark[i] as u8);
        eat(c.ground[i]);
        c.obj[i].to_le_bytes().into_iter().chain(c.tree[i].to_le_bytes()).for_each(&mut eat);
    }
    h
}

/// A context on hand-made cells.
struct Bench {
    grid: Grid,
    sprites: Sprites,
    brk: AtomicBool,
    divisor: i32,
}

impl Bench {
    fn new() -> Bench {
        Bench { grid: Grid::new(), sprites: sprites(), brk: AtomicBool::new(false), divisor: 30 }
    }

    fn ctx(&mut self, size: i32, o: Options, seed: u32) -> Ctx<'_> {
        Ctx {
            w: size,
            h: size,
            o,
            rng: Rng::new(seed),
            grid: &mut self.grid,
            cells: Cells::zero(size as u32),
            pts: Points::new(),
            sprites: &self.sprites,
            brk: &self.brk,
            divisor: &mut self.divisor,
        }
    }
}

/// Cells from rows of mark characters: `.` plain, `~` shallows, `=` deep sea, `b` band
/// (marsh), `^` stony soil, `h` hill marker, `m` mountain marker.
fn paint(c: &mut Ctx, rows: &[&str]) {
    for (y, row) in rows.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            let (mark, ground) = match ch {
                '~' => (0, 0),
                '=' => (2, 2),
                'b' => (8, 8),
                '^' => (12, 12),
                'h' => (HILL, 6),
                'm' => (MOUNTAIN, 12),
                _ => (6, 6),
            };
            let i = c.cells.idx(x as i32, y as i32);
            c.cells.mark[i] = mark;
            c.cells.ground[i] = ground;
        }
    }
}

fn count(c: &Cells, mark: i8) -> usize {
    c.mark.iter().filter(|&&m| m == mark).count()
}

// --- the share bars --------------------------------------------------------------------------

#[test]
fn the_default_panels_give_the_spec_weights() {
    let s = Shares::default();
    assert_eq!((s.l0(), s.w2(), s.l1(), s.lm(), s.sl2(), s.sw2(), s.f1()), (76, 40, 83, 11, 21, 59, 39));
    let w = relief::weights(&s, 1);
    assert_eq!(w, vec![675, 1080, 945, 1520, 3446, 2080, 715, 65]);
    assert_eq!(w.iter().sum::<i32>(), 10526);
    assert_eq!(relief::weights(&s, 2), vec![3900, 3900, 2200]);
    assert_eq!(s.panels(3), vec![53, 32, 11, 1]);
    assert_eq!(s.panels(1), vec![35, 40, 25]);
}

#[test]
fn splitters_snap_and_stop_as_the_toolkits() {
    let mut s = Shares::default();
    // At or below the minimum the panel snaps to 0; the stretched panel keeps the minimum.
    s.drag(0, 0, 3);
    assert_eq!((s.w0, s.l0()), (0, 103));
    s.drag(0, 0, 5);
    assert_eq!(s.w0, 0);
    s.drag(0, 0, 6);
    assert_eq!((s.w0, s.l0()), (6, 97));
    s.drag(0, 0, 200);
    assert_eq!((s.w0, s.l0()), (98, 5));
    // The right-hand panels: W3 is sized too (and not saved).
    s.drag(1, 1, 40);
    assert_eq!((s.w3, s.w2()), (40, 25));
    // Splitters of minimum 1: stony soil of 1 snaps to 0.
    s.drag(3, 2, 1);
    assert_eq!(s.lr, 0);
    s.drag(3, 2, 2);
    assert_eq!(s.lr, 2);
    s.drag(3, 1, 99);
    assert_eq!((s.lh, s.lm()), (32 + 9, 1));
}

#[test]
fn the_make_map_section_reads_and_writes_as_the_original() {
    let ini = Ini::parse("[MakeMap]\nW0=27\nW1=35\nW2=99\nSL=20\nLL=53\nLH=32\nLR=1\nSL1=54\nSw1=44\nF0=39\nF2=22\n");
    let s = Shares::from_section(ini.section(SECTION).unwrap());
    // W2 is stretched (the stored 99 is not used), SL2 missing changes nothing, the fixed 25s.
    assert_eq!(s, Shares::default());
    let blank = Shares::from_section(Ini::parse("[MakeMap]\nW0=x\n").section(SECTION).unwrap());
    assert_eq!((blank.w0, blank.w1, blank.f2, blank.w3, blank.sl3), (0, 0, 0, 25, 25));
    let keys: Vec<String> = s.keys().iter().map(|(k, v)| format!("{k}={v}")).collect();
    assert_eq!(keys.join(" "), "W0=27 W1=35 W2=40 SL=20 LL=53 LH=32 LR=1 SL1=54 SL2=21 Sw1=44 F0=39 F2=22");
    let own = crate::editor::files::tests::temp_dir("makemap-own");
    let install = crate::editor::files::tests::temp_dir("makemap-install");
    std::fs::write(install.join(super::super::options::FILE), b"[MakeMap]\r\nW0=50\r\n").unwrap();
    assert_eq!(Shares::load(Some(&own), Some(&install)).w0, 50);
    assert_eq!(Shares::load(Some(&own), None), Shares::default());
    crate::editor::options::Options::default().save(&own).unwrap();
    let mine = Shares { w0: 33, f2: 10, ..Shares::default() };
    mine.save(&own).unwrap();
    assert_eq!(Shares::load(Some(&own), Some(&install)), mine);
    assert_eq!(crate::editor::options::Options::load(Some(&own), None), crate::editor::options::Options::default());
}

// --- the cut -------------------------------------------------------------------------------

#[test]
fn the_targets_add_rounded_shares_and_cap_the_last() {
    let w = relief::weights(&Shares::default(), 1);
    assert_eq!(relief::targets(10_000, &w).unwrap(), vec![0, 641, 1667, 2565, 4009, 7283, 9259, 9938, 10000]);
    // 5/3 × 1 rounds to 2 three times: only the last is capped.
    assert_eq!(relief::targets(5, &[1, 1, 1]).unwrap(), vec![0, 2, 4, 5]);
    assert_eq!(relief::targets(5, &[0, 0]), Err(Stop::NoShares));
}

#[test]
fn the_cut_matches_twice_and_fixes_empty_classes() {
    let mut hist = vec![0i64; 1001];
    (hist[0], hist[2], hist[3]) = (3, 2, 5);
    let b = relief::cut(&hist, &[0, 3, 3, 10]);
    // Class 1 matched at bins 0 and 2 and keeps the later bounds; class 2 got high < low.
    assert_eq!((b.low[1], b.high[1]), (0, 2));
    assert_eq!((b.low[2], b.high[2]), (3, 3));
    assert_eq!((b.low[3], b.high[3]), (4, 1000));
    // A class that never matches keeps 0–0.
    let b = relief::cut(&hist, &[0, 3, 50, 10]);
    assert_eq!((b.low[2], b.high[2]), (0, 0));
}

// --- the relief ----------------------------------------------------------------------------

#[test]
fn the_fractal_draws_four_jitters_and_writes_only_zero_cells() {
    let mut bench = Bench::new();
    let mut c = bench.ctx(3, Options::default(), 7);
    let mut r = Rng::new(7);
    let j: Vec<i32> = (0..4).map(|_| r.random(100) - 50).collect();
    relief::midpoint(&mut c, 0, 0, 2, 2, 2, 1).unwrap();
    let v = |x, y| c.grid.v[gi(x, y)];
    let centre = (j[0] + j[1] + j[2] + j[3]) / 4;
    // Each written while it held 0, and kept by the quarters afterwards.
    assert_eq!((v(1, 0), v(1, 2), v(0, 1), v(2, 1), v(1, 1)), (j[0], j[1], j[2], j[3], centre));
    // The four 1 × 1 quarters draw four jitters each and recurse no further; the first one's
    // top value lands on its top-left corner, the corner of the map.
    let q: Vec<i32> = (0..16).map(|_| r.random(100) - 50).collect();
    assert_eq!(v(0, 0), q[0] + j[0] / 2);
    assert_eq!(v(2, 2), 0, "the bottom-right corner is never a midpoint");
    assert_eq!(c.rng.state(), r.state());
}

#[test]
fn the_blur_runs_in_place_and_counts_the_cell_twice() {
    let mut bench = Bench::new();
    let mut c = bench.ctx(3, Options::default(), 1);
    c.grid.v[gi(0, 0)] = 70;
    relief::blur(&mut c, 1);
    let v = |x, y| c.grid.v[gi(x, y)];
    // (70 + 70) / 5; then (0 + 28) / 7 with the new value; (4) / 5; (28 + 4) / 7.
    assert_eq!((v(0, 0), v(1, 0), v(2, 0), v(0, 1)), (28, 4, 0, 4));
}

#[test]
fn lines_mark_both_ends_and_decide_before_moving() {
    let cells = |a, b| {
        let mut bench = Bench::new();
        let mut c = bench.ctx(10, Options::default(), 1);
        relief::line(&mut c, a, b, relief::SEA, 3);
        let mut v = Vec::new();
        for y in 0..10 {
            for x in 0..10 {
                if c.grid.kind[gi(x, y)] == relief::SEA {
                    v.push((x, y));
                }
            }
        }
        v
    };
    assert_eq!(cells((0, 0), (5, 2)), vec![(0, 0), (1, 0), (2, 1), (3, 1), (4, 2), (5, 2)]);
    assert_eq!(cells((0, 0), (2, 5)), vec![(0, 0), (0, 1), (1, 2), (1, 3), (2, 4), (2, 5)]);
    assert_eq!(cells((3, 3), (0, 0)), vec![(0, 0), (1, 1), (2, 2), (3, 3)]);
    assert_eq!(cells((2, 2), (2, 2)), vec![(2, 2)]);
}

#[test]
fn the_feature_weight_falls_from_10000_to_0() {
    assert_eq!(relief::weight(0, 10), 10_000);
    assert_eq!(relief::weight(1, 10), 9406);
    // t = ½: 10000 × ½ ÷ 1.1.
    assert_eq!(relief::weight(10, 10), 4545);
    assert_eq!(relief::weight(19, 10), 420);
    assert_eq!(relief::weight(20, 10), 0);
}

#[test]
fn the_island_corner_keeps_the_edge_radius() {
    let mut bench = Bench::new();
    let mut c = bench.ctx(50, options(50, ISLAND), 1);
    relief::sea_stamps(&mut c);
    let r = |x, y| c.grid.r[gi(x, y)];
    assert_eq!((r(0, 0), r(49, 0), r(49, 49), r(0, 49), r(25, 0)), (10, 16, 16, 16, 10));
}

#[test]
fn valley_draws_set_turns_and_steps() {
    let t = relief::valley_turn;
    assert_eq!((t(0), t(1), t(-2), t(4), t(-6), t(7), t(-36), t(37)), (Some((0, 20)), Some((1, 10)), Some((-2, 5)), Some((4, 4)), Some((-6, 4)), Some((15, 3)), Some((-10, 3)), None));
}

// --- the land ------------------------------------------------------------------------------

#[test]
fn the_class_shares_land_on_the_cut_targets() {
    let mut bench = Bench::new();
    let mut c = bench.ctx(100, options(100, LAND), 12345);
    let b = relief::relief(&mut c, true).unwrap();
    let cum = relief::targets(10_000, &relief::weights(&c.o.shares, 1)).unwrap();
    let mut n = [0i64; 9];
    for y in 0..100 {
        for x in 0..100 {
            for (k, nk) in n.iter_mut().enumerate().skip(1) {
                *nk += b.holds(k, c.grid.h[gi(x, y)]) as i64;
            }
        }
    }
    // Each class within a histogram bin of its target.
    for k in 1..=8 {
        let want = cum[k] - cum[k - 1];
        assert!((n[k] - want).abs() <= 30, "class {k}: {} for {want}", n[k]);
    }
    // The band split lands on its own targets.
    land::classes(&mut c, &b);
    let band = count(&c.cells, 8) as i64;
    land::band(&mut c).unwrap();
    let (sand, low) = (count(&c.cells, 10) as i64, count(&c.cells, 5) as i64);
    let (marsh, swamp) = (count(&c.cells, 8) as i64, count(&c.cells, 9) as i64);
    assert_eq!((sand, low), (ext::round_div(54 * band, 100), ext::round_div(21 * band, 100)));
    assert_eq!(swamp, ext::round_div(59 * (marsh + swamp), 100));
    assert_eq!(sand + low + marsh + swamp, band);
}

#[test]
fn sand_grows_only_from_the_shallows() {
    for (water, sand) in [('~', true), ('=', false)] {
        let mut bench = Bench::new();
        let mut c = bench.ctx(12, Options::default(), 3);
        let w: String = std::iter::repeat_n(water, 12).collect();
        let rows = [w.as_str(), &w, &w, "bbbbbbbbbbbb", "bbbbbbbbbbbb", "bbbbbbbbbbbb", "bbbbbbbbbbbb", "............", "............", "............", "............", "............"];
        paint(&mut c, &rows);
        land::band(&mut c).unwrap();
        assert_eq!(count(&c.cells, 10) > 0, sand, "water {water}");
    }
}

#[test]
fn lowland_that_cannot_grow_stops_instead_of_hanging() {
    let mut bench = Bench::new();
    let o = Options { shares: Shares { sl1: 0, sl3: 0, ..Shares::default() }, ..Options::default() };
    let mut c = bench.ctx(8, o, 3);
    paint(&mut c, &["^^^^^^^^", "^^^^^^^^", "^^bbbb^^", "^^bbbb^^", "^^bbbb^^", "^^^^^^^^", "^^^^^^^^", "^^^^^^^^"]);
    assert_eq!(land::band(&mut c), Err(Stop::Hang(Stage::MoreLowland)));
}

#[test]
fn swamp_takes_the_water_side_first_and_always_draws_once() {
    let rows = ["........", "........", "..~~~~..", "..bbbb..", "........", "..bbbb..", "........", "........"];
    let swamps = |sw1: i32, rows: &[&str]| {
        let mut bench = Bench::new();
        let o = Options { shares: Shares { sw1, ..Shares::default() }, ..Options::default() };
        let mut c = bench.ctx(8, o, 5);
        paint(&mut c, rows);
        land::swamp(&mut c, 0).unwrap();
        let by_water = (2..6).filter(|&x| c.cells.mark[c.cells.idx(x, 3)] == 9).count();
        (count(&c.cells, 9), by_water)
    };
    let near: Vec<&str> = rows.iter().enumerate().map(|(y, r)| if y == 5 { "........" } else { r }).collect();
    // Four marsh cells by the water, a share of 59: Round(2.36) of them.
    assert_eq!(swamps(44, &near), (2, 2));
    // A share of 0 still draws once, and every water-side cell is a hit.
    assert_eq!(swamps(103, &near), (1, 1));
    // Eight marsh cells: Round(4.72) = 5 > 4 by the water, so all four and one more.
    assert_eq!(swamps(44, &rows), (5, 4));
}

#[test]
fn mountains_and_hills_fill_blocks_from_the_bottom_right() {
    let mut bench = Bench::new();
    let mut c = bench.ctx(10, Options::default(), 9);
    paint(&mut c, &["..........", ".mmm......", ".mmm......", ".mmm......", "..........", "......hh..", "......h...", "..........", "..........", ".........."]);
    land::mountains(&mut c);
    let o = c.cells.obj[c.cells.idx(3, 3)];
    assert_eq!(o >> 8, 5);
    assert!((30..34).contains(&(o & 0xff)));
    assert_eq!(c.cells.obj.iter().filter(|&&o| o != 0).count(), 1);
    land::hills(&mut c).unwrap();
    // Three hill markers of four take a 2 × 2 block, its object on the plain cell.
    let o = c.cells.obj[c.cells.idx(7, 6)];
    assert_eq!(o >> 8, 1);
    assert!((20..28).contains(&(o & 0xff)));
    assert_eq!(c.cells.mark[c.cells.idx(7, 6)], HILL, "the covered plain cell is hill-covered");
}

#[test]
fn random_hills_never_try_blocks_on_row_or_column_0() {
    let mut bench = Bench::new();
    let mut c = bench.ctx(20, options(20, LAND), 11);
    let full: String = "hhhhhh".to_string() + &".".repeat(14);
    let plain = ".".repeat(20);
    let rows: Vec<&str> = (0..20).map(|y| if y < 6 { full.as_str() } else { plain.as_str() }).collect();
    paint(&mut c, &rows);
    land::hills(&mut c).unwrap();
    assert!(c.cells.obj.iter().all(|&o| o == 0 || (o & 0xff) < 60), "no size-6 hill");
}

#[test]
fn a_small_forest_pool_stops_after_the_first_cluster() {
    // Almost no forest: under 30 pool cells on a 50 map.
    let o = Options { shares: Shares { f0: 99, f2: 1, ..Shares::default() }, ..options(50, LAND) };
    let out = run(o, 2024);
    assert_eq!(out.stop, Some(Stop::SmallForestPool));
    let trees = out.cells.tree.iter().filter(|&&t| t != 0).count();
    assert!(trees > 0 && trees <= 51, "{trees} trees");
    assert!(!out.complete());
}

/// The original's cluster loop as it is written: a scan of the pool for every sweep and a
/// new listing of the map after every cluster.
fn literal_clusters(c: &mut Ctx) {
    let families = c.sprites.families();
    let pool = |c: &Ctx| -> Vec<usize> { (0..c.cells.mark.len()).filter(|&i| c.grid.v[gi(i as i32 % c.w, i as i32 / c.w)] > 1).collect() };
    let mut p = pool(c);
    p.iter().for_each(|&i| c.cells.flag[i] = 0);
    let q = p.len() as i32 / *c.divisor;
    while !p.is_empty() {
        let seed = p[c.rng.random(p.len() as i32) as usize];
        c.cells.flag[seed] = 1;
        let mut budget = c.w;
        for &i in &p {
            if c.cells.flag[i] == 0 {
                continue;
            }
            let (px, py) = (i as i32 % c.w, i as i32 / c.w);
            for k in 0..12 {
                let (nx, ny) = (px + land::DX[k], py + land::DY[k]);
                if 0 < ny && ny < c.h - 1 && 0 < nx && nx < c.w - 1 {
                    let nb = c.cells.idx(nx, ny);
                    if c.cells.flag[nb] == 0 && c.grid.v[gi(nx, ny)] > 1 && budget > 0 {
                        c.cells.flag[nb] = 1;
                        budget -= 1;
                    }
                }
            }
        }
        let mut f = families[c.rng.random(families.len() as i32) as usize];
        if f == 6 || f == 7 {
            f = 0;
        }
        for &i in &p {
            if c.cells.flag[i] == 0 {
                continue;
            }
            let fc = c.grid.v[gi(i as i32 % c.w, i as i32 / c.w)];
            let mut cls = if fc == 3 { 11 } else if f < 8 { 9 } else { 10 };
            let mut v = f;
            match c.cells.ground[i] {
                8 => v = [9, 9, 9, 5, 5, 4][c.rng.random(6) as usize],
                5 => v = [4, 4, 5][c.rng.random(3) as usize],
                10 => v = [5, 5, 4][c.rng.random(3) as usize],
                _ if f == 4 || f == 5 => v = 0,
                _ => {}
            }
            if v == 4 {
                cls = 9;
            }
            let id = (cls as u16) * 256 + 12 * v as u16 + c.rng.random(c.sprites.plants(cls, v)) as u16;
            if c.sprites.loaded(id) {
                c.cells.tree[i] = if c.rng.random(5) == 0 && c.sprites.loaded(id + 120) { id + 120 } else { id };
                c.cells.mark[i] = if fc == 2 { land::TREES } else { land::THICKET };
                c.grid.v[gi(i as i32 % c.w, i as i32 / c.w)] = 0;
            }
        }
        if (p.len() as i32 + 1) / q < *c.divisor {
            *c.divisor -= 1;
        }
        p = pool(c);
        p.iter().for_each(|&i| c.cells.flag[i] = 0);
    }
}

#[test]
fn the_counted_pool_plants_as_the_original_loop() {
    for (kind, seed) in [(LAND, 31), (ISLAND, 32), (ESTUARY, 33)] {
        let mut bench = Bench::new();
        let mut c = bench.ctx(100, options(100, kind), seed);
        *c.divisor = 120;
        let b = relief::relief(&mut c, true).unwrap();
        land::terrain(&mut c, &b).unwrap();
        land::mountains(&mut c);
        land::hills(&mut c).unwrap();
        land::forest_field(&mut c).unwrap();
        let (cells, v, rng) = (c.cells.clone(), c.grid.v.clone(), c.rng.clone());
        land::clusters(&mut c).unwrap();
        let (fast, fast_rng, fast_div) = (c.cells.clone(), c.rng.state(), *c.divisor);
        c.cells = cells;
        c.grid.v = v;
        c.rng = rng;
        *c.divisor = 120;
        literal_clusters(&mut c);
        assert_eq!((hash(&c.cells), c.rng.state(), *c.divisor), (hash(&fast), fast_rng, fast_div), "kind {kind}");
        assert!(fast.tree.iter().filter(|&&t| t != 0).count() > 1000);
    }
}

#[test]
fn trees_that_never_have_a_picture_stop_the_forest() {
    // The built-in palette has only family 0: marsh trees can never be drawn.
    let mut g = Generator::new();
    let job = Job { options: options(50, LAND), seed: 77, sprites: Sprites::from_palette(&Palette::fallback()), start: Cells::zero(50), clock: 1 };
    let out = g.run(job, &AtomicBool::new(false));
    assert_eq!(out.stop, Some(Stop::Hang(Stage::Forest)));
}

#[test]
fn massif_counts_spill_into_the_next_class_as_the_loaders() {
    let p = Palette { objects: vec![ObjectKey { class: 1, sprite: 21 }, ObjectKey { class: 4, sprite: 81 }, ObjectKey { class: 5, sprite: 12 }], ..Palette::default() };
    let s = Sprites::from_palette(&p);
    assert_eq!((s.hills(2), s.mountains(1)), (1, 2));
}

// --- whole runs ----------------------------------------------------------------------------

#[test]
fn the_same_seed_and_options_give_the_same_map() {
    let cases = [
        (options(100, LAND), 12345, 0xa4fa62b3f256b760u64),
        (Options { orient: 1, blur: 0, ..options(50, ESTUARY) }, 99, 0x909233534c5d8bbe),
        (Options { blur: 4, ..options(100, ISLAND) }, 777, 0x849e4f6d6869088f),
        (Options { orient: 3, ..options(50, VALLEY) }, -5, 0xf5b80f63dfe7a616),
        (options(50, ARCHIPELAGO), 8, 0x51ddf571a54d46da),
        (Options { orient: 2, ..options(50, SKERRIES) }, 1, 0xac249acf0b416f4a),
        (options(50, LAKE), 2, 0xa25883ade2b082ba),
        (Options { orient: 1, ..options(50, COAST) }, 3, 0xee9ac67e01bd34e9),
    ];
    for (o, seed, want) in cases {
        let a = run(o, seed);
        let b = run(o, seed);
        assert_eq!(a.cells, b.cells);
        assert!(a.complete(), "{:?} {seed}: {:?}", o.kind, a.stop);
        assert_eq!(hash(&a.cells), want, "kind {} seed {seed}", o.kind);
    }
}

#[test]
fn the_seed_field_needs_keep_and_is_never_0() {
    let o = options(50, LAND);
    let typed = run(o, 5);
    assert_eq!((typed.seed, typed.header_seed), (5, None));
    let off = run(Options { keep: false, ..o }, 5);
    assert_eq!((off.seed, off.header_seed), (4242, Some(4242)));
    let zero = run(o, 0);
    assert_eq!((zero.seed, zero.header_seed), (4242, Some(4242)));
    assert_eq!(off.cells, zero.cells);
    assert_eq!(run(o, 4242).cells, zero.cells);
}

#[test]
fn reusing_the_relief_cuts_the_last_forest_field_and_keeps_old_trees() {
    let o = options(50, LAND);
    let mut g = Generator::new();
    let first = run_with(&mut g, o, 10, Cells::zero(50));
    let (grid, divisor) = (g.grid.clone(), g.divisor);
    let reuse = Options { rebuild: false, ..o };
    let again = run_with(&mut g, reuse, 10, first.cells.clone());
    // The forest's divisor is the one the last run counted down to 0: the reuse run stops
    // before its first cluster (Razdor's reading of the original's stale value).
    assert_eq!((divisor, again.stop), (0, Some(Stop::SmallForestPool)));
    assert_ne!(again.cells.terrain(), first.cells.terrain(), "a new map from the forest noise");
    // It depends only on what the grid held.
    let mut g2 = Generator { grid, divisor };
    assert_eq!(run_with(&mut g2, reuse, 10, first.cells.clone()).cells, again.cells);
    // The first run's trees stay (the tree slot is not cleared), now on any terrain.
    assert_eq!(again.cells.tree, first.cells.tree);
    // With a divisor a relief run set, the reuse run plants over them.
    let mut g3 = Generator { grid: g2.grid.clone(), divisor: 30 };
    let planted = run_with(&mut g3, reuse, 10, first.cells.clone());
    assert!(planted.complete());
    let kept = (0..planted.cells.mark.len()).filter(|&i| !matches!(planted.cells.mark[i], -9 | -11) && planted.cells.tree[i] != 0).count();
    assert!(kept > 0 && (0..planted.cells.mark.len()).all(|i| matches!(planted.cells.mark[i], -9 | -11) || planted.cells.tree[i] == first.cells.tree[i]));
    // A dialog that never ran has nothing to reuse: a flat field, and the cells untouched.
    let fresh = run_with(&mut Generator::new(), reuse, 10, first.cells.clone());
    assert_eq!(fresh.stop, Some(Stop::FlatField));
    assert_eq!(fresh.cells, first.cells);
}

#[test]
fn the_break_button_stops_a_run() {
    let out = Generator::new().run(Job { options: options(200, LAND), seed: 1, sprites: sprites(), start: Cells::zero(200), clock: 1 }, &AtomicBool::new(true));
    assert_eq!(out.stop, Some(Stop::Break));
}

#[test]
fn the_new_document_has_the_original_defaults() {
    let mut old = crate::editor::defaults::new_scenario(crate::editor::NewMap::default());
    old.header.start_time = 77;
    old.description = "old".into();
    let out = run(options(50, LAND), 31337);
    let s = new_scenario(&out, &old, "T");
    assert_eq!((s.header.width, s.header.height, s.header.generator_seed, s.header.start_time), (50, 50, 31337, 0));
    assert_eq!(s.header.relations, DEFAULT_RELATIONS);
    assert!(s.header.heroes.iter().all(|h| h.gold == 0 && h.x == 0));
    assert_eq!((s.title.as_str(), s.description.as_str(), s.buildings.len(), s.terrain.len()), ("T", "", 0, 2500));
    assert_eq!(s.objects, out.cells.objects());
    // A run that stopped keeps the old header but for the size, the clock seed and the
    // relations.
    let stopped = Outcome { stop: Some(Stop::Break), header_seed: Some(9), ..out };
    let s = new_scenario(&stopped, &old, "T");
    assert_eq!((s.header.start_time, s.header.generator_seed, s.header.width), (77, 9, 50));
    assert!(s.header.heroes.iter().all(|h| h.gold == 500));
}

#[test]
fn the_install_sprites_give_a_pinned_map() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
    let sprites = Sprites::from_palette(&Palette::from_sprites(&dt.map_objects().unwrap()));
    let o = options(200, ISLAND);
    let out = Generator::new().run(Job { options: o, seed: 2026, sprites, start: Cells::zero(200), clock: 1 }, &AtomicBool::new(false));
    assert!(out.complete());
    assert_eq!(hash(&out.cells), 0xfa37bae8d5550c8a);
}

