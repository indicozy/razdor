//! The depth regions of a sprite, cut into cards: a port of `tools/depth/cut_sprites.py`, its
//! plain pass (the bends pass for castles is not ported yet).
//!
//! The steps and constants are the script's. Where the script walks a Python set (no fixed order),
//! this port takes the smallest pair or region first, so a tie always goes the same way.
//! Contours come from imageproc (Suzuki-Abe, as OpenCV's), simplified with Douglas-Peucker and
//! triangulated with earcutr (a port of mapbox earcut, as the script's `mapbox_earcut`).

use std::collections::BTreeSet;

use earcutr::earcut;
use image::GrayImage;
use imageproc::contours::{find_contours, BorderType, Contour};
use imageproc::geometry::approximate_polygon_dp;
use imageproc::distance_transform::Norm;
use imageproc::morphology::dilate;
use imageproc::point::Point;

use crate::dt::cards::Card;
use crate::dt::gfx::Image;

/// Pixels at least this opaque are the sprite. Shadows (alpha 153 to 187) are not part of any card.
pub const ALPHA_OPAQUE: u8 = 204;
/// Fewer opaque pixels than this: no cards.
const MIN_OPAQUE: usize = 200;
/// Neighbours join a region when their normalised depths differ by at most this.
const DEPTH_EDGE: f32 = 0.06;
/// A region under max(SMALL_MIN_PX, SMALL_FRAC of the opaque pixels) merges into a neighbour.
const SMALL_MIN_PX: f64 = 60.0;
const SMALL_FRAC: f64 = 0.015;
/// More regions than this merge pairwise by mean depth.
const MAX_REGIONS: usize = 8;
/// A region with another region directly below this share of its bottom pixels is occluded.
const OCCLUDED_FRAC: f64 = 0.30;
/// Douglas-Peucker tolerance of the outlines, in sprite pixels.
const SIMPLIFY_EPS: f64 = 1.0;

/// The sprite's cards, back to front (foot y ascending). Empty when the sprite stays one card:
/// too few opaque pixels, one region, or fewer than two cards. `depth` is the sprite's depth map
/// (`width * height`, higher = closer), as [`super::DepthModel::depth`] gives it.
pub fn cut(img: &Image, depth: &[f32]) -> Vec<Card> {
    let (w, h) = (img.width as usize, img.height as usize);
    let n = w * h;
    if n == 0 || depth.len() != n || img.rgba.len() != n * 4 {
        return Vec::new();
    }
    let op: Vec<bool> = img.rgba.chunks_exact(4).map(|p| p[3] >= ALPHA_OPAQUE).collect();
    let n_op = op.iter().filter(|&&o| o).count();
    if n_op < MIN_OPAQUE {
        return Vec::new();
    }
    let d = normalise(depth, &op);
    let (lab, k) = merge_regions(label_regions(&op, &d, w, h), &d, w, h, n_op);
    if k < 2 {
        return Vec::new();
    }
    let mut cards = build_cards(&lab, w, h, k, &d);
    if cards.len() < 2 {
        return Vec::new();
    }
    cards.sort_by(|a, b| a.foot.1.total_cmp(&b.foot.1).then(a.foot.0.total_cmp(&b.foot.0)));
    cards
}

/// Depth scaled to 0..1 over the opaque pixels (all 0 when they are flat).
fn normalise(depth: &[f32], op: &[bool]) -> Vec<f32> {
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for (&v, &o) in depth.iter().zip(op) {
        if o {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    if hi > lo {
        depth.iter().map(|&v| (v - lo) / (hi - lo)).collect()
    } else {
        vec![0.0; depth.len()]
    }
}

fn find_root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find_root(parent, a), find_root(parent, b));
    if ra != rb {
        parent[ra.max(rb)] = ra.min(rb);
    }
}

/// The opaque pixels' regions: 4-connected, neighbours joined when their depths differ by at most
/// `DEPTH_EDGE` (a union-find). Labels run 0.. in raster order of each region's first pixel; -1
/// outside the sprite.
fn label_regions(op: &[bool], d: &[f32], w: usize, h: usize) -> Vec<i32> {
    let n = w * h;
    let mut parent: Vec<usize> = (0..n).collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !op[i] {
                continue;
            }
            if x + 1 < w && op[i + 1] && (d[i] - d[i + 1]).abs() <= DEPTH_EDGE {
                union(&mut parent, i, i + 1);
            }
            if y + 1 < h && op[i + w] && (d[i] - d[i + w]).abs() <= DEPTH_EDGE {
                union(&mut parent, i, i + w);
            }
        }
    }
    let mut id_of_root = vec![-1i32; n];
    let mut next = 0;
    let mut lab = vec![-1i32; n];
    for i in 0..n {
        if op[i] {
            let r = find_root(&mut parent, i);
            if id_of_root[r] < 0 {
                id_of_root[r] = next;
                next += 1;
            }
            lab[i] = id_of_root[r];
        }
    }
    lab
}

/// Renumbers the labels 0..k in their order (gaps closed), returns k.
fn compact(lab: &mut [i32]) -> usize {
    let max = lab.iter().copied().max().unwrap_or(-1);
    if max < 0 {
        return 0;
    }
    let mut used = vec![false; max as usize + 1];
    for &l in lab.iter() {
        if l >= 0 {
            used[l as usize] = true;
        }
    }
    let mut map = vec![-1i32; used.len()];
    let mut k = 0;
    for (l, &u) in used.iter().enumerate() {
        if u {
            map[l] = k;
            k += 1;
        }
    }
    for l in lab.iter_mut() {
        if *l >= 0 {
            *l = map[*l as usize];
        }
    }
    k as usize
}

/// Region r joins region t; the labels are renumbered and the count returned.
fn merge_into(lab: &mut [i32], r: usize, t: usize) -> usize {
    for l in lab.iter_mut() {
        if *l == r as i32 {
            *l = t as i32;
        }
    }
    compact(lab)
}

struct Stats {
    cnt: Vec<f64>,
    /// Mean normalised depth.
    md: Vec<f64>,
    /// Mean x and y.
    mx: Vec<f64>,
    my: Vec<f64>,
}

fn stats(lab: &[i32], d: &[f32], w: usize, k: usize) -> Stats {
    let mut s = Stats { cnt: vec![0.0; k], md: vec![0.0; k], mx: vec![0.0; k], my: vec![0.0; k] };
    for (i, &l) in lab.iter().enumerate() {
        if l < 0 {
            continue;
        }
        let r = l as usize;
        let (x, y) = ((i % w) as f64, (i / w) as f64);
        s.cnt[r] += 1.0;
        s.md[r] += f64::from(d[i]);
        s.mx[r] += x;
        s.my[r] += y;
    }
    for r in 0..k {
        s.md[r] /= s.cnt[r];
        s.mx[r] /= s.cnt[r];
        s.my[r] /= s.cnt[r];
    }
    s
}

/// The pairs of touching regions, as (smaller, larger).
fn adjacency(lab: &[i32], w: usize, h: usize) -> BTreeSet<(usize, usize)> {
    let mut pairs = BTreeSet::new();
    let mut add = |a: i32, b: i32| {
        if a >= 0 && b >= 0 && a != b {
            pairs.insert((a.min(b) as usize, a.max(b) as usize));
        }
    };
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if x + 1 < w {
                add(lab[i], lab[i + 1]);
            }
            if y + 1 < h {
                add(lab[i], lab[i + w]);
            }
        }
    }
    pairs
}

/// Small regions merge into the touching region with the closest mean depth (or the nearest
/// region, when none touches); then more than `MAX_REGIONS` merge pairwise by mean depth, the
/// closest pair first (a pair of regions that do not touch is allowed only when no two do).
/// Returns the labels and their count.
fn merge_regions(mut lab: Vec<i32>, d: &[f32], w: usize, h: usize, n_op: usize) -> (Vec<i32>, usize) {
    let mut k = compact(&mut lab);
    let thr = SMALL_MIN_PX.max(SMALL_FRAC * n_op as f64);
    while k > 1 {
        let s = stats(&lab, d, w, k);
        let Some(r) = (0..k).filter(|&q| s.cnt[q] < thr).min_by(|&a, &b| s.cnt[a].total_cmp(&s.cnt[b])) else {
            break;
        };
        let neighbours: Vec<usize> = adjacency(&lab, w, h)
            .into_iter()
            .filter_map(|(a, b)| if a == r { Some(b) } else if b == r { Some(a) } else { None })
            .collect();
        let t = if neighbours.is_empty() {
            let dist2 = |q: usize| (s.mx[q] - s.mx[r]).powi(2) + (s.my[q] - s.my[r]).powi(2);
            (0..k).filter(|&q| q != r).min_by(|&a, &b| dist2(a).total_cmp(&dist2(b))).unwrap_or(r)
        } else {
            neighbours
                .into_iter()
                .min_by(|&a, &b| (s.md[r] - s.md[a]).abs().total_cmp(&(s.md[r] - s.md[b]).abs()))
                .unwrap_or(r)
        };
        if t == r {
            break;
        }
        k = merge_into(&mut lab, r, t);
    }
    while k > MAX_REGIONS {
        let s = stats(&lab, d, w, k);
        let mut pairs = adjacency(&lab, w, h);
        if pairs.is_empty() {
            pairs = (0..k).flat_map(|a| (a + 1..k).map(move |b| (a, b))).collect();
        }
        let Some(&(a, b)) = pairs.iter().min_by(|p, q| {
            (s.md[p.0] - s.md[p.1]).abs().total_cmp(&(s.md[q.0] - s.md[q.1]).abs())
        }) else {
            break;
        };
        k = merge_into(&mut lab, b, a);
    }
    (lab, k)
}

/// Each region's ground point: its foot (the mean x of its bottom pixels, and the row where it
/// meets the ground), rounded to a tenth. An occluded region (another region directly below 30 %
/// or more of its bottom pixels) moves down along the ground line fitted through the unoccluded
/// regions: depth against row, the line where depth grows with the row (closer, lower).
fn feet(lab: &[i32], d: &[f32], w: usize, h: usize, k: usize) -> Vec<(f32, f32)> {
    let mut pixels: Vec<Vec<(usize, usize)>> = vec![Vec::new(); k];
    for y in 0..h {
        for x in 0..w {
            let l = lab[y * w + x];
            if l >= 0 {
                pixels[l as usize].push((x, y));
            }
        }
    }
    // (bottom row, foot x, depth at the bottom, occluded) per region
    let rows: Vec<(f64, f64, f64, bool)> = pixels
        .iter()
        .enumerate()
        .map(|(r, pts)| {
            let bottom = pts.iter().map(|p| p.1).max().unwrap_or(0);
            let near: Vec<&(usize, usize)> = pts.iter().filter(|p| p.1 + 2 >= bottom).collect();
            let bx = near.iter().map(|p| p.0 as f64).sum::<f64>() / near.len() as f64;
            let dlow = near.iter().map(|p| f64::from(d[p.1 * w + p.0])).sum::<f64>() / near.len() as f64;
            let sel: Vec<&(usize, usize)> = pts.iter().filter(|p| p.1 + 1 >= bottom).collect();
            let other = sel
                .iter()
                .filter(|p| {
                    p.1 + 1 < h && {
                        let l = lab[(p.1 + 1) * w + p.0];
                        l >= 0 && l as usize != r
                    }
                })
                .count();
            let occluded = other as f64 / sel.len() as f64 >= OCCLUDED_FRAC;
            (bottom as f64, bx, dlow, occluded)
        })
        .collect();

    let ground: Vec<(f64, f64)> = rows.iter().filter(|r| !r.3).map(|r| (r.0, r.2)).collect();
    let mut fit = None;
    if ground.len() >= 2 {
        let n = ground.len() as f64;
        let xm = ground.iter().map(|p| p.0).sum::<f64>() / n;
        let ym = ground.iter().map(|p| p.1).sum::<f64>() / n;
        let sxx: f64 = ground.iter().map(|p| (p.0 - xm).powi(2)).sum();
        if sxx > 0.0 {
            let b = ground.iter().map(|p| (p.0 - xm) * (p.1 - ym)).sum::<f64>() / sxx;
            if b > 0.0 {
                fit = Some((ym - b * xm, b));
            }
        }
    }

    rows.iter()
        .map(|&(bottom, bx, dlow, occluded)| {
            let mut y = bottom;
            if let (true, Some((a, b))) = (occluded, fit) {
                y = ((dlow - a) / b).max(bottom).min((h - 1) as f64);
            }
            (round1(bx) as f32, round1(y) as f32)
        })
        .collect()
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round_ties_even() / 10.0
}

/// The index of the outer border that encloses border `j` (the hole's own outline, for a hole).
fn enclosing_outer(contours: &[Contour<i32>], mut j: usize) -> Option<usize> {
    loop {
        let p = contours[j].parent?;
        if contours[p].border_type == BorderType::Outer {
            return Some(p);
        }
        j = p;
    }
}

fn sq(p: Point<i32>, q: Point<i32>) -> i64 {
    let (dx, dy) = (i64::from(p.x - q.x), i64::from(p.y - q.y));
    dx * dx + dy * dy
}

/// OpenCV's CHAIN_APPROX_SIMPLE on a closed 8-connected outline: a point inside a straight run
/// (the same step before and after it) goes; only the corners stay.
fn chain_simple(pts: &[Point<i32>]) -> Vec<Point<i32>> {
    let n = pts.len();
    (0..n)
        .filter(|&i| {
            let (prev, cur, next) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            (cur.x - prev.x, cur.y - prev.y) != (next.x - cur.x, next.y - cur.y)
        })
        .map(|i| pts[i])
        .collect()
}

/// Douglas-Peucker on a closed outline with OpenCV's split: the curve is cut at the point farthest
/// from its first point, and each half is simplified as an open curve. (imageproc's
/// `approximate_polygon_dp` treats its input as open, which is wrong for a closed ring.)
fn closed_dp(pts: &[Point<i32>]) -> Vec<Point<i32>> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let p0 = pts[0];
    let mut m = 1;
    let mut far = -1;
    for (i, &p) in pts.iter().enumerate().skip(1) {
        let d = sq(p, p0);
        if d > far {
            far = d;
            m = i;
        }
    }
    let first = approximate_polygon_dp(&pts[..=m], SIMPLIFY_EPS, false);
    let mut rest = pts[m..].to_vec();
    rest.push(p0);
    let second = approximate_polygon_dp(&rest, SIMPLIFY_EPS, false);
    // `first` runs p0..pm and `second` pm..p0: the shared ends are kept once, the outline closes.
    let mut out = first;
    out.extend_from_slice(&second[1..second.len() - 1]);
    out
}

/// A card's outline (`verts`) and triangles (`tris`).
type Shape = (Vec<(f32, f32)>, Vec<u16>);

/// The outline of region r as a card: its pixels dilated by 1 px, the outer rings with their holes,
/// simplified and triangulated. `None` when nothing triangulates.
fn region_card(lab: &[i32], w: usize, h: usize, r: usize) -> Option<Shape> {
    let mask: Vec<u8> = lab.iter().map(|&l| if l == r as i32 { 255 } else { 0 }).collect();
    let mask = GrayImage::from_raw(w as u32, h as u32, mask)?;
    let grown = dilate(&mask, Norm::LInf, 1);
    // The tracer needs background around the shape: a region touching the sprite's edge is traced
    // on a copy with a 1 px frame (OpenCV's tracer needs none), and the points move back.
    let mut framed = GrayImage::new(w as u32 + 2, h as u32 + 2);
    for (x, y, p) in grown.enumerate_pixels() {
        framed.put_pixel(x + 1, y + 1, *p);
    }
    let mut contours: Vec<Contour<i32>> = find_contours(&framed);
    for c in contours.iter_mut() {
        for p in c.points.iter_mut() {
            p.x -= 1;
            p.y -= 1;
        }
    }
    let mut verts: Vec<(f32, f32)> = Vec::new();
    let mut tris: Vec<u16> = Vec::new();
    for (i, outer) in contours.iter().enumerate() {
        if outer.border_type != BorderType::Outer {
            continue;
        }
        let outer_ring = closed_dp(&chain_simple(&outer.points));
        if outer_ring.len() < 3 {
            continue;
        }
        let mut rings = vec![outer_ring];
        for (j, hole) in contours.iter().enumerate() {
            if hole.border_type == BorderType::Hole && enclosing_outer(&contours, j) == Some(i) {
                let ring = closed_dp(&chain_simple(&hole.points));
                if ring.len() >= 3 {
                    rings.push(ring);
                }
            }
        }
        let mut flat: Vec<f64> = Vec::new();
        let mut hole_starts: Vec<usize> = Vec::new();
        for (ri, ring) in rings.iter().enumerate() {
            if ri > 0 {
                hole_starts.push(flat.len() / 2);
            }
            for p in ring {
                flat.push(f64::from(p.x));
                flat.push(f64::from(p.y));
            }
        }
        let Ok(t) = earcut(&flat, &hole_starts, 2) else { continue };
        if t.is_empty() {
            continue;
        }
        let base = verts.len();
        if base + flat.len() / 2 > usize::from(u16::MAX) {
            return None;
        }
        verts.extend(flat.chunks_exact(2).map(|v| (v[0] as f32, v[1] as f32)));
        tris.extend(t.iter().map(|&v| (v + base) as u16));
    }
    if tris.is_empty() {
        return None;
    }
    Some((verts, tris))
}

/// One card per region that triangulates, with the region's foot. Not sorted.
fn build_cards(lab: &[i32], w: usize, h: usize, k: usize, d: &[f32]) -> Vec<Card> {
    feet(lab, d, w, h, k)
        .into_iter()
        .enumerate()
        .filter_map(|(r, foot)| {
            let (verts, tris) = region_card(lab, w, h, r)?;
            Some(Card { foot, verts, tris, ground: false })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sprite from (x, y, depth) cells: opaque where a cell is given, transparent elsewhere.
    fn sprite(w: u32, h: u32, cells: impl Fn(u32, u32) -> Option<f32>) -> (Image, Vec<f32>) {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let mut depth = vec![0f32; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                if let Some(v) = cells(x, y) {
                    let i = (y * w + x) as usize;
                    rgba[i * 4..i * 4 + 4].copy_from_slice(&[90, 120, 60, 255]);
                    depth[i] = v;
                }
            }
        }
        (Image { width: w, height: h, rgba }, depth)
    }

    fn block(x0: u32, x1: u32, y0: u32, y1: u32, v: f32) -> impl Fn(u32, u32) -> Option<f32> {
        move |x, y| (x >= x0 && x <= x1 && y >= y0 && y <= y1).then_some(v)
    }

    fn covers(card: &Card, p: (f32, f32)) -> bool {
        let sign = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| (a.0 - c.0) * (b.1 - c.1) - (b.0 - c.0) * (a.1 - c.1);
        card.tris.chunks_exact(3).any(|t| {
            let v = |i: u16| card.verts[i as usize];
            let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
            let (d1, d2, d3) = (sign(p, a, b), sign(p, b, c), sign(p, c, a));
            let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
            let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
            !(neg && pos)
        })
    }

    #[test]
    fn two_separated_blocks_make_two_cards_with_feet_at_their_bottoms() {
        let left = block(2, 11, 5, 27, 0.2);
        let right = block(25, 36, 5, 27, 0.9);
        let (img, depth) = sprite(40, 30, |x, y| left(x, y).or(right(x, y)));
        let cards = cut(&img, &depth);
        assert_eq!(cards.len(), 2);
        // Back to front: the same foot row, so the left one (the smaller x) first.
        assert_eq!(cards[0].foot, (6.5, 27.0));
        assert_eq!(cards[1].foot, (30.5, 27.0));
        assert!(covers(&cards[0], (6.5, 15.0)) && !covers(&cards[0], (30.5, 15.0)));
    }

    #[test]
    fn one_block_stays_one_card() {
        let (img, depth) = sprite(40, 30, block(2, 30, 5, 27, 0.5));
        assert!(cut(&img, &depth).is_empty());
    }

    #[test]
    fn too_few_opaque_pixels_make_no_cards() {
        let (img, depth) = sprite(40, 30, block(2, 9, 5, 12, 0.2));
        assert!(cut(&img, &depth).is_empty(), "64 opaque pixels are under 200");
    }

    #[test]
    fn a_hole_stays_a_hole() {
        // A 20x20 block with a transparent 8x8 hole, and a second block of another depth.
        let block_hole = |x: u32, y: u32| {
            let outer = (2..=21).contains(&x) && (2..=21).contains(&y);
            let hole = (7..=14).contains(&x) && (7..=14).contains(&y);
            (outer && !hole).then_some(0.2)
        };
        let other = block(26, 35, 2, 21, 0.9);
        let (img, depth) = sprite(40, 24, |x, y| block_hole(x, y).or(other(x, y)));
        let cards = cut(&img, &depth);
        assert_eq!(cards.len(), 2);
        let hole = (10.5, 10.5);
        assert!(cards.iter().all(|c| !covers(c, hole)), "the hole's centre is in no triangle");
        assert!(cards.iter().any(|c| covers(c, (4.5, 4.5))), "the solid part is covered");
    }

    #[test]
    fn an_occluded_region_stands_on_the_ground_fit() {
        // Region 0 (depth 0.4, bottom row 10) has region 1 (depth 0.2, rows 11..20) directly below
        // it: occluded. Regions 1 and 2 (bottom 40, depth 0.6) fit the ground line
        // depth = -0.2 + 0.02 * row, so region 0 stands at row (0.4 + 0.2) / 0.02 = 30.
        let (w, h) = (10usize, 45usize);
        let mut lab = vec![-1i32; w * h];
        let mut d = vec![0f32; w * h];
        let mut paint = |x0: usize, x1: usize, y0: usize, y1: usize, l: i32, v: f32| {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    lab[y * w + x] = l;
                    d[y * w + x] = v;
                }
            }
        };
        paint(0, 4, 5, 10, 0, 0.4);
        paint(0, 4, 11, 20, 1, 0.2);
        paint(5, 9, 21, 40, 2, 0.6);
        let feet = feet(&lab, &d, w, h, 3);
        assert_eq!(feet, vec![(2.0, 30.0), (2.0, 20.0), (7.0, 40.0)]);
    }

    #[test]
    fn a_region_on_the_sprite_edge_keeps_its_card() {
        // Block A touches the left and top edges of the sprite; its outline must still be traced.
        let a = block(0, 11, 0, 27, 0.2);
        let b = block(25, 36, 5, 27, 0.9);
        let (img, depth) = sprite(40, 30, |x, y| a(x, y).or(b(x, y)));
        let cards = cut(&img, &depth);
        assert_eq!(cards.len(), 2);
        assert!(covers(&cards[0], (3.0, 10.0)), "the edge block is covered by its card");
    }

    #[test]
    fn a_small_speck_merges_into_its_neighbour() {
        // Five pixels of a third depth touch block A: they join A (a region under 60 px), so two
        // cards remain.
        let a = block(2, 11, 5, 27, 0.2);
        let speck = block(12, 12, 10, 14, 0.5);
        let b = block(25, 36, 5, 27, 0.9);
        let (img, depth) = sprite(40, 30, |x, y| a(x, y).or(speck(x, y)).or(b(x, y)));
        assert_eq!(cut(&img, &depth).len(), 2);
    }

    #[test]
    fn more_than_eight_regions_merge_to_eight() {
        // Ten separated 8x8 blocks of distinct depths: none is small, none touches; the pairs of
        // closest depth merge until eight remain.
        let (img, depth) = sprite(52, 24, |x, y| {
            (0..10u32).find_map(|i| {
                let (c, r) = (i % 5, i / 5);
                let (x0, y0) = (1 + 10 * c, 1 + 11 * r);
                (x >= x0 && x < x0 + 8 && y >= y0 && y < y0 + 8).then_some(i as f32 / 10.0)
            })
        });
        assert_eq!(cut(&img, &depth).len(), 8);
    }

    #[test]
    fn a_square_outline_keeps_its_four_corners() {
        // The 40 unit steps around a 10 x 10 square: the edges' points go, the corners stay.
        let mut ring: Vec<Point<i32>> = Vec::new();
        ring.extend((0..10).map(|x| Point::new(x, 0)));
        ring.extend((0..10).map(|y| Point::new(10, y)));
        ring.extend((0..10).map(|x| Point::new(10 - x, 10)));
        ring.extend((0..10).map(|y| Point::new(0, 10 - y)));
        let out = closed_dp(&chain_simple(&ring));
        let mut corners: Vec<(i32, i32)> = out.iter().map(|p| (p.x, p.y)).collect();
        corners.sort();
        assert_eq!(corners, vec![(0, 0), (0, 10), (10, 0), (10, 10)]);
    }
}
